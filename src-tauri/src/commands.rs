use std::sync::Arc;

use tauri::State;

use crate::db::Db;
use crate::engine::{AddTorrentSource, TorrentEngine, TorrentInfo};
use crate::sources::archive_org::{self, ArchiveOrgItem};

pub struct EngineState(pub Arc<dyn TorrentEngine>);
pub struct HttpClient(pub reqwest::Client);

#[tauri::command]
pub async fn add_torrent(
    engine: State<'_, EngineState>,
    magnet: String,
) -> Result<TorrentInfo, String> {
    engine
        .0
        .add(AddTorrentSource::Magnet(magnet))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_torrents(engine: State<'_, EngineState>) -> Result<Vec<TorrentInfo>, String> {
    engine.0.list().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pause_torrent(engine: State<'_, EngineState>, id: String) -> Result<(), String> {
    engine.0.pause(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn remove_torrent(
    engine: State<'_, EngineState>,
    id: String,
    delete_files: bool,
) -> Result<(), String> {
    engine
        .0
        .remove(&id, delete_files)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_stream_url(
    engine: State<'_, EngineState>,
    id: String,
    file_idx: usize,
) -> Result<String, String> {
    engine
        .0
        .stream_url(&id, file_idx)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_archive_org(
    http: State<'_, HttpClient>,
    query: String,
) -> Result<Vec<ArchiveOrgItem>, String> {
    archive_org::search(&http.0, &query)
        .await
        .map_err(|e| e.to_string())
}

/// Descarga el .torrent público del ítem, lo entrega al TorrentEngine activo
/// y registra el ítem en media_items (source_type = 'archive_org') para que
/// quede en la biblioteca local — no solo en la lista de torrents del motor.
#[tauri::command]
pub async fn add_archive_org_item(
    http: State<'_, HttpClient>,
    engine: State<'_, EngineState>,
    db: State<'_, Db>,
    identifier: String,
    title: String,
    year: Option<i64>,
    licenseurl: Option<String>,
) -> Result<TorrentInfo, String> {
    let bytes = archive_org::fetch_torrent_bytes(&http.0, &identifier)
        .await
        .map_err(|e| e.to_string())?;

    let info = engine
        .0
        .add(AddTorrentSource::TorrentBytes(bytes))
        .await
        .map_err(|e| e.to_string())?;

    let media_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO media_items \
             (id, source_type, source_identifier, title, year, license, engine_torrent_id) \
             VALUES (?1, 'archive_org', ?2, ?3, ?4, ?5, ?6)",
            (&media_id, &identifier, &title, year, licenseurl, &info.id),
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(info)
}
