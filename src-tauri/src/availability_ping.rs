// Ping de disponibilidad para el caché de curación (ver ai/curation.rs y
// el plan "Caché de curación IA con puntaje absoluto + ping de
// disponibilidad") — corre en el paso de fondo de Online/IPTV, nunca en
// el fetch rápido, e incrementa/resetea consecutive_ping_failures en
// curation_cache. Independiente de curation_enabled: un ítem allowlisted
// a mano (ej. Blender Foundation) igual puede desaparecer con el tiempo.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::db::Db;

const PING_CONCURRENCY: usize = 16;
pub(crate) const CONSECUTIVE_FAILURES_THRESHOLD: i64 = 5;

/// Reusa `http_retry::send_with_retry` (5 intentos) en vez de un único
/// intento — verificado en vivo (Mandato 1, no hipótesis): archive.org
/// devuelve ~50% de fallo intermitente por request (mismo comportamiento
/// que documenta `http_retry.rs`, bug #18), confirmado mandando 16 pings
/// reales en paralelo contra `.torrent` de archive.org: 1/16 con 500 y
/// 1/16 con timeout total, sobre ítems que sí existen. Con un solo
/// intento, esa tasa de fallo base — no la disponibilidad real del
/// ítem — es lo que terminaba acumulando `consecutive_ping_failures`
/// hasta cruzar el umbral y ocultar ítems sanos del grid. Alcanza con el
/// status code: usado por la familia archive.org y `public_domain_torrents`,
/// donde "disponible" significa que el `.torrent` responde, no hace falta
/// interpretar el cuerpo (a diferencia de IPTV, ver `iptv::probe_manifest`).
pub(crate) async fn ping_ok(client: &reqwest::Client, url: &str) -> bool {
    matches!(
        crate::http_retry::send_with_retry(|| client.get(url)).await,
        Ok(resp) if resp.status().is_success()
    )
}

/// Semáforo para acotar cuántos pings corren a la vez — `pub(crate)` para
/// que el llamador pueda compartir UNO SOLO entre varias tandas de
/// `ping_many` que corren en paralelo entre sí (ver
/// `online_library::curate_online_items_inner`: sin esto, cuatro familias
/// corriendo concurrentes vía `tokio::join!` terminaban usando cada una su
/// propio límite de `PING_CONCURRENCY`, llegando a 4x esa cifra en
/// conexiones simultáneas reales contra archive.org — verificado en vivo
/// que eso alcanza para gatillar algún throttling del lado del origen que
/// ni los 5 reintentos de `ping_ok` superan, Mandato 1, no hipótesis).
pub(crate) fn new_ping_semaphore() -> Arc<Semaphore> {
    Arc::new(Semaphore::new(PING_CONCURRENCY))
}

/// Corre `check` sobre varios `(item_key, target)` en paralelo, acotado
/// por `semaphore` para no saturar al origen ni la propia conexión — una
/// sesión de Online puede traer ~150-200 ítems entre las tres colecciones
/// de archive.org (`rows=50` cada una) más Public Domain Torrents. Recibe
/// el semáforo por parámetro (no lo crea acá) para que el llamador pueda
/// compartir uno solo entre varias tandas concurrentes, ver
/// `new_ping_semaphore`.
pub(crate) async fn ping_many<F, Fut>(
    targets: Vec<(String, String)>,
    semaphore: Arc<Semaphore>,
    check: F,
) -> Vec<(String, bool)>
where
    F: Fn(String) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = bool> + Send + 'static,
{
    let mut set = tokio::task::JoinSet::new();
    for (key, target) in targets {
        let sem = semaphore.clone();
        let check = check.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.expect("el semáforo no se cierra nunca");
            let ok = check(target).await;
            (key, ok)
        });
    }
    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok(pair) = res {
            results.push(pair);
        }
    }
    results
}

/// Upsert de resultados de ping: éxito resetea `consecutive_ping_failures`
/// a 0, falla lo incrementa. No toca `included`/`score`/`hint_used`
/// (columnas de la curación IA, ver `ai::curation`) — al no mencionarlas,
/// `ON CONFLICT` las deja como estaban.
pub(crate) fn record_ping_results(db: &Db, source_id: &str, results: &[(String, bool)]) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    for (item_key, ok) in results {
        if *ok {
            conn.execute(
                "INSERT INTO curation_cache (source_id, item_key, consecutive_ping_failures, last_ping_at)
                 VALUES (?1, ?2, 0, datetime('now'))
                 ON CONFLICT(source_id, item_key) DO UPDATE SET
                    consecutive_ping_failures = 0, last_ping_at = datetime('now')",
                rusqlite::params![source_id, item_key],
            )
            .map_err(|e| e.to_string())?;
        } else {
            conn.execute(
                "INSERT INTO curation_cache (source_id, item_key, consecutive_ping_failures, last_ping_at)
                 VALUES (?1, ?2, 1, datetime('now'))
                 ON CONFLICT(source_id, item_key) DO UPDATE SET
                    consecutive_ping_failures = consecutive_ping_failures + 1, last_ping_at = datetime('now')",
                rusqlite::params![source_id, item_key],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Lee `consecutive_ping_failures` actuales para filtrar el grid — se
/// llama después de `record_ping_results`, mismo `source_id`.
pub(crate) fn ping_failure_counts(db: &Db, source_id: &str) -> Result<HashMap<String, i64>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT item_key, consecutive_ping_failures FROM curation_cache WHERE source_id = ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([source_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn migrated_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(Mutex::new(conn))
    }

    #[tokio::test]
    async fn ping_ok_true_on_success_false_on_error_status() {
        let mut server = mockito::Server::new_async().await;
        let ok_mock = server.mock("GET", "/ok").with_status(200).create_async().await;
        // 500 dispara los 5 intentos de http_retry (ver ping_ok) — el mock
        // debe aceptar esa cantidad de llamadas, no solo una.
        let fail_mock = server.mock("GET", "/fail").with_status(500).expect(5).create_async().await;

        let client = reqwest::Client::new();
        assert!(ping_ok(&client, &format!("{}/ok", server.url())).await);
        assert!(!ping_ok(&client, &format!("{}/fail", server.url())).await);

        ok_mock.assert_async().await;
        fail_mock.assert_async().await;
    }

    #[test]
    fn record_ping_results_increments_on_failure_and_resets_on_success() {
        let db = migrated_db();
        record_ping_results(&db, "src", &[("a".to_string(), false)]).unwrap();
        record_ping_results(&db, "src", &[("a".to_string(), false)]).unwrap();
        assert_eq!(ping_failure_counts(&db, "src").unwrap().get("a"), Some(&2));

        record_ping_results(&db, "src", &[("a".to_string(), true)]).unwrap();
        assert_eq!(ping_failure_counts(&db, "src").unwrap().get("a"), Some(&0));
    }

    #[test]
    fn record_ping_results_reaches_threshold_after_enough_failures() {
        let db = migrated_db();
        for _ in 0..CONSECUTIVE_FAILURES_THRESHOLD {
            record_ping_results(&db, "src", &[("dead".to_string(), false)]).unwrap();
        }
        let counts = ping_failure_counts(&db, "src").unwrap();
        assert!(counts["dead"] >= CONSECUTIVE_FAILURES_THRESHOLD);
    }

    #[test]
    fn record_ping_results_does_not_touch_ai_curation_columns() {
        let db = migrated_db();
        {
            let conn = db.0.lock().unwrap();
            conn.execute(
                "INSERT INTO curation_cache (source_id, item_key, included, score, hint_used) VALUES ('src', 'a', 1, 77, 'crit')",
                [],
            )
            .unwrap();
        }
        record_ping_results(&db, "src", &[("a".to_string(), false)]).unwrap();
        let conn = db.0.lock().unwrap();
        let (included, score): (i64, Option<i64>) = conn
            .query_row("SELECT included, score FROM curation_cache WHERE source_id='src' AND item_key='a'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(included, 1);
        assert_eq!(score, Some(77));
    }

    #[tokio::test]
    async fn ping_many_runs_all_targets_and_reports_per_key_result() {
        let mut server = mockito::Server::new_async().await;
        let ok_mock = server.mock("GET", "/a").with_status(200).create_async().await;
        let fail_mock = server.mock("GET", "/b").with_status(500).expect(5).create_async().await;
        let base = server.url();

        let targets = vec![
            ("a".to_string(), format!("{base}/a")),
            ("b".to_string(), format!("{base}/b")),
        ];
        let results = ping_many(targets, new_ping_semaphore(), move |url| {
            let client = reqwest::Client::new();
            async move { ping_ok(&client, &url).await }
        })
        .await;

        let map: HashMap<String, bool> = results.into_iter().collect();
        assert_eq!(map.get("a"), Some(&true));
        assert_eq!(map.get("b"), Some(&false));
        ok_mock.assert_async().await;
        fail_mock.assert_async().await;
    }
}
