use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::db::Db;
use crate::engine::{AddTorrentSource, TorrentEngine, TorrentInfo};
use crate::sources::archive_org::{self, ArchiveOrgItem};

pub struct EngineState(pub Arc<dyn TorrentEngine>);
pub struct HttpClient(pub reqwest::Client);

#[derive(Serialize)]
pub struct MediaItem {
    pub id: String,
    pub source_type: String,
    pub source_identifier: String,
    pub title: String,
    pub year: Option<i64>,
    pub license: Option<String>,
    pub engine_torrent_id: Option<String>,
    pub is_private: bool,
    pub added_at: String,
}

/// Biblioteca local (media_items) — distinta de list_torrents: esto es lo
/// que el usuario ve como "su colección", el motor puede tener entradas
/// transitorias que nunca llegan a ser un media_item (ver Fase 1 del plan).
#[tauri::command]
pub async fn list_media_items(db: State<'_, Db>) -> Result<Vec<MediaItem>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, source_type, source_identifier, title, year, license, \
             engine_torrent_id, is_private, added_at FROM media_items ORDER BY added_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(MediaItem {
                id: r.get(0)?,
                source_type: r.get(1)?,
                source_identifier: r.get(2)?,
                title: r.get(3)?,
                year: r.get(4)?,
                license: r.get(5)?,
                engine_torrent_id: r.get(6)?,
                is_private: r.get::<_, i64>(7)? != 0,
                added_at: r.get(8)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Agrega un torrent a partir de bytes de archivo .torrent subidos por el
/// usuario (tab "Subir .torrent" del modal unificado) — no crea fila en
/// media_items porque no hay metadata de catálogo asociada, a diferencia
/// de add_archive_org_item.
#[tauri::command]
pub async fn add_torrent_file(
    engine: State<'_, EngineState>,
    bytes: Vec<u8>,
) -> Result<TorrentInfo, String> {
    engine
        .0
        .add(AddTorrentSource::TorrentBytes(bytes))
        .await
        .map_err(|e| e.to_string())
}

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
    db: State<'_, Db>,
    query: String,
) -> Result<Vec<ArchiveOrgItem>, String> {
    let mediatype_filter: Option<String> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT mediatype_filter FROM source_settings WHERE id = 'archive_org'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?
    };
    archive_org::search(&http.0, &query, mediatype_filter.as_deref())
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

    // Fallback HTTP: la mayoría de los .torrent de archive.org dependen de
    // webseeds (BEP19) que librqbit no soporta (ver Tarea 9 de verificación
    // E2E). Si no se puede resolver un archivo reproducible, no es fatal —
    // el torrent sigue agregado y puede eventualmente completar por P2P.
    if let Ok(url) = archive_org::primary_video_file(&http.0, &identifier).await {
        engine
            .0
            .register_http_fallback(&info.id, url)
            .await
            .map_err(|e| e.to_string())?;
    }

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
