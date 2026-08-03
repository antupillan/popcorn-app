use anyhow::Context;
use serde::{Deserialize, Serialize};

const SEARCH_URL: &str = "https://archive.org/advancedsearch.php";

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ArchiveOrgItem {
    pub identifier: String,
    pub title: String,
    pub year: Option<i64>,
    pub licenseurl: Option<String>,
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
pub async fn search(client: &reqwest::Client, query: &str) -> anyhow::Result<Vec<ArchiveOrgItem>> {
    let resp: SearchResponse = client
        .get(SEARCH_URL)
        .query(&[
            ("q", query),
            ("fl[]", "identifier"),
            ("fl[]", "title"),
            ("fl[]", "year"),
            ("fl[]", "licenseurl"),
            ("rows", "50"),
            ("output", "json"),
        ])
        .send()
        .await
        .context("no se pudo contactar archive.org")?
        .json()
        .await
        .context("respuesta de archive.org con formato inesperado")?;
    Ok(resp.response.docs)
}

/// Descarga el .torrent público del ítem. archive.org redirige (302) al
/// datanode real que lo sirve — reqwest sigue redirects por defecto.
pub async fn fetch_torrent_bytes(
    client: &reqwest::Client,
    identifier: &str,
) -> anyhow::Result<Vec<u8>> {
    let url = format!("https://archive.org/download/{identifier}/{identifier}_archive.torrent");
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("no se pudo descargar {url}"))?
        .error_for_status()
        .with_context(|| format!("{url} respondió con error"))?;
    Ok(resp.bytes().await?.to_vec())
}
