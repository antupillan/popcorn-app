mod ai;
mod commands;
mod db;
mod engine;
mod indexers;
mod sources;

use std::sync::{Arc, Mutex};
use tauri::Manager;

use commands::{EngineState, HttpClient};
use engine::embedded_rqbit::EmbeddedRqbit;
use engine::TorrentEngine;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let conn = db::open(app.handle())?;
            app.manage(db::Db(Mutex::new(conn)));

            let downloads_dir = app.path().app_data_dir()?.join("downloads");
            std::fs::create_dir_all(&downloads_dir)?;
            let embedded = tauri::async_runtime::block_on(EmbeddedRqbit::new(downloads_dir))?;
            app.manage(EngineState(Arc::new(embedded) as Arc<dyn TorrentEngine>));
            app.manage(HttpClient(reqwest::Client::new()));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::add_torrent,
            commands::list_torrents,
            commands::pause_torrent,
            commands::remove_torrent,
            commands::get_stream_url,
            commands::search_archive_org,
            commands::add_archive_org_item,
            commands::list_media_items,
            commands::add_torrent_file,
            indexers::list_indexers,
            indexers::add_indexer,
            indexers::remove_indexer,
            indexers::toggle_indexer,
            indexers::test_indexer,
            indexers::search_indexers,
            ai::commands::list_ai_providers,
            ai::commands::add_ai_provider,
            ai::commands::remove_ai_provider,
            ai::commands::set_active_ai_provider,
            ai::commands::parse_query,
            sources::settings::list_source_settings,
            sources::settings::set_source_curation_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
