use anyhow::Context;
use serde::{Deserialize, Serialize};

const SEARCH_URL: &str = "https://archive.org/advancedsearch.php";

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ArchiveOrgItem {
    pub identifier: String,
    pub title: String,
    pub year: Option<i64>,
    pub licenseurl: Option<String>,
    /// No viene en la respuesta de advancedsearch.php — se deriva del
    /// `identifier` después de deserializar (ver `search`).
    #[serde(default)]
    pub thumbnail_url: String,
}

#[derive(Deserialize)]
struct SearchResponse {
    response: SearchResponseBody,
}

#[derive(Deserialize)]
struct SearchResponseBody {
    docs: Vec<ArchiveOrgItem>,
}

/// Only fuentes P2P nativas para v1 — archive.org distribuye vía .torrent
/// real (verificado contra la API en vivo), no scraping ni mirrors propios.
///
/// `mediatype_filter` (ej. "movies") restringe del lado del servidor de
/// archive.org, que indexa de todo (libros, audio, software) además de
/// video — sin esto, una búsqueda de película trae ruido no relacionado.
/// Viene de `source_settings.mediatype_filter`, nunca hardcodeado acá; si
/// es `None` no se aplica ningún filtro (fail-open, ver plan).
pub async fn search(
    client: &reqwest::Client,
    query: &str,
    mediatype_filter: Option<&str>,
) -> anyhow::Result<Vec<ArchiveOrgItem>> {
    let q = match mediatype_filter {
        Some(mt) if !mt.is_empty() => format!("({query}) AND mediatype:({mt})"),
        _ => query.to_string(),
    };
    let resp: SearchResponse = crate::http_retry::send_with_retry(|| {
        client.get(SEARCH_URL).query(&[
            ("q", q.as_str()),
            ("fl[]", "identifier"),
            ("fl[]", "title"),
            ("fl[]", "year"),
            ("fl[]", "licenseurl"),
            ("rows", "50"),
            ("output", "json"),
        ])
    })
    .await
    .context("no se pudo contactar archive.org")?
    .json()
    .await
    .context("respuesta de archive.org con formato inesperado")?;
    let mut docs = resp.response.docs;
    for item in &mut docs {
        item.thumbnail_url = format!("https://archive.org/services/img/{}", item.identifier);
    }
    Ok(docs)
}

#[derive(Deserialize)]
struct MetadataResponse {
    files: Vec<MetadataFile>,
}

#[derive(Deserialize)]
struct MetadataFile {
    name: String,
    format: Option<String>,
}

const PLAYABLE_FORMATS: &[&str] = &["h.264", "512kb mp4", "mpeg4"];

/// Fallback HTTP directo (ver TorrentEngine::register_http_fallback): la
/// mayoría de los .torrent de archive.org dependen de webseeds BEP19 que
/// librqbit no soporta (upstream: ikatson/rqbit#500), así que se resuelve
/// el archivo reproducible real vía la API de metadata pública y se
/// construye su URL de descarga directa, que sí soporta Range de forma
/// nativa (verificado contra la API en vivo).
pub async fn primary_video_file(
    client: &reqwest::Client,
    identifier: &str,
) -> anyhow::Result<String> {
    let url = format!("https://archive.org/metadata/{identifier}");
    let meta: MetadataResponse = crate::http_retry::send_with_retry(|| client.get(&url))
        .await
        .with_context(|| format!("no se pudo consultar {url}"))?
        .json()
        .await
        .context("metadata de archive.org con formato inesperado")?;

    let file = meta
        .files
        .iter()
        .find(|f| {
            f.format
                .as_deref()
                .map(|fmt| PLAYABLE_FORMATS.contains(&fmt.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .context("no se encontró un archivo de video reproducible en el ítem")?;

    Ok(format!(
        "https://archive.org/download/{identifier}/{}",
        file.name
    ))
}

/// Descarga el .torrent público del ítem. archive.org redirige (302) al
/// datanode real que lo sirve — reqwest sigue redirects por defecto.
pub async fn fetch_torrent_bytes(
    client: &reqwest::Client,
    identifier: &str,
) -> anyhow::Result<Vec<u8>> {
    let url = format!("https://archive.org/download/{identifier}/{identifier}_archive.torrent");
    let resp = crate::http_retry::send_with_retry(|| client.get(&url))
        .await
        .with_context(|| format!("no se pudo descargar {url}"))?
        .error_for_status()
        .with_context(|| format!("{url} respondió con error"))?;
    Ok(resp.bytes().await?.to_vec())
}
