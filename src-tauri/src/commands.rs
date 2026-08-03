use std::sync::Arc;

use tauri::State;

use crate::engine::{AddTorrentSource, TorrentEngine, TorrentInfo};

pub struct EngineState(pub Arc<dyn TorrentEngine>);

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
