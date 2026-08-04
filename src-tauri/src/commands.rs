use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::ai::{commands::try_build_active_provider, curation, StructuredQuery};
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
    let (source_type, source_identifier, engine_torrent_id): (String, String, Option<String>) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT source_type, source_identifier, engine_torrent_id FROM media_items WHERE id = ?1",
            [media_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|e| e.to_string())?
    };

    let resolved_id = match engine_torrent_id {
        Some(id) if engine.exists(&id).await => id,
        _ => heal_media_item(http, engine, db, media_id, &source_type, &source_identifier).await?,
    };

    engine
        .stream_url(&resolved_id, file_idx)
        .await
        .map_err(|e| e.to_string())
}

/// Re-agrega un ítem al motor a partir de su `source_type`/`source_identifier`
/// guardados en `media_items` y persiste el `engine_torrent_id` nuevo.
async fn heal_media_item(
    http: &reqwest::Client,
    engine: &Arc<dyn TorrentEngine>,
    db: &Db,
    media_id: &str,
    source_type: &str,
    source_identifier: &str,
) -> Result<String, String> {
    let new_id = match source_type {
        "archive_org" => {
            let bytes = archive_org::fetch_torrent_bytes(http, source_identifier)
                .await
                .map_err(|e| e.to_string())?;
            let info = engine
                .add(AddTorrentSource::TorrentBytes(bytes))
                .await
                .map_err(|e| e.to_string())?;
            // No fatal si esto falla — igual que en add_archive_org_item, el
            // torrent re-agregado puede completar por P2P sin el fallback.
            if let Ok(url) = archive_org::primary_video_file(http, source_identifier).await {
                engine
                    .register_http_fallback(&info.id, url)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            info.id
        }
        "magnet" => {
            let info = engine
                .add(AddTorrentSource::Magnet(source_identifier.to_string()))
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
    conn.execute(
        "UPDATE media_items SET engine_torrent_id = ?1 WHERE id = ?2",
        (&new_id, media_id),
    )
    .map_err(|e| e.to_string())?;

    Ok(new_id)
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
    let items = archive_org::search(&http.0, &query, mediatype_filter.as_deref())
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
