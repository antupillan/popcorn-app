use regex::Regex;

use super::{IndexerResult, JsonPaths};

fn magnet_re() -> Regex {
    // Excluye `]` además de espacio/comillas/ángulos: sin esto, un magnet
    // envuelto en CDATA (`<![CDATA[magnet:...]]>`) se come el `]]` de
    // cierre como si fuera parte de la URI.
    Regex::new(r#"magnet:\?[^\s"'<>\]]+"#).unwrap()
}

fn item_re() -> Regex {
    Regex::new(r"(?s)<item[^>]*>(.*?)</item>").unwrap()
}

fn tag_re(tag: &str) -> Regex {
    // Acepta prefijo de namespace opcional (ej. `nyaa:size`) y contenido
    // envuelto o no en CDATA — cubre los feeds RSS reales de indexers de
    // torrents sin necesitar un parser XML con soporte de namespaces.
    Regex::new(&format!(
        r#"(?s)<(?:[a-zA-Z0-9]+:)?{tag}[^>]*>\s*(?:<!\[CDATA\[(.*?)\]\]>|([^<]*))\s*</(?:[a-zA-Z0-9]+:)?{tag}>"#
    ))
    .unwrap()
}

fn capture_text(re: &Regex, haystack: &str) -> Option<String> {
    let caps = re.captures(haystack)?;
    caps.get(1)
        .or_else(|| caps.get(2))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

fn dn_from_magnet(magnet: &str) -> Option<String> {
    let dn = magnet.split('&').find_map(|part| part.strip_prefix("dn="))?;
    let decoded = urlencoding::decode(dn).ok()?;
    Some(decoded.replace('+', " "))
}

/// Página con magnets sueltos (sin estructura de items) — extrae todos los
/// `magnet:` únicos de la respuesta y usa el parámetro `dn` como título
/// cuando está presente. Cubre indexers que devuelven una lista simple.
pub fn magnet_list(body: &str) -> Vec<IndexerResult> {
    let magnet_re = magnet_re();
    let mut seen = std::collections::HashSet::new();
    magnet_re
        .find_iter(body)
        .map(|m| m.as_str().to_string())
        .filter(|magnet| seen.insert(magnet.clone()))
        .map(|magnet| IndexerResult {
            title: dn_from_magnet(&magnet).unwrap_or_else(|| "(sin título)".to_string()),
            magnet,
            size: None,
            seeders: None,
            source_indexer: String::new(),
        })
        .collect()
}

/// RSS de torrents (patrón Nyaa y similares): regex sobre bloques
/// `<item>...</item>`, no un parser XML estricto — alcanza para el
/// universo real de feeds RSS de indexers de torrents.
pub fn rss(body: &str) -> Vec<IndexerResult> {
    let item_re = item_re();
    let magnet_re = magnet_re();
    let title_re = tag_re("title");
    let size_re = tag_re("size");
    let seeders_re = tag_re("seeders");
    let infohash_re = tag_re("infoHash");

    item_re
        .captures_iter(body)
        .filter_map(|cap| {
            let item = cap.get(1)?.as_str();
            let title = capture_text(&title_re, item)?;
            // Feeds reales (ej. Nyaa.si a 2026-08) dejaron de incluir un
            // `magnet:` literal — solo traen `infoHash` + link al .torrent.
            // Sin este fallback, `rss()` devuelve cero resultados contra
            // ese formato (confirmado contra el feed real, no una hipótesis).
            let magnet = magnet_re.find(item).map(|m| m.as_str().to_string()).or_else(|| {
                capture_text(&infohash_re, item)
                    .map(|hash| format!("magnet:?xt=urn:btih:{hash}&dn={}", urlencoding::encode(&title)))
            })?;
            Some(IndexerResult {
                title,
                magnet,
                size: capture_text(&size_re, item),
                seeders: capture_text(&seeders_re, item),
                source_indexer: String::new(),
            })
        })
        .collect()
}

/// API JSON genérica: navega `items_path` (dot-path simple, sin índices de
/// array) hasta el array de resultados y lee cada campo por su propio
/// dot-path — cubre la mayoría de APIs de búsqueda reales sin requerir
/// JSONPath completo.
pub fn json(body: &str, paths: Option<&JsonPaths>) -> anyhow::Result<Vec<IndexerResult>> {
    let paths =
        paths.ok_or_else(|| anyhow::anyhow!("indexer JSON sin json_paths configurado"))?;
    let root: serde_json::Value = serde_json::from_str(body)?;
    let items = if paths.items_path.is_empty() {
        &root
    } else {
        dig(&root, &paths.items_path)
            .ok_or_else(|| anyhow::anyhow!("items_path '{}' no encontrado", paths.items_path))?
    };
    let items = items.as_array().ok_or_else(|| {
        anyhow::anyhow!("items_path '{}' no apunta a un array", paths.items_path)
    })?;

    Ok(items
        .iter()
        .filter_map(|item| {
            let title = dig(item, &paths.title_field)?.as_str()?.to_string();
            let magnet = dig(item, &paths.magnet_field)?.as_str()?.to_string();
            let size = paths
                .size_field
                .as_ref()
                .and_then(|f| dig(item, f))
                .map(value_to_string);
            let seeders = paths
                .seeders_field
                .as_ref()
                .and_then(|f| dig(item, f))
                .map(value_to_string);
            Some(IndexerResult {
                title,
                magnet,
                size,
                seeders,
                source_indexer: String::new(),
            })
        })
        .collect())
}

fn dig<'a>(value: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    path.split('.').try_fold(value, |v, key| v.get(key))
}

fn value_to_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexers::JsonPaths;

    #[test]
    fn magnet_list_dedupes_and_extracts_dn_as_title() {
        let body = r#"
            <a href="magnet:?xt=urn:btih:AAA&dn=Sita+Sings+the+Blues&tr=udp://tracker">dl</a>
            <a href="magnet:?xt=urn:btih:AAA&dn=Sita+Sings+the+Blues&tr=udp://tracker">dup</a>
            <a href="magnet:?xt=urn:btih:BBB">no title</a>
        "#;
        let results = magnet_list(body);
        assert_eq!(results.len(), 2, "debe deduplicar el magnet repetido");
        assert_eq!(results[0].title, "Sita Sings the Blues");
        assert_eq!(results[1].title, "(sin título)");
    }

    #[test]
    fn rss_parses_nyaa_style_feed_with_cdata_and_namespaced_tags() {
        let body = r#"<?xml version="1.0"?>
<rss><channel>
<item>
<title><![CDATA[Sintel (2010) 1080p]]></title>
<nyaa:magnetUri><![CDATA[magnet:?xt=urn:btih:CCC&dn=Sintel]]></nyaa:magnetUri>
<nyaa:seeders>42</nyaa:seeders>
<nyaa:size>1.4 GiB</nyaa:size>
</item>
<item>
<title>Tears of Steel</title>
<link>magnet:?xt=urn:btih:DDD&dn=Tears</link>
</item>
</channel></rss>"#;
        let results = rss(body);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Sintel (2010) 1080p");
        assert_eq!(results[0].magnet, "magnet:?xt=urn:btih:CCC&dn=Sintel");
        assert_eq!(results[0].seeders.as_deref(), Some("42"));
        assert_eq!(results[0].size.as_deref(), Some("1.4 GiB"));
        assert_eq!(results[1].title, "Tears of Steel");
        assert_eq!(results[1].seeders, None);
    }

    #[test]
    fn rss_skips_items_without_a_magnet() {
        let body = "<item><title>Sin magnet</title></item>";
        assert!(rss(body).is_empty());
    }

    #[test]
    fn rss_builds_magnet_from_infohash_when_no_literal_magnet_present() {
        // Estructura real del feed RSS de nyaa.si a 2026-08 (verificado
        // contra la API en vivo): no trae `<nyaa:magnetUri>`, solo
        // `<nyaa:infoHash>` + link al .torrent.
        let body = r#"<item>
            <title>[HatSubs] One Piece 1172 (WEB 1080p) [451AF1F0].mkv</title>
            <link>https://nyaa.si/download/2141524.torrent</link>
            <nyaa:seeders>454</nyaa:seeders>
            <nyaa:size>1.3 GiB</nyaa:size>
            <nyaa:infoHash>8f4d28a5c8dbf9993a57aa62e7492681cbef8849</nyaa:infoHash>
        </item>"#;
        let results = rss(body);
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].magnet,
            "magnet:?xt=urn:btih:8f4d28a5c8dbf9993a57aa62e7492681cbef8849&dn=%5BHatSubs%5D%20One%20Piece%201172%20%28WEB%201080p%29%20%5B451AF1F0%5D.mkv"
        );
        assert_eq!(results[0].seeders.as_deref(), Some("454"));
    }

    #[test]
    fn json_navigates_items_path_and_fields() {
        let body = r#"{
            "data": {
                "results": [
                    {"name": "Cosmos Laundromat", "link": "magnet:?xt=urn:btih:EEE", "peers": 7, "filesize": "800 MB"},
                    {"name": "Sin magnet"}
                ]
            }
        }"#;
        let paths = JsonPaths {
            items_path: "data.results".to_string(),
            title_field: "name".to_string(),
            magnet_field: "link".to_string(),
            size_field: Some("filesize".to_string()),
            seeders_field: Some("peers".to_string()),
        };
        let results = json(body, Some(&paths)).unwrap();
        assert_eq!(results.len(), 1, "el segundo item sin magnet_field se descarta");
        assert_eq!(results[0].title, "Cosmos Laundromat");
        assert_eq!(results[0].magnet, "magnet:?xt=urn:btih:EEE");
        assert_eq!(results[0].seeders.as_deref(), Some("7"));
        assert_eq!(results[0].size.as_deref(), Some("800 MB"));
    }

    #[test]
    fn json_without_paths_is_an_error() {
        assert!(json("{}", None).is_err());
    }

    #[test]
    fn json_root_as_array_when_items_path_is_empty() {
        let body = r#"[{"t": "X", "m": "magnet:?xt=urn:btih:FFF"}]"#;
        let paths = JsonPaths {
            items_path: String::new(),
            title_field: "t".to_string(),
            magnet_field: "m".to_string(),
            size_field: None,
            seeders_field: None,
        };
        let results = json(body, Some(&paths)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].magnet, "magnet:?xt=urn:btih:FFF");
    }
}
