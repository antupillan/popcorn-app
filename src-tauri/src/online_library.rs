use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::ai::{commands::try_build_active_provider, curation, AiProvider};
use crate::commands::{add_archive_org_item_core, seed_archive_org_item_core, EngineState, HttpClient};
use crate::db::Db;
use crate::engine::{AddTorrentSource, TorrentEngine, TorrentInfo};
use crate::sources::{archive_org, public_domain_torrents};

/// Ítem unificado de las fuentes Online (ver plan, Biblioteca unificada) —
/// `kind` distingue de qué catálogo salió, no necesariamente del
/// `media_items.source_type` que termina en DB: archive.org, Blender
/// Foundation, Prelinger y feature_films comparten todos
/// `source_type='archive_org'` (ninguno tiene fila propia en el CHECK, ver
/// migración) porque los cuatro resuelven por identifier de archive.org —
/// solo `kind` los distingue del lado del frontend/curación.
#[derive(Serialize, Deserialize, Clone)]
pub struct OnlineItem {
    pub kind: String, // "archive_org" | "public_domain_torrents" | "blender_foundation" | "prelinger" | "feature_films"
    pub identifier: String,
    pub title: String,
    pub year: Option<i64>,
    pub license: Option<String>,
    pub thumbnail_url: Option<String>,
}

fn online_item_from_archive_org(kind: &str, item: archive_org::ArchiveOrgItem) -> OnlineItem {
    OnlineItem {
        kind: kind.to_string(),
        identifier: item.identifier,
        title: item.title,
        year: item.year,
        license: item.licenseurl,
        thumbnail_url: Some(item.thumbnail_url),
    }
}

fn online_item_from_pdt(item: public_domain_torrents::PublicDomainMovie) -> OnlineItem {
    OnlineItem {
        kind: "public_domain_torrents".to_string(),
        identifier: item.identifier,
        title: item.title,
        year: None,
        license: None,
        thumbnail_url: None,
    }
}

fn curation_settings_for(db: &Db, source_id: &str) -> Result<(bool, Option<String>), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT curation_enabled, curation_hint FROM source_settings WHERE id = ?1",
        [source_id],
        |r| Ok((r.get::<_, i64>(0)? != 0, r.get(1)?)),
    )
    .map_err(|e| e.to_string())
}

fn archive_org_mediatype_filter(db: &Db) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT mediatype_filter FROM source_settings WHERE id = 'archive_org'",
        [],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

/// Fuentes rápidas de Online: archive.org (modo "browse sin búsqueda") +
/// Blender Foundation (síncrono, sin I/O) — separado de Public Domain
/// Torrents (`browse_public_domain_torrents_inner` abajo) porque ese sitio
/// de terceros es mucho más lento (~13s medido en vivo contra ~1.5s de
/// archive.org, ver sección "investigar lentitud" del plan): un solo
/// comando esperando a ambas fuentes frenaba toda la pestaña Online detrás
/// de la más lenta. archive.org fallando se loguea y se saltea, no tira
/// abajo a Blender (mismo criterio que `iptv::list_channels`).
///
/// Sin curación acá a propósito — separada en `curate_online_items_inner`
/// (comando aparte, ver abajo). Encontrado en vivo: con un proveedor de IA
/// activo pero inalcanzable, curar síncrono acá significaba hasta varios
/// minutos de espera antes de mostrar nada, aunque el fetch en sí tarda
/// ~1-2s — la pestaña Online debe cargar igual de rápido con o sin IA
/// configurada, la curación es una mejora que llega después, no un gate.
async fn browse_online_library_fast_inner(
    http: &reqwest::Client,
    db: &Db,
) -> Result<Vec<OnlineItem>, String> {
    let mediatype_filter = archive_org_mediatype_filter(db)?;

    let (archive_org_result, prelinger_result, feature_films_result) = tokio::join!(
        archive_org::browse_movies(http, mediatype_filter.as_deref()),
        archive_org::browse_prelinger(http),
        archive_org::browse_feature_films(http),
    );

    let mut all = Vec::new();
    match archive_org_result {
        Ok(items) => all.extend(items.into_iter().map(|i| online_item_from_archive_org("archive_org", i))),
        Err(e) => eprintln!("[popcorn] fuente Online 'archive_org' falló: {e}"),
    }
    match prelinger_result {
        Ok(items) => all.extend(items.into_iter().map(|i| online_item_from_archive_org("prelinger", i))),
        Err(e) => eprintln!("[popcorn] fuente Online 'prelinger' falló: {e}"),
    }
    match feature_films_result {
        Ok(items) => all.extend(items.into_iter().map(|i| online_item_from_archive_org("feature_films", i))),
        Err(e) => eprintln!("[popcorn] fuente Online 'feature_films' falló: {e}"),
    }

    // Allowlist fija ya vetted a mano — nunca pasa por curate_by_hint (ver
    // source_settings.curation_enabled=0 para esta fuente).
    all.extend(
        archive_org::blender_foundation_items()
            .into_iter()
            .map(|i| online_item_from_archive_org("blender_foundation", i)),
    );

    Ok(all)
}

/// Identidad estable de un `OnlineItem` para el caché de curación/ping —
/// `kind` distingue fuentes que comparten `identifier` (ver comentario del
/// struct), así que ninguno de los dos solo alcanza.
fn online_item_key(item: &OnlineItem) -> String {
    format!("{}:{}", item.kind, item.identifier)
}

/// URL a pinguear para confirmar disponibilidad — Public Domain Torrents
/// ya trae la URL de descarga resuelta como `identifier`; el resto
/// (archive.org y sus tres variantes) resuelve al `.torrent` real que
/// `add_archive_org_item_core` terminaría descargando.
fn ping_target_for(item: &OnlineItem) -> String {
    if item.kind == "public_domain_torrents" {
        item.identifier.clone()
    } else {
        archive_org::torrent_url(&item.identifier)
    }
}

/// Cura (si `curation_enabled` y hay proveedor activo) y pinguea
/// disponibilidad — en paralelo, no una tras otra — un grupo de ítems de
/// una misma fuente (`source_id` = mismo id que `source_settings`). El
/// ping corre siempre, incluso con `curation_enabled=0` (ej.
/// `blender_foundation`, allowlist vetted a mano — igual puede
/// desaparecer de archive.org con el tiempo). Ítems con
/// `consecutive_ping_failures` sobre el umbral quedan afuera del
/// resultado.
async fn curate_and_ping_bucket(
    db: &Db,
    http: &reqwest::Client,
    ping_semaphore: std::sync::Arc<tokio::sync::Semaphore>,
    source_id: &str,
    curation_enabled: bool,
    provider: Option<&dyn AiProvider>,
    items: Vec<OnlineItem>,
    hint: Option<&str>,
) -> Result<Vec<OnlineItem>, String> {
    if items.is_empty() {
        return Ok(items);
    }

    let recently_pinged = crate::availability_ping::recently_pinged_keys(db, source_id)?;
    let ping_targets: Vec<(String, String)> = items
        .iter()
        .map(|i| (online_item_key(i), ping_target_for(i)))
        .filter(|(key, _)| !recently_pinged.contains(key))
        .collect();
    let http_for_ping = http.clone();
    let ping_fut = crate::availability_ping::ping_many(ping_targets, ping_semaphore, move |url| {
        let client = http_for_ping.clone();
        async move { crate::availability_ping::ping_ok(&client, &url).await }
    });

    let curate_fut = async {
        match provider {
            Some(provider) if curation_enabled => {
                curation::curate_by_hint(db, source_id, provider, items, |i: &OnlineItem| i.title.as_str(), online_item_key, hint)
                    .await
            }
            _ => Ok(items),
        }
    };

    let (ping_results, curated) = tokio::join!(ping_fut, curate_fut);
    crate::availability_ping::record_ping_results(db, source_id, &ping_results)?;
    let mut curated = curated?;
    let failure_counts = crate::availability_ping::ping_failure_counts(db, source_id)?;
    curated.retain(|i| {
        failure_counts.get(&online_item_key(i)).copied().unwrap_or(0)
            < crate::availability_ping::CONSECUTIVE_FAILURES_THRESHOLD
    });
    Ok(curated)
}

/// Re-cura una lista de `OnlineItem` ya obtenida (vía `browse_online_library`)
/// — el frontend la llama después de renderizar el resultado rápido sin
/// curar, sin bloquear esa primera carga. Las cuatro familias de
/// archive.org (`archive_org`/`prelinger`/`feature_films`/
/// `blender_foundation`, la única que llega acá aparte de las tres
/// explícitas — Public Domain Torrents tiene su propio comando, ver
/// `browse_public_domain_torrents_inner`) corren en paralelo vía
/// `curate_and_ping_bucket`, compartiendo un solo semáforo de ping: cada
/// una usando el suyo propio significaba hasta 4x `PING_CONCURRENCY`
/// conexiones simultáneas reales contra archive.org — verificado en vivo
/// que eso alcanza para gatillar throttling del origen que ni los
/// reintentos de `ping_ok` superan (Mandato 1, no hipótesis).
async fn curate_online_items_inner(db: &Db, http: &reqwest::Client, items: Vec<OnlineItem>) -> Result<Vec<OnlineItem>, String> {
    let provider = try_build_active_provider(db)?;
    let ping_semaphore = crate::availability_ping::new_ping_semaphore();

    let mut archive_org_items = Vec::new();
    let mut prelinger_items = Vec::new();
    let mut feature_films_items = Vec::new();
    let mut blender_foundation_items = Vec::new();
    for item in items {
        match item.kind.as_str() {
            "archive_org" => archive_org_items.push(item),
            "prelinger" => prelinger_items.push(item),
            "feature_films" => feature_films_items.push(item),
            _ => blender_foundation_items.push(item),
        }
    }

    let (archive_org_enabled, archive_org_hint) = curation_settings_for(db, "archive_org")?;
    let (prelinger_enabled, prelinger_hint) = curation_settings_for(db, "prelinger")?;
    let (feature_films_enabled, feature_films_hint) = curation_settings_for(db, "feature_films")?;
    let (blender_foundation_enabled, blender_foundation_hint) = curation_settings_for(db, "blender_foundation")?;

    let (archive_org_items, prelinger_items, feature_films_items, blender_foundation_items) = tokio::join!(
        curate_and_ping_bucket(db, http, ping_semaphore.clone(), "archive_org", archive_org_enabled, provider.as_deref(), archive_org_items, archive_org_hint.as_deref()),
        curate_and_ping_bucket(db, http, ping_semaphore.clone(), "prelinger", prelinger_enabled, provider.as_deref(), prelinger_items, prelinger_hint.as_deref()),
        curate_and_ping_bucket(db, http, ping_semaphore.clone(), "feature_films", feature_films_enabled, provider.as_deref(), feature_films_items, feature_films_hint.as_deref()),
        curate_and_ping_bucket(db, http, ping_semaphore, "blender_foundation", blender_foundation_enabled, provider.as_deref(), blender_foundation_items, blender_foundation_hint.as_deref()),
    );

    let mut all = Vec::new();
    all.extend(archive_org_items?);
    all.extend(prelinger_items?);
    all.extend(feature_films_items?);
    all.extend(blender_foundation_items?);
    Ok(all)
}

#[tauri::command]
pub async fn curate_online_library(
    http: State<'_, HttpClient>,
    db: State<'_, Db>,
    items: Vec<OnlineItem>,
) -> Result<Vec<OnlineItem>, String> {
    curate_online_items_inner(&db, &http.0, items).await
}

/// Public Domain Torrents solo — a diferencia del grupo rápido, un fallo
/// acá se propaga como `Err` en vez de loguearse y saltearse: es la única
/// fuente de este comando, el frontend decide cómo mostrarlo (nota inline,
/// no bloquea el resto de la pestaña Online que ya se renderizó aparte).
async fn browse_public_domain_torrents_inner(
    http: &reqwest::Client,
    db: &Db,
) -> Result<Vec<OnlineItem>, String> {
    let (pdt_enabled, pdt_hint) = curation_settings_for(db, "public_domain_torrents")?;
    let provider = try_build_active_provider(db)?;

    let items = public_domain_torrents::browse(http)
        .await
        .map_err(|e| e.to_string())?;
    let items: Vec<OnlineItem> = items.into_iter().map(online_item_from_pdt).collect();
    curate_and_ping_bucket(
        db,
        http,
        crate::availability_ping::new_ping_semaphore(),
        "public_domain_torrents",
        pdt_enabled,
        provider.as_deref(),
        items,
        pdt_hint.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn browse_online_library(
    http: State<'_, HttpClient>,
    db: State<'_, Db>,
) -> Result<Vec<OnlineItem>, String> {
    browse_online_library_fast_inner(&http.0, &db).await
}

#[tauri::command]
pub async fn browse_public_domain_torrents(
    http: State<'_, HttpClient>,
    db: State<'_, Db>,
) -> Result<Vec<OnlineItem>, String> {
    browse_public_domain_torrents_inner(&http.0, &db).await
}

/// Descarga/agrega un ítem Online al motor y lo registra en `media_items`.
/// `kind` despacha: "archive_org"/"blender_foundation"/"prelinger"/
/// "feature_films" comparten `add_archive_org_item_core` (los cuatro son
/// ítems de archive.org, ver `OnlineItem`); "public_domain_torrents" tiene
/// su propio flujo — sin fallback HTTP (P2P puro, limitación conocida, ver
/// plan).
async fn add_online_item_inner(
    http: &reqwest::Client,
    engine: &Arc<dyn TorrentEngine>,
    db: &Db,
    kind: &str,
    identifier: &str,
    title: &str,
    year: Option<i64>,
    license: Option<String>,
) -> Result<TorrentInfo, String> {
    match kind {
        "archive_org" | "blender_foundation" | "prelinger" | "feature_films" => {
            add_archive_org_item_core(http, engine, db, identifier, title, year, license).await
        }
        "public_domain_torrents" => {
            let bytes = public_domain_torrents::fetch_torrent_bytes(http, identifier)
                .await
                .map_err(|e| e.to_string())?;
            let info = engine
                .add(AddTorrentSource::TorrentBytes(bytes))
                .await
                .map_err(|e| e.to_string())?;
            let media_id = uuid::Uuid::new_v4().to_string();
            let conn = db.0.lock().map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO media_items \
                 (id, source_type, source_identifier, title, year, license, engine_torrent_id) \
                 VALUES (?1, 'public_domain_torrents', ?2, ?3, ?4, ?5, ?6)",
                (&media_id, identifier, title, year, &license, &info.id),
            )
            .map_err(|e| e.to_string())?;
            Ok(info)
        }
        other => Err(format!("kind de ítem Online desconocido: {other}")),
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn add_online_item(
    app: tauri::AppHandle,
    http: State<'_, HttpClient>,
    engine: State<'_, EngineState>,
    db: State<'_, Db>,
    kind: String,
    identifier: String,
    title: String,
    year: Option<i64>,
    license: Option<String>,
) -> Result<TorrentInfo, String> {
    let info = add_online_item_inner(
        &http.0,
        &engine.0,
        &db,
        &kind,
        &identifier,
        &title,
        year,
        license.clone(),
    )
    .await?;

    // Sembrado automático en background tras "Ver", solo familia
    // archive.org (Public Domain Torrents ya es P2P real desde que se
    // agrega) — no bloquea la respuesta ni la reproducción, que ya está
    // resuelta vía proxy. seed_archive_org_item_core saltea el insert en
    // media_items si la fila ya existe (la que acabamos de crear arriba).
    if matches!(kind.as_str(), "archive_org" | "blender_foundation" | "prelinger" | "feature_films") {
        let task_app = app.clone();
        let task_identifier = identifier.clone();
        let task_title = title.clone();
        tokio::spawn(async move {
            let http = task_app.state::<HttpClient>();
            let engine = task_app.state::<EngineState>();
            let db = task_app.state::<Db>();
            if let Err(e) = seed_archive_org_item_core(
                &http.0,
                &engine.0,
                &db,
                &task_identifier,
                &task_title,
                year,
                license,
            )
            .await
            {
                eprintln!("[popcorn] sembrado automático falló para {task_identifier}: {e}");
            }
        });
    }

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rusqlite::Connection;

    fn migrated_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(std::sync::Mutex::new(conn))
    }

    #[test]
    fn online_item_from_archive_org_carries_kind_and_thumbnail() {
        let item = archive_org::ArchiveOrgItem {
            identifier: "sita_sings_the_blues".to_string(),
            title: "Sita Sings the Blues".to_string(),
            year: Some(2008),
            licenseurl: Some("http://creativecommons.org/licenses/by-sa/3.0/".to_string()),
            thumbnail_url: "https://archive.org/services/img/sita_sings_the_blues".to_string(),
        };
        let online = online_item_from_archive_org("blender_foundation", item);
        assert_eq!(online.kind, "blender_foundation");
        assert_eq!(online.identifier, "sita_sings_the_blues");
        assert_eq!(online.year, Some(2008));
        assert!(online.thumbnail_url.is_some());
    }

    #[test]
    fn online_item_from_pdt_has_no_year_license_or_thumbnail() {
        let item = public_domain_torrents::PublicDomainMovie {
            title: "Nosferatu".to_string(),
            identifier: "http://example.org/nosferatu.torrent".to_string(),
        };
        let online = online_item_from_pdt(item);
        assert_eq!(online.kind, "public_domain_torrents");
        assert_eq!(online.year, None);
        assert_eq!(online.license, None);
        assert_eq!(online.thumbnail_url, None);
    }

    #[test]
    fn curation_settings_for_reads_seeded_public_domain_torrents_row() {
        let db = migrated_db();
        let (enabled, hint) = curation_settings_for(&db, "public_domain_torrents").unwrap();
        assert!(enabled);
        assert!(hint.unwrap().contains("dominio público"));
    }

    #[test]
    fn curation_settings_for_reads_seeded_blender_foundation_row_disabled() {
        let db = migrated_db();
        let (enabled, _hint) = curation_settings_for(&db, "blender_foundation").unwrap();
        assert!(!enabled, "allowlist ya vetted, curación desactivada por default");
    }

    #[test]
    fn archive_org_mediatype_filter_reads_seeded_movies_value() {
        let db = migrated_db();
        assert_eq!(archive_org_mediatype_filter(&db).unwrap().as_deref(), Some("movies"));
    }

    struct PanicEngine;
    #[async_trait]
    impl TorrentEngine for PanicEngine {
        async fn add(&self, _source: AddTorrentSource) -> anyhow::Result<TorrentInfo> {
            panic!("no debería tocar el motor para un kind desconocido")
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
    async fn add_online_item_errors_honestly_for_unknown_kind_without_touching_engine() {
        let db = migrated_db();
        let engine: Arc<dyn TorrentEngine> = Arc::new(PanicEngine);
        let http = reqwest::Client::new();

        let err = add_online_item_inner(&http, &engine, &db, "netflix", "x", "X", None, None)
            .await
            .unwrap_err();
        assert!(err.contains("desconocido"), "debe explicar la limitación, no fallar en silencio: {err}");
    }

    /// Red real, deshabilitado por defecto: descarga un .torrent real de
    /// Public Domain Torrents, lo agrega a un EmbeddedRqbit real y confirma
    /// que quedó registrado en media_items. Corre con `--ignored`.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "red real, no apto para CI por defecto"]
    async fn add_online_item_downloads_and_registers_a_real_public_domain_torrent() {
        let db = migrated_db();
        let tmp = std::env::temp_dir().join(format!("popcorn-online-lib-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).unwrap();
        let embedded = crate::engine::embedded_rqbit::EmbeddedRqbit::new_standalone(tmp).await.unwrap();
        let engine: Arc<dyn TorrentEngine> = Arc::new(embedded);
        let http = reqwest::Client::new();

        let identifier = "http://www.publicdomaintorrents.com/bt/btdownload.php?type=torrent&file=Thirteenth_Guest.mp4.torrent";
        let info = add_online_item_inner(
            &http,
            &engine,
            &db,
            "public_domain_torrents",
            identifier,
            "Thirteenth Guest",
            None,
            None,
        )
        .await
        .unwrap();

        assert!(!info.id.is_empty());
        let conn = db.0.lock().unwrap();
        let (source_type, title): (String, String) = conn
            .query_row(
                "SELECT source_type, title FROM media_items WHERE engine_torrent_id = ?1",
                [&info.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(source_type, "public_domain_torrents");
        assert_eq!(title, "Thirteenth Guest");
    }

    fn item(kind: &str, identifier: &str) -> OnlineItem {
        OnlineItem {
            kind: kind.to_string(),
            identifier: identifier.to_string(),
            title: format!("título de {identifier}"),
            year: None,
            license: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn ping_target_for_uses_identifier_directly_for_pdt_and_torrent_url_for_the_rest() {
        let pdt = item("public_domain_torrents", "http://example.org/x.torrent");
        assert_eq!(ping_target_for(&pdt), "http://example.org/x.torrent");

        let ao = item("archive_org", "sita_sings_the_blues");
        assert_eq!(ping_target_for(&ao), archive_org::torrent_url("sita_sings_the_blues"));

        let blender = item("blender_foundation", "sintel");
        assert_eq!(ping_target_for(&blender), archive_org::torrent_url("sintel"));
    }

    #[test]
    fn online_item_key_combines_kind_and_identifier() {
        let i = item("prelinger", "abc123");
        assert_eq!(online_item_key(&i), "prelinger:abc123");
    }

    struct ScoredHintProvider(i32);
    #[async_trait]
    impl AiProvider for ScoredHintProvider {
        fn name(&self) -> &'static str {
            "fake-scored"
        }
        async fn parse_query(&self, _text: &str) -> anyhow::Result<crate::ai::StructuredQuery> {
            unreachable!("no lo usa este test")
        }
        async fn curate_results(
            &self,
            _query: &crate::ai::StructuredQuery,
            _candidates: &[String],
        ) -> anyhow::Result<Vec<usize>> {
            unreachable!("no lo usa este test")
        }
        async fn curate_by_hint(
            &self,
            candidates: &[String],
            _hint: Option<&str>,
        ) -> anyhow::Result<Vec<crate::ai::ScoredCandidate>> {
            Ok((0..candidates.len())
                .map(|i| crate::ai::ScoredCandidate { index: i, score: self.0 })
                .collect())
        }
        async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
        async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
    }

    // Puerto sin listener en loopback: el ping falla rápido, sin red real
    // ni DNS — a estos tests no les importa el resultado del ping salvo
    // el que lo prueba explícitamente.
    fn unroutable(identifier: &str) -> OnlineItem {
        item("public_domain_torrents", &format!("http://127.0.0.1:1/{identifier}"))
    }

    #[tokio::test]
    async fn curate_and_ping_bucket_pings_even_with_curation_disabled() {
        let db = migrated_db();
        let http = reqwest::Client::new();
        let i = unroutable("a");
        let key = online_item_key(&i);

        let result = curate_and_ping_bucket(&db, &http, crate::availability_ping::new_ping_semaphore(), "src", false, None, vec![i], None)
            .await
            .unwrap();

        assert_eq!(result.len(), 1, "sin curación IA el ítem pasa igual, solo se pinguea");
        let counts = crate::availability_ping::ping_failure_counts(&db, "src").unwrap();
        assert_eq!(counts.get(&key), Some(&1), "el ping debe haber corrido y registrado la falla");
    }

    #[tokio::test]
    async fn curate_and_ping_bucket_drops_items_over_the_ping_failure_threshold() {
        let db = migrated_db();
        let http = reqwest::Client::new();
        let i = unroutable("dead");
        let key = online_item_key(&i);
        crate::availability_ping::record_ping_results(&db, "src", &vec![(key, false); 5]).unwrap();

        let result = curate_and_ping_bucket(&db, &http, crate::availability_ping::new_ping_semaphore(), "src", false, None, vec![i], None)
            .await
            .unwrap();

        assert!(result.is_empty(), "un ítem con 5 fallas de ping consecutivas debe quedar afuera");
    }

    #[tokio::test]
    async fn curate_and_ping_bucket_curates_and_caches_when_enabled() {
        let db = migrated_db();
        let http = reqwest::Client::new();
        let provider = ScoredHintProvider(88);
        let i = item("archive_org", "some_movie");
        let key = online_item_key(&i);

        let result = curate_and_ping_bucket(&db, &http, crate::availability_ping::new_ping_semaphore(), "archive_org", true, Some(&provider), vec![i], Some("crit"))
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        let conn = db.0.lock().unwrap();
        let score: Option<i64> = conn
            .query_row(
                "SELECT score FROM curation_cache WHERE source_id = 'archive_org' AND item_key = ?1",
                [&key],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(score, Some(88));
    }

    #[tokio::test]
    async fn curate_and_ping_bucket_skips_repinging_items_checked_within_the_ttl() {
        let db = migrated_db();
        let http = reqwest::Client::new();
        // No-ruteable a propósito: si de verdad se re-pinguea, falla y el
        // conteo de fallas sube — el test detecta eso.
        let i = unroutable("recently_ok");
        let key = online_item_key(&i);
        crate::availability_ping::record_ping_results(&db, "src", &[(key.clone(), true)]).unwrap();

        let result = curate_and_ping_bucket(&db, &http, crate::availability_ping::new_ping_semaphore(), "src", false, None, vec![i], None)
            .await
            .unwrap();

        assert_eq!(result.len(), 1, "un ítem pingueado ok hace poco no debe re-pinguearse ni caer, aunque el ping real fallaría");
        let counts = crate::availability_ping::ping_failure_counts(&db, "src").unwrap();
        assert_eq!(counts.get(&key), Some(&0), "el conteo de fallas no debe tocarse — no hubo re-ping real");
    }
}
