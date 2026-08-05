mod ai;
mod commands;
mod db;
mod engine;
mod http_retry;
mod indexers;
mod iptv;
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
            // RecorderState arranca vacío en cada proceso nuevo — cualquier
            // fila 'recording' de una corrida anterior quedó huérfana.
            iptv::recorder::reconcile_interrupted_recordings(&conn)?;
            app.manage(db::Db(Mutex::new(conn)));
            app.manage(iptv::recorder::RecorderState::default());

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
            commands::get_stream_url_for_media_item,
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
            iptv::list_iptv_sources,
            iptv::add_iptv_source_url,
            iptv::add_iptv_source_file,
            iptv::remove_iptv_source,
            iptv::toggle_iptv_source,
            iptv::list_channels,
            iptv::validate_channel_manifest,
            iptv::recorder::start_recording,
            iptv::recorder::stop_recording,
            iptv::recorder::list_recordings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
