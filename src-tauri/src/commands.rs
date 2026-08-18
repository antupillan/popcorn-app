use std::sync::Arc;

use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::{Manager, State};

use crate::ai::{commands::try_build_active_provider, curation, StructuredQuery};
use crate::db::Db;
use crate::engine::{AddTorrentSource, TorrentEngine, TorrentInfo};
use crate::sources::archive_org::{self, ArchiveOrgItem};
use crate::sources::public_domain_torrents;

pub struct EngineState(pub Arc<dyn TorrentEngine>);
pub struct HttpClient(pub reqwest::Client);

const CATALOG_PROXY_SECRET_ID: &str = "catalog_http_proxy_url";

pub(crate) fn build_http_client() -> reqwest::Client {
    let proxy_url = crate::keychain::get_secret(CATALOG_PROXY_SECRET_ID).ok().flatten();
    let mut builder = reqwest::Client::builder();
    if let Some(url) = proxy_url {
        match reqwest::Proxy::all(&url) {
            Ok(proxy) => {
                let no_proxy = reqwest::NoProxy::from_string("localhost,127.0.0.1,::1");
                builder = builder.proxy(proxy.no_proxy(no_proxy));
            }
            Err(e) => eprintln!("[popcorn] catalog_http_proxy_url inválida ({e}), ignorada"),
        }
    }
    builder.build().unwrap_or_default()
}

#[tauri::command]
pub async fn set_catalog_proxy_url(url: String) -> Result<(), String> {
    crate::keychain::set_secret(CATALOG_PROXY_SECRET_ID, &url).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_catalog_proxy_status() -> Result<bool, String> {
    Ok(crate::keychain::get_secret(CATALOG_PROXY_SECRET_ID)
        .map_err(|e| e.to_string())?
        .is_some())
}

#[tauri::command]
pub async fn remove_catalog_proxy_url() -> Result<(), String> {
    crate::keychain::delete_secret(CATALOG_PROXY_SECRET_ID).map_err(|e| e.to_string())
}

/// `Some(mensaje)` cuando `lib.rs::setup` no pudo conectar al motor externo
/// configurado y cayó a `EmbeddedRqbit` — el frontend lo consulta una vez al
/// montar y lo muestra como aviso visible (nunca un fallback silencioso,
/// Mandato 1). `None` en el caso normal (motor embebido por elección, o
/// motor externo que sí conectó).
pub struct EngineFallbackWarning(pub Option<String>);

#[tauri::command]
pub async fn get_engine_fallback_warning(
    warning: State<'_, EngineFallbackWarning>,
) -> Result<Option<String>, String> {
    Ok(warning.0.clone())
}

/// Para el layout de TitleBar.tsx (semáforo macOS vs. controles a la
/// derecha en cualquier otro SO) — constante de compilación, sin plugin.
#[tauri::command]
pub fn get_os() -> &'static str {
    std::env::consts::OS
}

/// Alterna Mica/vibrancy en runtime (toggle de batería en Ajustes →
/// Ventana). El estado inicial en tauri.conf.json (windowEffects) solo
/// aplica al crear la ventana — set_effects es la única forma de
/// cambiarlo después. No-op documentado por Tauri en Linux.
#[tauri::command]
pub fn set_window_effects_enabled(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    let effects = enabled.then(|| tauri::utils::config::WindowEffectsConfig {
        effects: vec![tauri::utils::WindowEffect::Mica, tauri::utils::WindowEffect::Sidebar],
        ..Default::default()
    });
    window.set_effects(effects).map_err(|e| e.to_string())
}

/// Tope de subida del sembrado automático: 1 Mbps en bytes/seg (librqbit
/// opera en bytes, no bits). Fijo hasta que Etapa 2 lo vuelva ajustable.
pub(crate) const DEFAULT_SEED_UPLOAD_BPS: u32 = 1_000_000 / 8;

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

/// Borra un ítem de Mi Colección y su torrent/archivos asociados (mismo
/// `delete_files: true` que "Quitar" en la lista de Torrents). Si el motor
/// ya perdió la sesión del torrent (reinicio de la app, ver bug #26), el
/// error se ignora — el objetivo es que el ítem desaparezca de la
/// biblioteca igual, no bloquear el borrado por un torrent ya inexistente.
#[tauri::command]
pub async fn remove_media_item(
    engine: State<'_, EngineState>,
    db: State<'_, Db>,
    id: String,
) -> Result<(), String> {
    let engine_torrent_id: Option<String> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT engine_torrent_id FROM media_items WHERE id = ?1",
            [&id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .flatten()
    };
    if let Some(torrent_id) = engine_torrent_id {
        if let Err(e) = engine.0.remove(&torrent_id, true).await {
            eprintln!("[popcorn] no se pudo quitar el torrent {torrent_id} de {id}, se borra igual de la biblioteca: {e}");
        }
    }
    // Sin fallar si no hay .mp4 remuxeado cacheado para este ítem (no todo
    // ítem removido pasó por remux, ver resolve_remuxed_stream_url) — es
    // un archivo derivado, 100% reconstruible del original, nunca se
    // siembra ni se anuncia al swarm (ver plan).
    if let Some(downloads_dir) = engine.0.downloads_dir() {
        let cached = crate::engine::remux::cache_path(downloads_dir, &id);
        std::fs::remove_file(&cached).ok();
    }
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM media_items WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Agrega un torrent a partir de bytes de archivo .torrent subidos por el
/// usuario (tab "Subir .torrent" del modal unificado) — no crea fila en
/// media_items porque no hay metadata de catálogo asociada, a diferencia
/// de add_archive_org_item.
#[tauri::command]
pub async fn add_torrent_file(
    engine: State<'_, EngineState>,
    db: State<'_, Db>,
    bytes: Vec<u8>,
) -> Result<TorrentInfo, String> {
    let (download_bps, upload_bps) = crate::app_settings::read_speed_limits_bps(&db)?;
    engine
        .0
        .add_with_limits(AddTorrentSource::TorrentBytes(bytes), download_bps, upload_bps)
        .await
        .map_err(|e| e.to_string())
}

/// A diferencia de `add_torrent_file` (bytes originales no se guardan en
/// ningún lado, sanar tras perder la sesión del motor no es posible — ver
/// comentario explícito en `heal_media_item`), un magnet SÍ es su propio
/// identificador estable: `heal_media_item` ya sabe re-agregar
/// `source_type='magnet'` desde `source_identifier` (el magnet crudo), así
/// que registrar la fila acá es seguro y hace que el ítem sobreviva un
/// reinicio de la app, igual que archive.org.
pub(crate) async fn add_torrent_inner(
    engine: &Arc<dyn TorrentEngine>,
    db: &Db,
    magnet: String,
    download_bps: Option<u32>,
    upload_bps: Option<u32>,
) -> Result<TorrentInfo, String> {
    let info = engine
        .add_with_limits(AddTorrentSource::Magnet(magnet.clone()), download_bps, upload_bps)
        .await
        .map_err(|e| e.to_string())?;

    let media_id = uuid::Uuid::new_v4().to_string();
    let title = info.name.clone().unwrap_or_else(|| info.info_hash.clone());
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO media_items (id, source_type, source_identifier, title, engine_torrent_id) \
             VALUES (?1, 'magnet', ?2, ?3, ?4)",
            (&media_id, &magnet, &title, &info.id),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(info)
}

#[tauri::command]
pub async fn add_torrent(
    engine: State<'_, EngineState>,
    db: State<'_, Db>,
    magnet: String,
) -> Result<TorrentInfo, String> {
    let (download_bps, upload_bps) = crate::app_settings::read_speed_limits_bps(&db)?;
    add_torrent_inner(&engine.0, &db, magnet, download_bps, upload_bps).await
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

/// Resuelve la URL de streaming de un ítem de la biblioteca, sanando
/// `engine_torrent_id` si quedó apuntando a un torrent que ya no existe en
/// la sesión actual del motor (bug: `EmbeddedRqbit` no persiste su sesión
/// entre reinicios de la app — ver plan, bug #26). `media_items` es la
/// fuente de verdad: si el id no resuelve, se re-agrega desde ahí y se
/// actualiza la fila antes de devolver la URL.
#[tauri::command]
pub async fn get_stream_url_for_media_item(
    http: State<'_, HttpClient>,
    engine: State<'_, EngineState>,
    db: State<'_, Db>,
    media_id: String,
    file_idx: usize,
) -> Result<String, String> {
    resolve_media_item_stream_url(&http.0, &engine.0, &db, &media_id, file_idx).await
}

/// Lógica real de `get_stream_url_for_media_item`, separada del wrapper
/// `#[tauri::command]` para poder probarla con un `TorrentEngine` y una
/// conexión SQLite en memoria, sin necesitar una app Tauri corriendo (los
/// `State<'_, T>` no se pueden instanciar fuera de una — mismo patrón que
/// `try_build_active_provider` en `ai::commands`).
async fn resolve_media_item_stream_url(
    http: &reqwest::Client,
    engine: &Arc<dyn TorrentEngine>,
    db: &Db,
    media_id: &str,
    file_idx: usize,
) -> Result<String, String> {
    let (source_type, source_identifier, engine_torrent_id, stored_primary_file_idx): (
        String,
        String,
        Option<String>,
        Option<i64>,
    ) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT source_type, source_identifier, engine_torrent_id, primary_file_idx FROM media_items WHERE id = ?1",
            [media_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|e| e.to_string())?
    };

    let (resolved_id, healed_primary_file_idx) = match engine_torrent_id {
        Some(id) if engine.exists(&id).await => (id, None),
        _ => heal_media_item(http, engine, db, media_id, &source_type, &source_identifier).await?,
    };

    // Sobreescribe el file_idx que mande el frontend (siempre 0 hoy) cuando
    // ya sabemos cuál es el índice real del video — bug real reportado en
    // vivo: un torrent de archive.org trae, además del/los video(s),
    // transcripts/subtítulos/miniaturas/metadata como archivos propios, y
    // file_idx=0 podía caer en cualquiera de esos (visto en vivo: un
    // .asr.js de 135KB). `healed_primary_file_idx` (recién calculado si esta
    // llamada tuvo que sanar) tiene prioridad sobre `stored_primary_file_idx`
    // (leído de la DB ANTES de sanar, por lo tanto stale en la primera
    // sanación tras cada reinicio — bug real encontrado en vivo: sin esto,
    // la primera reproducción de cada sesión seguía usando file_idx=0 pese a
    // que `heal_media_item` ya había calculado y persistido el valor
    // correcto un instante antes, dentro de la misma llamada). Si ninguno
    // está disponible, sigue el file_idx recibido — no rompe lo que ya
    // andaba con file_idx=0 real.
    let file_idx = healed_primary_file_idx
        .or(stored_primary_file_idx.map(|i| i as usize))
        .unwrap_or(file_idx);

    // WebKitGTK no reproduce Matroska nativo vía <video src> (confirmado
    // en vivo: MEDIA_ERR_SRC_NOT_SUPPORTED pese a que el sistema decodifica
    // el mismo stream por otra vía — ver plan "Remux MKV -> MP4"). Si el
    // archivo real es .mkv, hay que remuxearlo a MP4 (sin recodificar)
    // antes de poder reproducirlo.
    if let Ok(path) = engine.file_path(&resolved_id, file_idx).await {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if crate::engine::remux::needs_remux(file_name) {
            return resolve_remuxed_stream_url(engine, media_id, &resolved_id, &path).await;
        }
    }

    engine
        .stream_url(&resolved_id, file_idx)
        .await
        .map_err(|e| e.to_string())
}

/// Sirve un ítem `.mkv` como MP4 remuxeado (cacheado por `media_id`, ver
/// `remux::cache_path`) — separado de `resolve_media_item_stream_url`
/// para no anidar más este camino, que ya es el caso menos común.
async fn resolve_remuxed_stream_url(
    engine: &Arc<dyn TorrentEngine>,
    media_id: &str,
    resolved_id: &str,
    original_path: &std::path::Path,
) -> Result<String, String> {
    let downloads_dir = engine
        .downloads_dir()
        .ok_or_else(|| "el motor activo no soporta remux (requiere EmbeddedRqbit)".to_string())?;
    let cached = crate::engine::remux::cache_path(downloads_dir, media_id);

    if !cached.exists() {
        // No se remuxea a medias: si el torrent todavía no terminó de
        // descargar, el archivo tiene huecos y el remux produciría un MP4
        // roto (Mandato 4, error honesto en vez de un intento silencioso).
        let finished = engine
            .list()
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|t| t.id == resolved_id)
            .map(|t| t.finished)
            .unwrap_or(false);
        if !finished {
            return Err(
                "este formato (MKV) necesita terminar de descargar antes de poder reproducirse".to_string(),
            );
        }
        crate::engine::remux::remux_mkv_to_mp4(original_path, &cached)
            .await
            .map_err(|e| e.to_string())?;
    }

    engine.local_stream_url(cached).await.map_err(|e| e.to_string())
}

/// Ventana máxima (intentos x intervalo = 2s) para dejar que la
/// verificación de piezas en background de `add_seeding_from_disk` confirme
/// `finished` antes de decidir si hace falta el fallback HTTP de archive.org
/// al sanar — ver comentario en `heal_media_item`. Puramente I/O local, no
/// depende de latencia de red externa.
const HEAL_FINISHED_RECHECK_ATTEMPTS: u32 = 10;
const HEAL_FINISHED_RECHECK_INTERVAL_MS: u64 = 200;

/// Re-agrega un ítem al motor a partir de su `source_type`/`source_identifier`
/// guardados en `media_items` y persiste el `engine_torrent_id` nuevo.
async fn heal_media_item(
    http: &reqwest::Client,
    engine: &Arc<dyn TorrentEngine>,
    db: &Db,
    media_id: &str,
    source_type: &str,
    source_identifier: &str,
) -> Result<(String, Option<usize>), String> {
    let mut primary_file_idx: Option<usize> = None;
    let new_id = match source_type {
        "archive_org" => {
            let bytes = archive_org::fetch_torrent_bytes_cached(http, source_identifier, engine.downloads_dir())
                .await
                .map_err(|e| e.to_string())?;
            primary_file_idx = archive_org::resolve_torrent_files(&bytes)
                .ok()
                .and_then(|files| archive_org::resolve_primary_file_idx(&files, source_identifier));
            // add_seeding_from_disk, no add() plano — mismo motivo que
            // add_archive_org_item_core: si ya se sembró antes, add() sin
            // overwrite falla contra los archivos ya completos en disco.
            let info = engine
                .add_seeding_from_disk(AddTorrentSource::TorrentBytes(bytes), None)
                .await
                .map_err(|e| e.to_string())?;
            // Revertido (bug real encontrado en vivo, más grave que el que
            // esto intentaba arreglar): sacar el registro del fallback acá
            // asumía "sanar implica que ya estaba completo antes" — falso
            // para ítems agregados por el flujo regular (add_archive_org_
            // item_core, sin sembrado automático dedicado), que dependen
            // POR COMPLETO del fallback HTTP para reproducirse — archive.org
            // casi nunca tiene peers P2P reales para contenido de dominio
            // público. Sin el fallback, esos ítems (la mayoría de Mi
            // Colección) quedaban sirviendo el archivo local a medio
            // descargar (verificado en vivo: 0 bytes reales en un .mp4
            // de 117MB) en vez del proxy que sí funcionaba. `stream_url`
            // (embedded_rqbit.rs) ya decide solo, por `finished`, cuándo
            // preferir local sobre el fallback — no hace falta forzarlo acá.
            //
            // Pero registrar el fallback significa red (primary_video_file
            // pega a archive.org) — innecesaria si el archivo YA está
            // completo en disco, caso real y común en Mi Colección (bug
            // reportado en vivo: reproducir un ítem 100% local y ya
            // sembrado seguía generando tráfico/errores contra archive.org
            // en cada sanación tras cada reinicio). `add_seeding_from_disk`
            // no verifica piezas de forma síncrona — corre en background
            // dentro de la sesión de librqbit, así que `info.finished` recién
            // devuelto puede ser `false` por un instante aunque el archivo
            // ya sea válido. Sondeo acotado, solo I/O local (nunca red): si
            // la verificación no cierra en esa ventana, se asume incompleto
            // y sí vale la pena el fallback.
            let mut finished = info.finished;
            for _ in 0..HEAL_FINISHED_RECHECK_ATTEMPTS {
                if finished {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(HEAL_FINISHED_RECHECK_INTERVAL_MS)).await;
                finished = engine
                    .list()
                    .await
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .any(|t| t.id == info.id && t.finished);
            }
            if !finished {
                if let Ok(url) = archive_org::primary_video_file(http, source_identifier).await {
                    engine
                        .register_http_fallback(&info.id, url)
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            info.id
        }
        "magnet" => {
            // add_seeding_from_disk (overwrite:true), no add() plano — mismo
            // motivo que la rama "archive_org": si el archivo ya se
            // descargó completo antes de perder la sesión del motor (ej.
            // reinicio de la app), add() sin overwrite falla con
            // "allow_overwrite = false" en vez de reengancharse al archivo
            // ya existente (bug real encontrado en vivo, no hipotético).
            let info = engine
                .add_seeding_from_disk(AddTorrentSource::Magnet(source_identifier.to_string()), None)
                .await
                .map_err(|e| e.to_string())?;
            info.id
        }
        "public_domain_torrents" => {
            // Sin fallback HTTP (a diferencia de archive_org): P2P puro,
            // limitación conocida (ver plan) — `source_identifier` ya es la
            // URL de descarga resuelta (ver `PublicDomainMovie::identifier`).
            let bytes = public_domain_torrents::fetch_torrent_bytes(http, source_identifier)
                .await
                .map_err(|e| e.to_string())?;
            let info = engine
                .add(AddTorrentSource::TorrentBytes(bytes))
                .await
                .map_err(|e| e.to_string())?;
            info.id
        }
        other => {
            // "torrent_file": los bytes originales del .torrent no se guardan
            // en ningún lado (ni DB ni disco) — no hay de dónde re-agregarlo.
            // Hoy ningún comando inserta media_items con este source_type
            // (ver add_torrent_file), así que esta rama es honestidad ante un
            // caso futuro, no un bug actual (Mandato 1: decir la limitación,
            // no fabricar una recuperación que no puede existir).
            return Err(format!(
                "el ítem de tipo '{other}' no se puede re-agregar automáticamente: \
                 no se guardaron los bytes originales del .torrent"
            ));
        }
    };

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    // COALESCE: si esta sanación no pudo resolver el índice (ej. .torrent
    // sin parsear), no pisa un valor ya bueno de una sanación anterior.
    conn.execute(
        "UPDATE media_items SET engine_torrent_id = ?1, primary_file_idx = COALESCE(?2, primary_file_idx) WHERE id = ?3",
        (&new_id, primary_file_idx.map(|i| i as i64), media_id),
    )
    .map_err(|e| e.to_string())?;

    Ok((new_id, primary_file_idx))
}

#[tauri::command]
pub async fn search_archive_org(
    http: State<'_, HttpClient>,
    db: State<'_, Db>,
    query: String,
) -> Result<Vec<ArchiveOrgItem>, String> {
    let (mediatype_filter, curation_enabled): (Option<String>, bool) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mediatype_filter: Option<String> = conn
            .query_row(
                "SELECT mediatype_filter FROM source_settings WHERE id = 'archive_org'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let curation_enabled: bool = conn
            .query_row(
                "SELECT curation_enabled FROM source_settings WHERE id = 'archive_org'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())?
            != 0;
        (mediatype_filter, curation_enabled)
    };
    let items = archive_org::search(&http.0, &query, mediatype_filter.as_deref(), None)
        .await
        .map_err(|e| e.to_string())?;
    if !curation_enabled {
        return Ok(items);
    }
    // Fail-open: sin proveedor de IA activo, la búsqueda sigue funcionando
    // con el filtro barato de mediatype ya aplicado (ver plan, decisión
    // "curación por proveedor de búsqueda").
    match try_build_active_provider(&db)? {
        Some(provider) => {
            let sq = StructuredQuery { title: query, ..Default::default() };
            Ok(curation::curate(provider.as_ref(), &sq, items, |i| i.title.as_str()).await)
        }
        None => Ok(items),
    }
}

/// Núcleo reusado por el comando `add_archive_org_item` y por
/// `online_library::add_online_item` para ítems "archive_org"/
/// "blender_foundation" (ambos son ítems de archive.org — Blender solo
/// difiere en de qué catálogo salió la búsqueda, ver
/// `sources::archive_org::blender_foundation_items` — y comparten
/// `source_type = 'archive_org'`, sin fila propia en el CHECK de
/// `media_items`). Descarga el .torrent público, lo entrega al TorrentEngine
/// activo y registra el ítem en media_items para que quede en la biblioteca
/// local, no solo en la lista de torrents del motor.
pub(crate) async fn add_archive_org_item_core(
    http: &reqwest::Client,
    engine: &Arc<dyn TorrentEngine>,
    db: &Db,
    identifier: &str,
    title: &str,
    year: Option<i64>,
    licenseurl: Option<String>,
) -> Result<TorrentInfo, String> {
    let bytes = archive_org::fetch_torrent_bytes_cached(http, identifier, engine.downloads_dir())
        .await
        .map_err(|e| e.to_string())?;

    // Calculado antes de mover `bytes` al motor — bug real reportado en
    // vivo: el reproductor asumía file_idx=0 como "el video", pero un
    // torrent de archive.org trae también transcripts/subtítulos/
    // miniaturas/metadata como archivos propios (ver
    // archive_org::resolve_primary_file_idx).
    let primary_file_idx = archive_org::resolve_torrent_files(&bytes)
        .ok()
        .and_then(|files| archive_org::resolve_primary_file_idx(&files, identifier));

    // add_seeding_from_disk (overwrite:true), no add() plano — error real
    // encontrado en vivo: si este identifier ya se sembró antes (archivos
    // completos en disco, ver seed_archive_org_item_core), un add() sin
    // overwrite falla ("allow_overwrite = false") tanto en un segundo "Ver"
    // como en la sanación de bug #26 tras reiniciar la app. Sin límite de
    // subida acá — no es el flujo de sembrado dedicado.
    let info = engine
        .add_seeding_from_disk(AddTorrentSource::TorrentBytes(bytes), None)
        .await
        .map_err(|e| e.to_string())?;

    // Fallback HTTP: la mayoría de los .torrent de archive.org dependen de
    // webseeds (BEP19) que librqbit no soporta (ver Tarea 9 de verificación
    // E2E). Si no se puede resolver un archivo reproducible, no es fatal —
    // el torrent sigue agregado y puede eventualmente completar por P2P.
    if let Ok(url) = archive_org::primary_video_file(http, identifier).await {
        engine
            .register_http_fallback(&info.id, url)
            .await
            .map_err(|e| e.to_string())?;
    }

    let media_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO media_items \
             (id, source_type, source_identifier, title, year, license, engine_torrent_id, primary_file_idx) \
             VALUES (?1, 'archive_org', ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                &media_id,
                identifier,
                title,
                year,
                licenseurl,
                &info.id,
                primary_file_idx.map(|i| i as i64),
            ),
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(info)
}

#[tauri::command]
pub async fn add_archive_org_item(
    app: tauri::AppHandle,
    http: State<'_, HttpClient>,
    engine: State<'_, EngineState>,
    db: State<'_, Db>,
    identifier: String,
    title: String,
    year: Option<i64>,
    licenseurl: Option<String>,
) -> Result<TorrentInfo, String> {
    let info = add_archive_org_item_core(&http.0, &engine.0, &db, &identifier, &title, year, licenseurl.clone()).await?;

    // Mismo sembrado automático en background que ya usa add_online_item
    // (online_library.rs) — bug real reportado en vivo: este comando
    // ("+/Buscar", SearchModal tab "Torrents") nunca lo disparaba, así
    // que un ítem agregado por acá quedaba dependiendo del proxy HTTP
    // para siempre en vez de terminar 100% local. No bloquea la
    // respuesta ni la reproducción, que ya está resuelta vía proxy.
    {
        let task_app = app.clone();
        let task_identifier = identifier.clone();
        let task_title = title.clone();
        tokio::spawn(async move {
            let http = task_app.state::<HttpClient>();
            let engine = task_app.state::<EngineState>();
            let db = task_app.state::<Db>();
            if let Err(e) =
                seed_archive_org_item_core(&http.0, &engine.0, &db, &task_identifier, &task_title, year, licenseurl)
                    .await
            {
                eprintln!("[popcorn] sembrado automático en background falló para '{task_identifier}': {e}");
            }
        });
    }

    Ok(info)
}

/// Sembrado real (ver plan): descarga por HTTP **todos** los archivos
/// reales del `.torrent` (no solo el reproducible — hallazgo real contra
/// cosmos-laundromat: los archivos de metadata chicos también cuentan
/// para el 100%, ver `archive_org::resolve_torrent_files`) a la ruta
/// exacta que cada uno espera, y recién entonces agrega el torrent al
/// motor — librqbit verifica el hash contra esos archivos ya en disco y
/// los marca 100% tenidos de entrada, sembrando de verdad al swarm real en
/// vez de solo servir por el proxy HTTP (`add_archive_org_item_core`).
/// Solo tiene sentido para la familia archive.org (archive_org/
/// blender_foundation/prelinger/feature_films) — Public Domain Torrents ya
/// es P2P real desde que se agrega.
pub(crate) async fn seed_archive_org_item_core(
    http: &reqwest::Client,
    engine: &Arc<dyn TorrentEngine>,
    db: &Db,
    identifier: &str,
    title: &str,
    year: Option<i64>,
    licenseurl: Option<String>,
) -> Result<TorrentInfo, String> {
    let downloads_dir = engine
        .downloads_dir()
        .ok_or_else(|| "el motor activo no soporta sembrado real (requiere EmbeddedRqbit)".to_string())?
        .to_path_buf();

    let torrent_bytes = archive_org::fetch_torrent_bytes_cached(http, identifier, Some(&downloads_dir))
        .await
        .map_err(|e| e.to_string())?;

    let files = archive_org::resolve_torrent_files(&torrent_bytes).map_err(|e| e.to_string())?;
    let primary_file_idx = archive_org::resolve_primary_file_idx(&files, identifier);

    // Streaming a disco en vez de cargar cada respuesta entera en memoria —
    // son películas, no los KB de un .torrent. Secuencial, no en paralelo:
    // el archivo grande domina el tiempo total de todos modos, y evita
    // saturar de golpe la conexión del usuario con varias descargas a la vez.
    for file in &files {
        let download_url = format!(
            "https://archive.org/download/{identifier}/{}",
            urlencoding::encode(&file.archive_org_filename)
        );
        let target_path = downloads_dir.join(&file.local_path);
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        }

        // Timeout por intento — sin esto una conexión colgada (confirmado en
        // vivo: un intento de descarga se quedó sin avanzar ni fallar,
        // indefinidamente) nunca dispara el próximo reintento de
        // send_with_retry ni termina el sembrado en background.
        let mut resp = crate::http_retry::send_with_retry(|| {
            http.get(&download_url).timeout(std::time::Duration::from_secs(300))
        })
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;

        use tokio::io::AsyncWriteExt;
        let mut out = tokio::fs::File::create(&target_path)
            .await
            .map_err(|e| e.to_string())?;
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            out.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
    }

    // Todos los archivos ya están completos en el lugar exacto —
    // add_seeding_from_disk corre el chequeo de hash de librqbit contra
    // ellos (con overwrite:true, necesario porque ya existen) y reconoce
    // el torrent 100% tenido de entrada, sin bajar nada por P2P.
    let info = engine
        .add_seeding_from_disk(
            AddTorrentSource::TorrentBytes(torrent_bytes),
            Some(DEFAULT_SEED_UPLOAD_BPS),
        )
        .await
        .map_err(|e| e.to_string())?;

    // Si ya existe una fila para este identifier (caso común: el sembrado
    // se dispara automático en background después de agregar rápido, ver
    // add_archive_org_item/add_online_item_inner), actualiza su
    // engine_torrent_id y primary_file_idx al resultado de ESTA descarga
    // real — bug real reportado en vivo: antes se saltaba en silencio, la
    // descarga real pasaba pero nada quedaba apuntando a ella, así que el
    // ítem seguía dependiendo del proxy/P2P para siempre pese al trabajo
    // ya hecho acá.
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let already_exists: bool = conn
            .query_row(
                "SELECT 1 FROM media_items WHERE source_identifier = ?1",
                [identifier],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(false);
        if already_exists {
            conn.execute(
                "UPDATE media_items SET engine_torrent_id = ?1, primary_file_idx = ?2 WHERE source_identifier = ?3",
                (&info.id, primary_file_idx.map(|i| i as i64), identifier),
            )
            .map_err(|e| e.to_string())?;
        } else {
            let media_id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO media_items \
                 (id, source_type, source_identifier, title, year, license, engine_torrent_id, primary_file_idx) \
                 VALUES (?1, 'archive_org', ?2, ?3, ?4, ?5, ?6, ?7)",
                (&media_id, identifier, title, year, licenseurl, &info.id, primary_file_idx.map(|i| i as i64)),
            )
            .map_err(|e| e.to_string())?;
        }
    }

    Ok(info)
}

#[tauri::command]
pub async fn seed_archive_org_item(
    http: State<'_, HttpClient>,
    engine: State<'_, EngineState>,
    db: State<'_, Db>,
    identifier: String,
    title: String,
    year: Option<i64>,
    licenseurl: Option<String>,
) -> Result<TorrentInfo, String> {
    seed_archive_org_item_core(&http.0, &engine.0, &db, &identifier, &title, year, licenseurl).await
}

#[cfg(test)]
mod healing_tests {
    use super::*;
    use async_trait::async_trait;
    use rusqlite::Connection;
    use std::collections::HashSet;
    use std::sync::Mutex as StdMutex;

    /// Motor en memoria — imita exactamente el defecto que causa el bug #26:
    /// solo un subconjunto de ids "existe" en la sesión actual, como pasaría
    /// tras reiniciar EmbeddedRqbit sin persistencia. `add` siempre otorga un
    /// id nuevo y lo suma a los existentes, igual que una sesión real.
    struct FakeEngine {
        existing_ids: StdMutex<HashSet<String>>,
        next_id: StdMutex<u32>,
    }

    impl FakeEngine {
        fn with_existing(ids: &[&str]) -> Self {
            Self {
                existing_ids: StdMutex::new(ids.iter().map(|s| s.to_string()).collect()),
                next_id: StdMutex::new(100),
            }
        }
    }

    #[async_trait]
    impl TorrentEngine for FakeEngine {
        async fn add(&self, _source: AddTorrentSource) -> anyhow::Result<TorrentInfo> {
            let id = {
                let mut n = self.next_id.lock().unwrap();
                *n += 1;
                n.to_string()
            };
            self.existing_ids.lock().unwrap().insert(id.clone());
            Ok(TorrentInfo {
                id,
                name: Some("fake".to_string()),
                info_hash: "deadbeef".to_string(),
                progress_bytes: 0,
                total_bytes: 0,
                download_speed_mbps: 0.0,
                upload_speed_mbps: 0.0,
                uploaded_bytes: 0,
                finished: false,
                state: "initializing".to_string(),
                error: None,
            })
        }
        async fn list(&self) -> anyhow::Result<Vec<TorrentInfo>> {
            Ok(vec![])
        }
        async fn pause(&self, _id: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn remove(&self, _id: &str, _delete_files: bool) -> anyhow::Result<()> {
            Ok(())
        }
        async fn stream_url(&self, id: &str, file_idx: usize) -> anyhow::Result<String> {
            if self.existing_ids.lock().unwrap().contains(id) {
                Ok(format!("http://fake/{id}/{file_idx}"))
            } else {
                Err(anyhow::anyhow!("torrent no encontrado"))
            }
        }
    }

    fn migrated_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(std::sync::Mutex::new(conn))
    }

    fn insert_media_item(
        db: &Db,
        id: &str,
        source_type: &str,
        source_identifier: &str,
        engine_torrent_id: Option<&str>,
    ) {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO media_items (id, source_type, source_identifier, title, engine_torrent_id) \
             VALUES (?1, ?2, ?3, 'Test', ?4)",
            (id, source_type, source_identifier, engine_torrent_id),
        )
        .unwrap();
    }

    fn stored_engine_torrent_id(db: &Db, media_id: &str) -> Option<String> {
        let conn = db.0.lock().unwrap();
        conn.query_row(
            "SELECT engine_torrent_id FROM media_items WHERE id = ?1",
            [media_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn add_torrent_inner_registers_media_item_that_survives_engine_restart() {
        let db = migrated_db();
        let engine: Arc<dyn TorrentEngine> = Arc::new(FakeEngine::with_existing(&[]));
        let magnet = "magnet:?xt=urn:btih:abc&dn=one+piece";

        let info = add_torrent_inner(&engine, &db, magnet.to_string(), None, None)
            .await
            .unwrap();

        let (source_type, source_identifier, title): (String, String, String) = {
            let conn = db.0.lock().unwrap();
            conn.query_row(
                "SELECT source_type, source_identifier, title FROM media_items WHERE engine_torrent_id = ?1",
                [&info.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap()
        };
        assert_eq!(source_type, "magnet");
        assert_eq!(source_identifier, magnet, "source_identifier debe ser el magnet crudo, para poder re-agregarlo");
        assert_eq!(title, "fake", "usa TorrentInfo.name cuando está presente");

        let media_id: String = {
            let conn = db.0.lock().unwrap();
            conn.query_row("SELECT id FROM media_items WHERE engine_torrent_id = ?1", [&info.id], |r| r.get(0))
                .unwrap()
        };

        // Simula un reinicio de la app: el motor pierde toda sesión previa.
        let restarted_engine: Arc<dyn TorrentEngine> = Arc::new(FakeEngine::with_existing(&[]));
        let http = reqwest::Client::new();
        let url = resolve_media_item_stream_url(&http, &restarted_engine, &db, &media_id, 0)
            .await
            .unwrap();
        assert!(url.starts_with("http://fake/"), "debe sanar re-agregando el magnet, no fallar: {url}");
    }

    #[tokio::test]
    async fn resolves_directly_when_engine_torrent_id_still_exists() {
        let db = migrated_db();
        insert_media_item(&db, "m1", "magnet", "magnet:?xt=urn:btih:abc", Some("5"));
        let engine: Arc<dyn TorrentEngine> = Arc::new(FakeEngine::with_existing(&["5"]));
        let http = reqwest::Client::new();

        let url = resolve_media_item_stream_url(&http, &engine, &db, "m1", 0)
            .await
            .unwrap();

        assert_eq!(url, "http://fake/5/0");
        // No debió tocar la fila — sigue siendo el id original.
        assert_eq!(stored_engine_torrent_id(&db, "m1").as_deref(), Some("5"));
    }

    #[tokio::test]
    async fn heals_stale_engine_torrent_id_for_magnet_source() {
        let db = migrated_db();
        // "stale-id" no está entre los ids que el motor reconoce — simula
        // exactamente la pérdida de sesión del bug #26.
        insert_media_item(&db, "m1", "magnet", "magnet:?xt=urn:btih:abc", Some("stale-id"));
        let engine: Arc<dyn TorrentEngine> = Arc::new(FakeEngine::with_existing(&[]));
        let http = reqwest::Client::new();

        let url = resolve_media_item_stream_url(&http, &engine, &db, "m1", 0)
            .await
            .unwrap();

        assert!(url.starts_with("http://fake/"));
        assert!(!url.contains("stale-id"), "no debe seguir usando el id viejo: {url}");
        let healed = stored_engine_torrent_id(&db, "m1");
        assert_ne!(healed.as_deref(), Some("stale-id"), "SQLite debe quedar actualizado con el id nuevo");
    }

    #[tokio::test]
    async fn heals_when_engine_torrent_id_is_null() {
        let db = migrated_db();
        insert_media_item(&db, "m1", "magnet", "magnet:?xt=urn:btih:abc", None);
        let engine: Arc<dyn TorrentEngine> = Arc::new(FakeEngine::with_existing(&[]));
        let http = reqwest::Client::new();

        let url = resolve_media_item_stream_url(&http, &engine, &db, "m1", 0)
            .await
            .unwrap();

        assert!(url.starts_with("http://fake/"));
        assert!(stored_engine_torrent_id(&db, "m1").is_some());
    }

    #[tokio::test]
    async fn errors_honestly_for_unrecoverable_torrent_file_source() {
        let db = migrated_db();
        insert_media_item(&db, "m1", "torrent_file", "whatever", Some("stale-id"));
        let engine: Arc<dyn TorrentEngine> = Arc::new(FakeEngine::with_existing(&[]));
        let http = reqwest::Client::new();

        let err = resolve_media_item_stream_url(&http, &engine, &db, "m1", 0)
            .await
            .unwrap_err();

        assert!(err.contains("no se puede re-agregar"), "debe explicar la limitación, no fallar en silencio: {err}");
        // No debió tocar la fila ante un fallo de sanación.
        assert_eq!(stored_engine_torrent_id(&db, "m1").as_deref(), Some("stale-id"));
    }

    #[tokio::test]
    async fn errors_when_media_item_does_not_exist() {
        let db = migrated_db();
        let engine: Arc<dyn TorrentEngine> = Arc::new(FakeEngine::with_existing(&[]));
        let http = reqwest::Client::new();

        assert!(resolve_media_item_stream_url(&http, &engine, &db, "no-existe", 0)
            .await
            .is_err());
    }
}

#[cfg(test)]
mod seed_tests {
    use super::*;
    use async_trait::async_trait;
    use rusqlite::Connection;

    fn migrated_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(std::sync::Mutex::new(conn))
    }

    /// downloads_dir() con default None (no lo sobreescribe) — cualquier
    /// motor que no sea EmbeddedRqbit cae acá.
    struct EngineWithoutDownloadsDir;
    #[async_trait]
    impl TorrentEngine for EngineWithoutDownloadsDir {
        async fn add(&self, _source: AddTorrentSource) -> anyhow::Result<TorrentInfo> {
            unimplemented!("no debería llegar a agregar nada si downloads_dir() no está soportado")
        }
        async fn list(&self) -> anyhow::Result<Vec<TorrentInfo>> {
            unimplemented!()
        }
        async fn pause(&self, _id: &str) -> anyhow::Result<()> {
            unimplemented!()
        }
        async fn remove(&self, _id: &str, _delete_files: bool) -> anyhow::Result<()> {
            unimplemented!()
        }
        async fn stream_url(&self, _id: &str, _file_idx: usize) -> anyhow::Result<String> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn errors_honestly_when_engine_does_not_support_downloads_dir() {
        let db = migrated_db();
        let engine: Arc<dyn TorrentEngine> = Arc::new(EngineWithoutDownloadsDir);
        let http = reqwest::Client::new();

        let err = seed_archive_org_item_core(&http, &engine, &db, "x", "X", None, None)
            .await
            .unwrap_err();
        assert!(
            err.contains("sembrado real"),
            "debe explicar la limitación, no fallar en silencio: {err}"
        );
    }

    /// Red real, deshabilitado por defecto: descarga completa el ítem más
    /// chico de Blender Foundation (~45.5MB, cosmos-laundromat) y confirma
    /// que, tras engine.add(), el TorrentInfo ya reporta 100% completo —
    /// prueba real de que librqbit lo reconoció sembrado desde el disco,
    /// sin bajar nada por P2P. Corre con `cargo test -- --ignored`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "red real, descarga ~45MB, no apto para CI por defecto"]
    async fn seed_archive_org_item_core_recognizes_a_fully_downloaded_file_as_complete() {
        let db = migrated_db();
        let tmp = std::env::temp_dir().join(format!("popcorn-seed-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let embedded = crate::engine::embedded_rqbit::EmbeddedRqbit::new_standalone(tmp).await.unwrap();
        let engine: Arc<dyn TorrentEngine> = Arc::new(embedded);
        let http = reqwest::Client::new();

        let info = seed_archive_org_item_core(
            &http,
            &engine,
            &db,
            "cosmos-laundromat",
            "Cosmos Laundromat",
            None,
            None,
        )
        .await
        .unwrap();
        assert!(info.total_bytes > 0, "debe conocer el tamaño total real");

        // librqbit corre el chequeo de hash en background (spawn_with_cancel,
        // confirmado leyendo torrent_state/mod.rs::_start) — no está listo
        // todavía en el instante en que add() retorna, hay que esperar a que
        // el estado deje "initializing". Timeout generoso (45MB debería
        // verificarse en segundos, no minutos) para no colgar el test si de
        // verdad no se reconoce.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut last_state = info.state.clone();
        let mut final_info = info;
        while std::time::Instant::now() < deadline {
            let list = engine.list().await.unwrap();
            let Some(current) = list.into_iter().find(|t| t.id == final_info.id) else {
                panic!("el torrent desapareció de la lista mientras se esperaba el chequeo");
            };
            last_state = current.state.clone();
            final_info = current;
            if last_state != "initializing" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }

        assert_eq!(
            final_info.progress_bytes, final_info.total_bytes,
            "el archivo ya estaba completo en disco antes de agregar — debe reconocerse 100% sembrado \
             una vez terminado el chequeo (estado final: {last_state})"
        );
    }

    /// Test pedido explícito por el usuario ante la duda de que el bug de
    /// reproducción fuera distinto a "archive.org está lento" — aísla el
    /// camino nativo de streaming (`/stream/`, sin `/proxy/`) para un
    /// archivo YA completo en disco, end-to-end: sembrar, confirmar
    /// `finished`, pedir `stream_url` (debe resolver `/stream/`, no
    /// `/proxy/`, gracias al fix del punto 4 de
    /// `resiliencia_streaming_archive_org.txt`) y efectivamente traer
    /// bytes reales con un request `Range` como haría un `<video>` real.
    /// Si esto falla, el bug está en nuestro servidor de streaming, no en
    /// archive.org — exactamente la distinción que pidió el usuario.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "red real, descarga ~45MB, no apto para CI por defecto"]
    async fn stream_url_serves_real_bytes_for_an_already_finished_seeded_torrent() {
        let db = migrated_db();
        let tmp = std::env::temp_dir().join(format!("popcorn-seed-stream-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let embedded = crate::engine::embedded_rqbit::EmbeddedRqbit::new_standalone(tmp).await.unwrap();
        let engine: Arc<dyn TorrentEngine> = Arc::new(embedded);
        let http = reqwest::Client::new();

        let info = seed_archive_org_item_core(&http, &engine, &db, "cosmos-laundromat", "Cosmos Laundromat", None, None)
            .await
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut finished = false;
        while std::time::Instant::now() < deadline {
            let list = engine.list().await.unwrap();
            let Some(current) = list.into_iter().find(|t| t.id == info.id) else {
                panic!("el torrent desapareció de la lista mientras se esperaba el chequeo");
            };
            if current.finished {
                finished = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
        assert!(finished, "el chequeo de piezas no terminó a tiempo — no se puede probar streaming sin esto");

        let url = engine.stream_url(&info.id, 0).await.expect("stream_url debe resolver para un torrent finished");
        assert!(
            url.contains("/stream/"),
            "un torrent finished debe servir local (/stream/), nunca por /proxy/ — url real: {url}"
        );

        let resp = http
            .get(&url)
            .header("Range", "bytes=0-1048575")
            .send()
            .await
            .expect("request Range a /stream/ para un archivo ya completo");
        assert!(
            resp.status().is_success(),
            "esperaba 200/206 sirviendo bytes reales de un archivo ya completo, recibí {}",
            resp.status()
        );
        let bytes = resp.bytes().await.expect("leer el cuerpo de la respuesta");
        assert!(!bytes.is_empty(), "la respuesta no debe venir vacía para un archivo ya completo en disco");
    }
}
