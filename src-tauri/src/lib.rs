mod ai;
mod app_settings;
mod availability_ping;
mod commands;
mod db;
mod engine;
mod http_retry;
mod indexers;
mod iptv;
mod keychain;
mod local_library;
mod online_library;
mod opensubtitles;
mod os_accent;
mod sources;
mod subtitles;
mod youtube;

use std::sync::{Arc, Mutex};
use tauri::Manager;

use commands::{EngineFallbackWarning, EngineState, HttpClient};
use engine::embedded_rqbit::EmbeddedRqbit;
use engine::external_qbittorrent::ExternalQbittorrent;
use engine::TorrentEngine;

/// Arma `EmbeddedRqbit` completo: sesión de librqbit + servidor de
/// streaming compartido levantado con esa sesión (`Some(session)`, para que
/// `/stream/*` funcione). Separado del `.setup()` porque se necesita tanto
/// en el camino normal (motor embebido por elección) como en el fallback
/// (motor externo configurado que no pudo conectar).
async fn build_embedded(
    downloads_dir: std::path::PathBuf,
    http_fallback: engine::stream_server::HttpFallbackMap,
    local_files: engine::stream_server::LocalFileMap,
    socks_proxy_url: Option<String>,
) -> anyhow::Result<EmbeddedRqbit> {
    let session =
        engine::embedded_rqbit::create_session_with_proxy(downloads_dir.clone(), socks_proxy_url).await?;
    let stream_port =
        engine::stream_server::spawn(Some(session.clone()), http_fallback.clone(), local_files.clone()).await?;
    Ok(EmbeddedRqbit::new(session, downloads_dir, stream_port, http_fallback, local_files))
}

/// Decide y construye el motor activo según `active_torrent_engine`
/// (`app_settings`). Si el motor configurado es `qbittorrent` pero no
/// conecta (daemon apagado, credenciales malas, host inalcanzable), cae a
/// `EmbeddedRqbit` igual — la app siempre abre — y devuelve el motivo del
/// fallback para que el frontend lo muestre (nunca silencioso, Mandato 1).
async fn build_active_engine(
    db: &db::Db,
    downloads_dir: std::path::PathBuf,
    http_fallback: engine::stream_server::HttpFallbackMap,
    local_files: engine::stream_server::LocalFileMap,
) -> anyhow::Result<(Arc<dyn TorrentEngine>, Option<String>)> {
    let kind = app_settings::read_active_engine_kind(db).unwrap_or_else(|_| "embedded".to_string());
    let socks_proxy_url = keychain::get_secret(app_settings::TORRENT_PROXY_SECRET_ID).unwrap_or(None);

    if kind != "qbittorrent" {
        let embedded = build_embedded(downloads_dir, http_fallback, local_files, socks_proxy_url).await?;
        return Ok((Arc::new(embedded), None));
    }

    let connection = app_settings::read_qbittorrent_connection(db);
    let attempt = match connection {
        Ok((base_url, username, password)) => {
            // session=None: el motor externo no expone /stream/* (P2P en
            // vivo), solo /local/* vía file_path() — ver engine/mod.rs.
            match engine::stream_server::spawn(None, http_fallback.clone(), local_files.clone()).await {
                Ok(stream_port) => {
                    ExternalQbittorrent::new(&base_url, &username, &password, stream_port, local_files.clone())
                        .await
                        .map_err(|e| e.to_string())
                }
                Err(e) => Err(e.to_string()),
            }
        }
        Err(e) => Err(e),
    };

    match attempt {
        Ok(qb) => Ok((Arc::new(qb), None)),
        Err(reason) => {
            eprintln!("[popcorn] no se pudo conectar al qBittorrent configurado: {reason}");
            let embedded = build_embedded(downloads_dir, http_fallback, local_files, socks_proxy_url).await?;
            Ok((
                Arc::new(embedded),
                Some(format!(
                    "No se pudo conectar al qBittorrent configurado ({reason}) — se usó el motor embebido en su lugar."
                )),
            ))
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let conn = db::open(app.handle())?;
            // RecorderState arranca vacío en cada proceso nuevo — cualquier
            // fila 'recording' de una corrida anterior quedó huérfana.
            iptv::recorder::reconcile_interrupted_recordings(&conn)?;
            app.manage(db::Db(Mutex::new(conn)));
            app.manage(iptv::recorder::RecorderState::default());

            let downloads_dir = app.path().app_data_dir()?.join("downloads");
            std::fs::create_dir_all(&downloads_dir)?;

            // El servidor de streaming es compartido por cualquier motor activo
            // (ver engine::stream_server) — se levanta una sola vez, no adentro
            // del constructor de cada motor. El motor concreto (embebido o
            // ExternalQbittorrent) se decide leyendo la config del usuario.
            let http_fallback: engine::stream_server::HttpFallbackMap = Arc::new(Mutex::new(Default::default()));
            let local_files: engine::stream_server::LocalFileMap = Arc::new(Mutex::new(Default::default()));
            let db_state = app.state::<db::Db>();
            let (active_engine, fallback_warning) = tauri::async_runtime::block_on(build_active_engine(
                &db_state,
                downloads_dir,
                http_fallback,
                local_files,
            ))?;

            app.manage(EngineState(active_engine));
            app.manage(EngineFallbackWarning(fallback_warning));
            app.manage(HttpClient(commands::build_http_client()));
            app.manage(indexers::torrent_health::TorrentHealthState::default());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_os,
            commands::get_engine_fallback_warning,
            os_accent::get_os_accent_color,
            commands::add_torrent,
            commands::list_torrents,
            commands::pause_torrent,
            commands::remove_torrent,
            commands::get_stream_url,
            commands::get_stream_url_for_media_item,
            commands::search_archive_org,
            commands::add_archive_org_item,
            commands::seed_archive_org_item,
            commands::list_media_items,
            commands::remove_media_item,
            commands::add_torrent_file,
            indexers::list_indexers,
            indexers::add_indexer,
            indexers::remove_indexer,
            indexers::toggle_indexer,
            indexers::test_indexer,
            indexers::search_indexers,
            indexers::search_indexers_with_ai,
            indexers::torrent_health::check_torrent_health_batch,
            ai::commands::list_ai_providers,
            ai::commands::add_ai_provider,
            ai::commands::remove_ai_provider,
            ai::commands::set_active_ai_provider,
            ai::commands::parse_query,
            ai::commands::search_added_content_with_ai,
            sources::settings::list_source_settings,
            sources::settings::set_source_curation_enabled,
            online_library::browse_online_library,
            online_library::browse_public_domain_torrents,
            online_library::curate_online_library,
            online_library::add_online_item,
            app_settings::get_speed_limits,
            app_settings::set_speed_limits,
            app_settings::get_torrent_engine_config,
            app_settings::set_torrent_engine_config,
            app_settings::test_torrent_engine,
            app_settings::set_torrent_proxy_url,
            app_settings::get_torrent_proxy_status,
            app_settings::remove_torrent_proxy_url,
            commands::set_catalog_proxy_url,
            commands::get_catalog_proxy_status,
            commands::remove_catalog_proxy_url,
            local_library::get_local_library_folder,
            local_library::set_local_library_folder,
            local_library::list_local_files,
            local_library::get_local_stream_url,
            iptv::list_iptv_sources,
            iptv::add_iptv_source_url,
            iptv::add_iptv_source_file,
            iptv::remove_iptv_source,
            iptv::toggle_iptv_source,
            iptv::list_channels,
            iptv::curate_channels,
            iptv::validate_channel_manifest,
            iptv::recorder::start_recording,
            iptv::recorder::stop_recording,
            iptv::recorder::list_recordings,
            iptv::recorder::get_recording_stream_url,
            iptv::recorder::delete_recording,
            youtube::list_youtube_sources,
            youtube::add_youtube_source,
            youtube::remove_youtube_source,
            youtube::toggle_youtube_source,
            youtube::set_youtube_api_key,
            youtube::get_youtube_api_key_status,
            youtube::remove_youtube_api_key,
            youtube::list_youtube_videos,
            youtube::curate_youtube_videos,
            subtitles::list_subtitles,
            subtitles::add_subtitle_text,
            subtitles::remove_subtitle,
            subtitles::translate_subtitle_texts,
            opensubtitles::search_opensubtitles,
            opensubtitles::download_opensubtitles_subtitle,
            opensubtitles::set_opensubtitles_credentials,
            opensubtitles::get_opensubtitles_credentials_status,
            opensubtitles::remove_opensubtitles_credentials,
            youtube::search_youtube_videos_in_added_channels,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
