use std::time::Duration;

use anyhow::Context;
use regex::Regex;
use serde::{Deserialize, Serialize};

const RSS_URL: &str = "https://www.publicdomaintorrents.info/bt/rss.php";

/// Acota el peor caso por ítem — medido en vivo (2026-08-06): sin este
/// límite, `browse()` con ~30 items concurrentes tardó 13.6s de punta a
/// punta contra el sitio real, muy por encima de las demás fuentes
/// Online (~1.5s archive.org). Un ítem que no responde a tiempo se trata
/// igual que uno roto (se descarta, no aborta el resto) — no configurable
/// por el usuario, es resiliencia interna igual que `http_retry::MAX_ATTEMPTS`.
const DETAIL_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct PublicDomainMovie {
    pub title: String,
    /// URL de descarga del .torrent ya resuelta (mp4 preferido sobre avi,
    /// ver `parse_detail_page`) — a diferencia de `ArchiveOrgItem::identifier`,
    /// no alcanza con un id corto: qué formatos existe cada título es
    /// variable y solo se sabe visitando su página de detalle, así que el
    /// identifier acá es directamente la URL final para no repetir esa
    /// visita en `fetch_torrent_bytes`/`heal_media_item`.
    pub identifier: String,
}

fn item_re() -> Regex {
    Regex::new(r"(?s)<item>(.*?)</item>").unwrap()
}
fn rss_title_re() -> Regex {
    Regex::new(r"(?s)<title>(.*?)</title>").unwrap()
}
fn rss_link_re() -> Regex {
    Regex::new(r"(?s)<link>(.*?)</link>").unwrap()
}

/// RSS del catálogo (`rss.php`): cada item apunta a una página de detalle,
/// no al .torrent directo. Sin CDATA ni namespaces (a diferencia de
/// indexers/parse.rs) — verificado contra el feed real a 2026-08.
fn parse_rss(body: &str) -> Vec<(String, String)> {
    let item_re = item_re();
    let title_re = rss_title_re();
    let link_re = rss_link_re();
    item_re
        .captures_iter(body)
        .filter_map(|cap| {
            let item = cap.get(1)?.as_str();
            let title = title_re.captures(item)?.get(1)?.as_str().trim().to_string();
            let link = link_re.captures(item)?.get(1)?.as_str().trim().to_string();
            Some((title, link))
        })
        .collect()
}

fn detail_title_re() -> Regex {
    Regex::new(r"(?s)<h3>(.*?)</h3>").unwrap()
}

/// publicdomaintorrents.info no envuelve sus atributos `href` en comillas
/// (`<a href=http://...btdownload.php?...>`) — un regex `href="..."` pierde
/// estos links por completo, confirmado contra la página real.
fn detail_torrent_link_re() -> Regex {
    Regex::new(r#"href=(http://[^\s>"]+btdownload\.php\?[^\s>"]+\.torrent)"#).unwrap()
}

struct ParsedDetail {
    title: String,
    torrent_url: String,
}

/// El RSS trae el nombre de archivo (`To_the_Shores_of_Iwo_Jima`) como
/// título, no el título real con capitalización correcta (`To the Shores
/// of Iwo Jima`) — solo la página de detalle lo tiene. Del resto de links
/// de descarga disponibles, prefiere `.mp4` sobre `.avi` (mejor balance
/// tamaño/calidad) y, entre variantes mp4, evita la de PSP si hay una
/// genérica. `None` si la página no tiene ningún link de descarga (item
/// retirado del catálogo pero que igual quedó listado en el RSS).
fn parse_detail_page(html: &str) -> Option<ParsedDetail> {
    let title = detail_title_re().captures(html)?.get(1)?.as_str().trim().to_string();
    let links: Vec<String> = detail_torrent_link_re()
        .captures_iter(html)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();
    let preferred = links
        .iter()
        .find(|l| l.to_lowercase().ends_with(".mp4.torrent") && !l.to_lowercase().contains("_psp"))
        .or_else(|| links.iter().find(|l| l.to_lowercase().ends_with(".mp4.torrent")))
        .or_else(|| links.first())?
        .clone();
    Some(ParsedDetail { title, torrent_url: preferred })
}

/// Catálogo completo: RSS (lista + link a detalle) y, por cada item, su
/// página de detalle para resolver título real y link de descarga preferido
/// (no derivable del RSS solo). Concurrente vía `JoinSet`: el feed real trae
/// ~30 items, secuencial sería decenas de segundos de latencia percibida al
/// abrir la pestaña. Un item que falla (red, o página sin links) se
/// descarta sin abortar el resto.
pub async fn browse(client: &reqwest::Client) -> anyhow::Result<Vec<PublicDomainMovie>> {
    let rss_body = crate::http_retry::send_with_retry(|| client.get(RSS_URL))
        .await
        .context("no se pudo contactar publicdomaintorrents.info")?
        .text()
        .await
        .context("RSS de publicdomaintorrents.info con formato inesperado")?;

    let mut set = tokio::task::JoinSet::new();
    for (_, link) in parse_rss(&rss_body) {
        let client = client.clone();
        set.spawn(async move {
            let fetch = async {
                let resp = crate::http_retry::send_with_retry(|| client.get(&link)).await.ok()?;
                let html = resp.text().await.ok()?;
                parse_detail_page(&html)
            };
            // Envuelve la secuencia completa de reintentos, no un intento
            // individual — un ítem que agota DETAIL_FETCH_TIMEOUT en total
            // se descarta ahí mismo en vez de arrastrar el resto del batch.
            tokio::time::timeout(DETAIL_FETCH_TIMEOUT, fetch).await.ok().flatten()
        });
    }

    let mut movies = Vec::new();
    while let Some(result) = set.join_next().await {
        if let Ok(Some(detail)) = result {
            movies.push(PublicDomainMovie { title: detail.title, identifier: detail.torrent_url });
        }
    }
    Ok(movies)
}

/// `identifier` es la URL de descarga ya resuelta por `browse` (ver
/// `PublicDomainMovie`), así que a diferencia de `archive_org::fetch_torrent_bytes`
/// no hace falta reconstruir nada ni revisitar la página de detalle.
pub async fn fetch_torrent_bytes(client: &reqwest::Client, identifier: &str) -> anyhow::Result<Vec<u8>> {
    let resp = crate::http_retry::send_with_retry(|| client.get(identifier))
        .await
        .with_context(|| format!("no se pudo descargar {identifier}"))?
        .error_for_status()
        .with_context(|| format!("{identifier} respondió con error"))?;
    Ok(resp.bytes().await?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixture real (RSS obtenido en vivo contra publicdomaintorrents.info,
    /// 2026-08-06), trimmed a 2 items.
    const RSS_FIXTURE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xml:base="">
<channel>
<title>Public Domain Movie Torrents</title>
 <item> <title>Thirteenth_Guest</title>
<link>http://www.publicdomaintorrents.com/nshowmovie.html?rstitle=Thirteenth_Guest.avi</link>
<guid isPermaLink="false"></guid>
<description>Filename: Thirteenth_Guest.avi &lt;br /&gt;
Uploaders: 0&lt;br /&gt;
Downloaders: 0&lt;br /&gt;
Size: 0.00MB</description>
<enclosure
url="http://www.publicdomaintorrents.com/nshowmovie.html?rstitle=Thirteenth_Guest.avi"  />
 <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
</item>
 <item> <title>To_the_Shores_of_Iwo_Jima</title>
<link>http://www.publicdomaintorrents.com/nshowmovie.html?rstitle=To_the_Shores_of_Iwo_Jima.avi</link>
<guid isPermaLink="false"></guid>
<description>Filename: To_the_Shores_of_Iwo_Jima.avi &lt;br /&gt;</description>
<enclosure
url="http://www.publicdomaintorrents.com/nshowmovie.html?rstitle=To_the_Shores_of_Iwo_Jima.avi"  />
 <pubDate>Thu, 01 Jan 1970 00:00:00 +0000</pubDate>
</item>
</channel></rss>"#;

    #[test]
    fn parse_rss_extracts_title_and_detail_link_per_item() {
        let items = parse_rss(RSS_FIXTURE);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, "Thirteenth_Guest");
        assert_eq!(
            items[0].1,
            "http://www.publicdomaintorrents.com/nshowmovie.html?rstitle=Thirteenth_Guest.avi"
        );
        assert_eq!(items[1].0, "To_the_Shores_of_Iwo_Jima");
    }

    /// Fixture real (página de detalle de "Thirteenth_Guest", obtenida en
    /// vivo), trimmed al bloque de título y links de descarga. Confirma el
    /// caso con 3 variantes (avi/mp4/psp) — debe preferir la mp4 genérica.
    const DETAIL_FIXTURE_THREE_LINKS: &str = r#"
<h3>Thirteenth Guest</h3><br>
<tr><td><a
href=http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=Thirteenth_Guest.avi.torrent>Click for Divx 764MB AVI</a></td>
<td><img src=divxi.jpg></img></td>
</tr>
<tr><td><a
href=http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=Thirteenth_Guest.mp4.torrent>Click for IPOD MP4  192 MB</a></td><td><img src=ipod.jpg></img></td></tr><tr><td><a
href=http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=Thirteenth_Guest_PSP.MP4.torrent>Click for PSP MP4  440 MB</a></td><td><img src=psp.gif></img></td></tr>
"#;

    /// Fixture real (página de detalle de "To_the_Shores_of_Iwo_Jima"),
    /// caso con solo 2 variantes (avi/mp4, sin psp) — confirma que la
    /// capitalización del `<h3>` difiere del nombre de archivo del RSS
    /// (title-case real "To the Shores of Iwo Jima", no un underscore→space
    /// naive que daría "To The Shores Of Iwo Jima").
    const DETAIL_FIXTURE_TWO_LINKS: &str = r#"
<h3>To the Shores of Iwo Jima</h3><br>
<tr><td><a
href=http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=To_the_Shores_of_Iwo_Jima.avi.torrent>Click for Divx AVI</a></td></tr>
<tr><td><a
href=http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=To_the_Shores_of_Iwo_Jima.mp4.torrent>Click for IPOD MP4</a></td></tr>
"#;

    #[test]
    fn parse_detail_page_prefers_generic_mp4_over_avi_and_psp() {
        let detail = parse_detail_page(DETAIL_FIXTURE_THREE_LINKS).unwrap();
        assert_eq!(detail.title, "Thirteenth Guest");
        assert_eq!(
            detail.torrent_url,
            "http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=Thirteenth_Guest.mp4.torrent"
        );
    }

    #[test]
    fn parse_detail_page_extracts_real_title_case_distinct_from_rss_filename() {
        let detail = parse_detail_page(DETAIL_FIXTURE_TWO_LINKS).unwrap();
        assert_eq!(detail.title, "To the Shores of Iwo Jima");
        assert_eq!(
            detail.torrent_url,
            "http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=To_the_Shores_of_Iwo_Jima.mp4.torrent"
        );
    }

    #[test]
    fn parse_detail_page_falls_back_to_first_link_when_no_mp4_exists() {
        let html = r#"<h3>Only Avi</h3>
<a href=http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=Only_Avi.avi.torrent>Click for Divx AVI</a>"#;
        let detail = parse_detail_page(html).unwrap();
        assert_eq!(
            detail.torrent_url,
            "http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=Only_Avi.avi.torrent"
        );
    }

    #[test]
    fn parse_detail_page_returns_none_when_item_has_no_download_link() {
        // Caso hipotético (no observado en el catálogo real durante esta
        // sesión, todos los items muestreados tenían al menos un link) pero
        // defendido explícitamente por el diseño ("saltea items rotos") —
        // simula un item retirado que sigue listado en el RSS.
        let html = "<h3>Retirado</h3><p>Este título ya no está disponible.</p>";
        assert!(parse_detail_page(html).is_none());
    }

    #[test]
    fn parse_detail_page_returns_none_without_a_title() {
        let html = "<p>página inesperada, sin bloque de título</p>";
        assert!(parse_detail_page(html).is_none());
    }

    /// Red real, deshabilitado por defecto (misma convención que
    /// `archive_org` no tiene hoy y que otros módulos marcan `#[ignore]`).
    /// Corre con `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn browse_returns_real_movies_from_the_live_catalog() {
        let client = reqwest::Client::new();
        let movies = browse(&client).await.unwrap();
        assert!(!movies.is_empty(), "el catálogo en vivo no debería estar vacío");
        for movie in &movies {
            assert!(!movie.title.is_empty());
            assert!(movie.identifier.contains("btdownload.php"));
        }
    }

    #[tokio::test]
    #[ignore]
    async fn fetch_torrent_bytes_downloads_a_real_bencoded_torrent() {
        let client = reqwest::Client::new();
        let identifier = "http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=Thirteenth_Guest.mp4.torrent";
        let bytes = fetch_torrent_bytes(&client, identifier).await.unwrap();
        assert!(bytes.starts_with(b"d8:announce"), "debe ser un .torrent bencoded real, no HTML de error");
    }
}
