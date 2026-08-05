mod hls_playlist;
mod parse;
pub mod recorder;

use anyhow::Context;
use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::{Manager, State};

use crate::ai::{commands::try_build_active_provider, curation};
use crate::db::Db;

#[derive(Serialize, Clone)]
pub struct IptvSource {
    pub id: String,
    pub name: String,
    pub source_kind: String, // "url" | "file"
    pub playlist_url: Option<String>,
    pub enabled: bool,
}

#[derive(Serialize, Clone)]
pub struct Channel {
    pub name: String,
    pub url: String,
    pub group: Option<String>,
    pub logo_url: Option<String>,
    pub tvg_id: Option<String>,
    pub source_id: String,
}

/// app_data_dir/iptv/playlists/{id}.m3u — mismo criterio que
/// `EmbeddedRqbit` usa un directorio para sus descargas, no un blob de DB
/// (ver migración iptv_sources).
fn playlist_file_path(app: &tauri::AppHandle, id: &str) -> anyhow::Result<std::path::PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("no se pudo resolver el directorio de datos de la app")?
        .join("iptv")
        .join("playlists");
    std::fs::create_dir_all(&dir).with_context(|| format!("no se pudo crear {}", dir.display()))?;
    Ok(dir.join(format!("{id}.m3u")))
}

fn row_to_source(row: &rusqlite::Row) -> rusqlite::Result<IptvSource> {
    Ok(IptvSource {
        id: row.get(0)?,
        name: row.get(1)?,
        source_kind: row.get(2)?,
        playlist_url: row.get(3)?,
        enabled: row.get::<_, i64>(4)? != 0,
    })
}

#[tauri::command]
pub async fn list_iptv_sources(db: State<'_, Db>) -> Result<Vec<IptvSource>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, source_kind, playlist_url, enabled \
             FROM iptv_sources ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_source).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_iptv_source_url(
    db: State<'_, Db>,
    name: String,
    playlist_url: String,
) -> Result<IptvSource, String> {
    let id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO iptv_sources (id, name, source_kind, playlist_url) VALUES (?1, ?2, 'url', ?3)",
            (&id, &name, &playlist_url),
        )
        .map_err(|e| e.to_string())?;
        // Toda fuente necesita su fila de curación, mismo contrato que
        // add_indexer (ver source_settings).
        conn.execute("INSERT INTO source_settings (id) VALUES (?1)", [&id])
            .map_err(|e| e.to_string())?;
    }
    Ok(IptvSource {
        id,
        name,
        source_kind: "url".to_string(),
        playlist_url: Some(playlist_url),
        enabled: true,
    })
}

#[tauri::command]
pub async fn add_iptv_source_file(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    name: String,
    bytes: Vec<u8>,
) -> Result<IptvSource, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let path = playlist_file_path(&app, &id).map_err(|e| e.to_string())?;
    std::fs::write(&path, &bytes)
        .with_context(|| format!("no se pudo escribir {}", path.display()))
        .map_err(|e| e.to_string())?;
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO iptv_sources (id, name, source_kind, playlist_url) VALUES (?1, ?2, 'file', NULL)",
            (&id, &name),
        )
        .map_err(|e| e.to_string())?;
        conn.execute("INSERT INTO source_settings (id) VALUES (?1)", [&id])
            .map_err(|e| e.to_string())?;
    }
    Ok(IptvSource {
        id,
        name,
        source_kind: "file".to_string(),
        playlist_url: None,
        enabled: true,
    })
}

#[tauri::command]
pub async fn remove_iptv_source(
    app: tauri::AppHandle,
    db: State<'_, Db>,
    id: String,
) -> Result<(), String> {
    let source_kind: Option<String> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row("SELECT source_kind FROM iptv_sources WHERE id = ?1", [&id], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
    };
    // Los indexers no tienen nada en disco que limpiar; una fuente IPTV de
    // tipo 'file' sí — no fatal si el archivo ya no está (Mandato de
    // Diagnóstico Íntegro: no bloquear el borrado en DB por un archivo que
    // igual ya no importa).
    if source_kind.as_deref() == Some("file") {
        if let Ok(path) = playlist_file_path(&app, &id) {
            let _ = std::fs::remove_file(&path);
        }
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM iptv_sources WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    // source_settings no tiene FK real a iptv_sources (ver migración) — el
    // borrado en cascada se hace a mano acá, mismo patrón que remove_indexer.
    conn.execute("DELETE FROM source_settings WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn toggle_iptv_source(db: State<'_, Db>, id: String, enabled: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE iptv_sources SET enabled = ?1 WHERE id = ?2",
        (enabled as i64, &id),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn fetch_channels_from_url(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<Channel>> {
    let body = crate::http_retry::send_with_retry(|| client.get(url))
        .await
        .with_context(|| format!("no se pudo contactar {url}"))?
        .text()
        .await?;
    Ok(parse::channel_list(&body))
}

fn fetch_channels_from_file(path: &std::path::Path) -> anyhow::Result<Vec<Channel>> {
    let body = std::fs::read_to_string(path)
        .with_context(|| format!("no se pudo leer {}", path.display()))?;
    Ok(parse::channel_list(&body))
}

async fn fetch_channels_for_source(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    source: &IptvSource,
) -> anyhow::Result<Vec<Channel>> {
    let mut channels = match source.source_kind.as_str() {
        "url" => {
            let url = source
                .playlist_url
                .as_deref()
                .context("fuente 'url' sin playlist_url")?;
            fetch_channels_from_url(client, url).await?
        }
        "file" => fetch_channels_from_file(&playlist_file_path(app, &source.id)?)?,
        other => anyhow::bail!("source_kind de fuente IPTV desconocido: {other}"),
    };
    for c in &mut channels {
        c.source_id = source.id.clone();
    }
    Ok(channels)
}

/// Config de curación por fuente — misma tabla `source_settings` que ya
/// usan archive.org e indexers, ver `sources::settings`.
fn curation_enabled_for(db: &Db, source_id: &str) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let enabled: i64 = conn
        .query_row(
            "SELECT curation_enabled FROM source_settings WHERE id = ?1",
            [source_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(enabled != 0)
}

fn curation_hint_for(db: &Db, source_id: &str) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT curation_hint FROM source_settings WHERE id = ?1",
        [source_id],
        |r| r.get::<_, Option<String>>(0),
    )
    .map_err(|e| e.to_string())
}

/// Despacha contra todas las fuentes IPTV habilitadas y fusiona canales.
/// Ningún canal se persiste — se refetchea/reparsea en cada llamada, mismo
/// criterio que `search_indexers` con los indexers. Una fuente que falla no
/// tira abajo a las demás. Curación por IA (fail-open si no hay proveedor
/// activo) se aplica por fuente, con su propio `curation_hint`, antes de
/// fusionar — no sobre la lista ya mezclada.
#[tauri::command]
pub async fn list_channels(
    app: tauri::AppHandle,
    http: State<'_, crate::commands::HttpClient>,
    db: State<'_, Db>,
) -> Result<Vec<Channel>, String> {
    let enabled: Vec<IptvSource> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, source_kind, playlist_url, enabled \
                 FROM iptv_sources WHERE enabled = 1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], row_to_source)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    let provider = try_build_active_provider(&db)?;

    let mut all = Vec::new();
    for source in &enabled {
        match fetch_channels_for_source(&app, &http.0, source).await {
            Ok(mut channels) => {
                if let Some(provider) = &provider {
                    if curation_enabled_for(&db, &source.id)? {
                        let hint = curation_hint_for(&db, &source.id)?;
                        channels = curation::curate_channels(
                            provider.as_ref(),
                            channels,
                            |c| c.name.as_str(),
                            hint.as_deref(),
                        )
                        .await;
                    }
                }
                all.append(&mut channels);
            }
            Err(e) => eprintln!("[popcorn] fuente IPTV '{}' falló: {e}", source.name),
        }
    }
    Ok(all)
}

/// "Supervisión liviana" de playback (ver plan): valida/reintenta el
/// manifest inicial contra el origen, nunca proxea segmentos — esos los pide
/// `hls.js` directo. Devuelve la URL final (post-redirect) para que el
/// frontend apunte ahí.
#[tauri::command]
pub async fn validate_channel_manifest(
    http: State<'_, crate::commands::HttpClient>,
    url: String,
) -> Result<String, String> {
    let resp = crate::http_retry::send_with_retry(|| http.0.get(&url))
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("el canal respondió con estado {}", resp.status()));
    }
    let final_url = resp.url().to_string();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !body.trim_start().starts_with("#EXTM3U") {
        return Err("la URL no devolvió un manifest HLS válido (no empieza con #EXTM3U)".to_string());
    }
    Ok(final_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_channels_from_file_reads_and_parses_local_playlist() {
        let dir = std::env::temp_dir().join(format!("popcorn-iptv-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.m3u");
        std::fs::write(&path, "#EXTM3U\n#EXTINF:-1,Canal Local\nhttps://example.org/local.m3u8\n").unwrap();

        let channels = fetch_channels_from_file(&path).unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Canal Local");
        assert_eq!(channels[0].source_id, "", "source_id lo estampa el llamador, no el parser");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn fetch_channels_from_file_errors_honestly_when_missing() {
        let path = std::env::temp_dir().join(format!("popcorn-iptv-missing-{}.m3u", uuid::Uuid::new_v4()));
        assert!(fetch_channels_from_file(&path).is_err());
    }

    #[tokio::test]
    #[ignore = "red real, no apto para CI por defecto — correr manualmente para verificar contra la fuente semilla real"]
    async fn fetch_channels_from_url_against_real_iptv_org_public_category() {
        let client = reqwest::Client::new();
        let channels = fetch_channels_from_url(
            &client,
            "https://iptv-org.github.io/iptv/categories/public.m3u",
        )
        .await
        .expect("la fuente semilla real debe responder con una playlist parseable");

        assert!(!channels.is_empty(), "la categoría 'Public' de iptv-org no debería estar vacía");
        for c in &channels {
            assert!(!c.name.is_empty());
            assert!(c.url.starts_with("http"), "url de canal inválida: {}", c.url);
        }
    }
}
