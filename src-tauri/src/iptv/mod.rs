mod hls_playlist;
mod parse;
pub mod recorder;

use anyhow::Context;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::ai::{commands::try_build_active_provider, curation, AiProvider};
use crate::db::Db;

#[derive(Serialize, Clone)]
pub struct IptvSource {
    pub id: String,
    pub name: String,
    pub source_kind: String, // "url" | "file"
    pub playlist_url: Option<String>,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
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
/// tira abajo a las demás. Sin curación acá a propósito — separada en
/// `curate_channels` (comando aparte, ver abajo). Encontrado en vivo: con
/// un proveedor de IA activo, curar síncrono acá bloqueaba la pestaña
/// Canales varios segundos/minutos antes de mostrar nada, aunque el fetch
/// en sí es rápido — mismo bug ya resuelto en `browse_online_library_fast_inner`
/// (ver online_library.rs), nunca replicado acá hasta ahora.
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

    let mut all = Vec::new();
    for source in &enabled {
        match fetch_channels_for_source(&app, &http.0, source).await {
            Ok(mut channels) => all.append(&mut channels),
            Err(e) => eprintln!("[popcorn] fuente IPTV '{}' falló: {e}", source.name),
        }
    }
    Ok(all)
}

/// Cura una lista de canales ya obtenida (ver `list_channels`) — se llama
/// después de renderizar el resultado rápido sin curar, nunca antes.
/// Agrupa por `source_id` (a diferencia de `curate_online_items_inner`, que
/// agrupa por un `kind` fijo: acá las fuentes son dinámicas, el usuario
/// agrega las que quiere) para aplicar el `curation_hint` correcto de cada
/// fuente. Orden final por `score` (ver `ai::curation::curate_by_hint`), no
/// por orden de aparición. Por cada grupo corre, en paralelo
/// (`tokio::join!`): curación IA (con caché, solo cura ítems nuevos o con
/// hint cambiado) y ping de disponibilidad (`availability_ping`, corre
/// siempre, incluso con `curation_enabled=0` — son chequeos distintos).
/// Canales con `consecutive_ping_failures` por encima del umbral quedan
/// afuera del resultado.
async fn curate_channels_inner(
    db: &Db,
    http: &reqwest::Client,
    provider: Option<&dyn AiProvider>,
    channels: Vec<Channel>,
) -> Result<Vec<Channel>, String> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<Channel>> = std::collections::HashMap::new();
    for ch in channels {
        groups
            .entry(ch.source_id.clone())
            .or_insert_with(|| {
                order.push(ch.source_id.clone());
                Vec::new()
            })
            .push(ch);
    }

    // Un solo semáforo para todas las fuentes de esta llamada — los grupos
    // se procesan secuenciales (no en paralelo entre sí, a diferencia de
    // Online), pero compartirlo es igual de correcto y evita duplicar el
    // criterio de a dónde vive el límite real.
    let ping_semaphore = crate::availability_ping::new_ping_semaphore();

    let mut result = Vec::new();
    for source_id in order {
        let group = groups.remove(&source_id).unwrap();

        let ping_targets: Vec<(String, String)> =
            group.iter().map(|c| (c.url.clone(), c.url.clone())).collect();
        let http_for_ping = http.clone();
        let ping_fut = crate::availability_ping::ping_many(ping_targets, ping_semaphore.clone(), move |url| {
            let client = http_for_ping.clone();
            async move { probe_manifest(&client, &url).await.is_ok() }
        });

        let source_id_for_curate = source_id.clone();
        let curate_fut = async move {
            if let Some(provider) = provider {
                if curation_enabled_for(db, &source_id_for_curate)? {
                    let hint = curation_hint_for(db, &source_id_for_curate)?;
                    return curation::curate_by_hint(
                        db,
                        &source_id_for_curate,
                        provider,
                        group,
                        |c: &Channel| c.name.as_str(),
                        |c: &Channel| c.url.clone(),
                        hint.as_deref(),
                    )
                    .await;
                }
            }
            Ok(group)
        };

        let (ping_results, curated) = tokio::join!(ping_fut, curate_fut);
        crate::availability_ping::record_ping_results(db, &source_id, &ping_results)?;
        let mut curated = curated?;
        let failure_counts = crate::availability_ping::ping_failure_counts(db, &source_id)?;
        curated.retain(|c| {
            failure_counts.get(&c.url).copied().unwrap_or(0)
                < crate::availability_ping::CONSECUTIVE_FAILURES_THRESHOLD
        });
        result.extend(curated);
    }
    Ok(result)
}

#[tauri::command]
pub async fn curate_channels(
    db: State<'_, Db>,
    http: State<'_, crate::commands::HttpClient>,
    channels: Vec<Channel>,
) -> Result<Vec<Channel>, String> {
    let provider = try_build_active_provider(&db)?;
    curate_channels_inner(&db, &http.0, provider.as_deref(), channels).await
}

/// Confirma que una respuesta HTTP ya obtenida es un manifest HLS real
/// (status 2xx + cuerpo que arranca con `#EXTM3U`) — separado del fetch en
/// sí porque tanto el comando de playback como `probe_manifest` (ping de
/// curación, ver `availability_ping.rs`) llegan acá después de su propio
/// `http_retry::send_with_retry`, sin duplicar la validación del cuerpo.
async fn validate_manifest_response(resp: reqwest::Response) -> Result<String, String> {
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

/// "Supervisión liviana" de playback (ver plan) y ping de disponibilidad
/// para el caché de curación (ver `availability_ping.rs`) comparten esta
/// función — ambos usos van con la política de 5 reintentos de
/// `http_retry`. Se probó en un solo intento primero, pero eso genera
/// falsos positivos ante orígenes con fallo intermitente conocido (mismo
/// hallazgo que motivó el reintento en `availability_ping::ping_ok` —
/// verificado en vivo, no solo hipotético). "Disponible" para un canal
/// IPTV es que cargue un manifest real, no solo que el status sea 2xx (a
/// diferencia de la familia archive.org/PDT) — un canal roto suele
/// devolver 200 con una página de error en vez de un manifest, aclarado
/// explícitamente por el usuario al pedir esto. Devuelve la URL final
/// (post-redirect) para que el frontend apunte ahí.
pub(crate) async fn probe_manifest(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = crate::http_retry::send_with_retry(|| client.get(url))
        .await
        .map_err(|e| e.to_string())?;
    validate_manifest_response(resp).await
}

#[tauri::command]
pub async fn validate_channel_manifest(
    http: State<'_, crate::commands::HttpClient>,
    url: String,
) -> Result<String, String> {
    probe_manifest(&http.0, &url).await
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

    fn seed_source_settings(db: &Db, id: &str, curation_enabled: bool, hint: Option<&str>) {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO source_settings (id, curation_enabled, curation_hint) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, curation_enabled as i64, hint],
        )
        .unwrap();
    }

    fn channel(source_id: &str, name: &str) -> Channel {
        Channel {
            name: name.to_string(),
            // Puerto sin listener en loopback: el ping falla rápido
            // ("connection refused", sin DNS) sin depender de red real ni
            // de un mock — a estos tests no les importa el resultado del
            // ping, solo que curate_channels_inner no explote por él.
            url: format!("http://127.0.0.1:1/{name}.m3u8"),
            group: None,
            logo_url: None,
            tvg_id: None,
            source_id: source_id.to_string(),
        }
    }

    /// Incluye todos los candidatos con un score fijo — no filtra ni
    /// reordena, pero graba el `hint` recibido en cada llamada para que
    /// el test pueda verificar que cada grupo usó su propio hint.
    struct HintRecordingProvider(std::sync::Mutex<Vec<Option<String>>>);
    #[async_trait]
    impl AiProvider for HintRecordingProvider {
        fn name(&self) -> &'static str {
            "fake-hint-recording"
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
            hint: Option<&str>,
        ) -> anyhow::Result<Vec<crate::ai::ScoredCandidate>> {
            self.0.lock().unwrap().push(hint.map(|h| h.to_string()));
            Ok((0..candidates.len())
                .map(|i| crate::ai::ScoredCandidate { index: i, score: 50 })
                .collect())
        }
        async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
        async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
    }

    #[tokio::test]
    async fn curate_channels_inner_applies_the_right_hint_per_source_group() {
        let db = migrated_db();
        seed_source_settings(&db, "src_a", true, Some("hint A"));
        seed_source_settings(&db, "src_b", true, Some("hint B"));
        let channels = vec![channel("src_a", "A1"), channel("src_b", "B1"), channel("src_a", "A2")];
        let provider = HintRecordingProvider(std::sync::Mutex::new(Vec::new()));
        let http = reqwest::Client::new();

        let result = curate_channels_inner(&db, &http, Some(&provider), channels).await.unwrap();

        assert_eq!(result.len(), 3, "no debe perder canales al agrupar/desagrupar");
        let mut hints_seen = provider.0.into_inner().unwrap();
        hints_seen.sort();
        assert_eq!(
            hints_seen,
            vec![Some("hint A".to_string()), Some("hint B".to_string())],
            "cada grupo de source_id debe curarse con su propio hint, no mezclado"
        );
    }

    #[tokio::test]
    async fn curate_channels_inner_skips_disabled_sources_without_calling_provider() {
        let db = migrated_db();
        seed_source_settings(&db, "src_off", false, Some("no debería usarse"));
        let channels = vec![channel("src_off", "X1")];
        let provider = HintRecordingProvider(std::sync::Mutex::new(Vec::new()));
        let http = reqwest::Client::new();

        let result = curate_channels_inner(&db, &http, Some(&provider), channels.clone())
            .await
            .unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "X1");
        assert!(
            provider.0.into_inner().unwrap().is_empty(),
            "curation_enabled=0 no debe llamar al proveedor"
        );
    }

    #[tokio::test]
    async fn curate_channels_inner_returns_channels_unchanged_without_a_provider() {
        let db = migrated_db();
        let channels = vec![channel("src_a", "A1")];
        let http = reqwest::Client::new();

        let result = curate_channels_inner(&db, &http, None, channels.clone()).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "A1");
    }

    #[tokio::test]
    async fn curate_channels_inner_drops_channels_over_the_ping_failure_threshold() {
        let db = migrated_db();
        let ch = channel("src_a", "Dead");
        // Ya venía con 5 fallas consecutivas de sesiones anteriores.
        crate::availability_ping::record_ping_results(&db, "src_a", &vec![(ch.url.clone(), false); 5]).unwrap();

        let http = reqwest::Client::new();
        let result = curate_channels_inner(&db, &http, None, vec![ch]).await.unwrap();

        assert!(result.is_empty(), "un canal con 5 fallas de ping consecutivas debe quedar afuera del grid");
    }

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
