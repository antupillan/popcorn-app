use std::time::Duration;

/// No configurable por el usuario: es resiliencia interna contra
/// inestabilidad transitoria de orígenes externos (archive.org, indexers
/// BYO, proveedores de IA), no una preferencia que varíe entre
/// usuarios/entornos. 5 intentos, no 3: medido en vivo contra el datanode
/// real de archive.org, la tasa de fallo por request ronda 50% — con 3
/// intentos, 3 fallos seguidos (12.5% de probabilidad) ya se observó en un
/// test real (ver bug #18 en el plan). 5 intentos baja esa probabilidad a
/// ~3%, manteniendo la latencia añadida acotada (peor caso: ~1.2s extra).
const MAX_ATTEMPTS: u32 = 5;
const RETRY_BACKOFF: Duration = Duration::from_millis(300);

/// Reintenta una request HTTP saliente ante 5xx o error de conexión/timeout
/// del origen. Motivado por evidencia real, no hipotética (bug #18, ver
/// plan): archive.org confirmó devolver 500 intermitente en ~50% de los
/// requests, reproducido repitiendo el mismo Range exacto contra el mismo
/// datanode (206/500/206/500 en la misma corrida) — no depende del
/// contenido pedido, es inestabilidad del lado del origen. Aplica al
/// ecosistema completo de llamadas salientes (archive.org, indexers BYO,
/// proveedores de IA), no solo al proxy de streaming donde se encontró.
///
/// `build` reconstruye la request en cada intento porque
/// `reqwest::RequestBuilder` no implementa `Clone`. Solo apto para
/// requests de solo lectura/idempotentes (GET, o POST cuyo body no cambia
/// entre intentos) — todos los llamadores actuales lo son.
pub async fn send_with_retry<F>(mut build: F) -> Result<reqwest::Response, reqwest::Error>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let mut last_err = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match build().send().await {
            Ok(r) if r.status().is_server_error() && attempt < MAX_ATTEMPTS => {
                eprintln!(
                    "[popcorn] http retry {attempt}/{MAX_ATTEMPTS}: url={} status={}",
                    r.url(),
                    r.status()
                );
                tokio::time::sleep(RETRY_BACKOFF).await;
            }
            Ok(r) => return Ok(r),
            Err(e) if attempt < MAX_ATTEMPTS => {
                eprintln!("[popcorn] http retry {attempt}/{MAX_ATTEMPTS}: error={e}");
                tokio::time::sleep(RETRY_BACKOFF).await;
                last_err = Some(e);
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    Err(last_err.expect("el loop siempre setea last_err antes de agotar los intentos"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, routing::get, Router};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// Servidor local real (no mock a nivel de tipos) que falla las primeras
    /// `fail_times` requests con 500 y luego responde 200 — imita el
    /// comportamiento observado del datanode de archive.org lo bastante
    /// fiel como para probar el helper de retry de punta a punta.
    async fn spawn_flaky_server(fail_times: u32) -> (u16, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_for_state = calls.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new()
            .route(
                "/",
                get(move |State(calls): State<Arc<AtomicU32>>| async move {
                    let n = calls.fetch_add(1, Ordering::SeqCst) + 1;
                    if n <= fail_times {
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR
                    } else {
                        axum::http::StatusCode::OK
                    }
                }),
            )
            .with_state(calls_for_state);
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (port, calls)
    }

    #[tokio::test]
    async fn succeeds_after_retrying_past_transient_5xx() {
        let (port, calls) = spawn_flaky_server(2).await;
        let client = reqwest::Client::new();
        let result = send_with_retry(|| client.get(format!("http://127.0.0.1:{port}/")))
            .await
            .unwrap();
        assert_eq!(result.status(), 200);
        assert_eq!(calls.load(Ordering::SeqCst), 3, "debe reintentar 2 veces y acertar en el 3er intento");
    }

    #[tokio::test]
    async fn gives_up_and_forwards_error_status_after_max_attempts() {
        let (port, calls) = spawn_flaky_server(u32::MAX).await;
        let client = reqwest::Client::new();
        let result = send_with_retry(|| client.get(format!("http://127.0.0.1:{port}/")))
            .await
            .unwrap();
        assert_eq!(result.status(), 500, "tras agotar los intentos, reenvía el último status tal cual");
        assert_eq!(calls.load(Ordering::SeqCst), MAX_ATTEMPTS);
    }

    #[tokio::test]
    async fn stops_retrying_after_max_attempts_on_connection_error() {
        let client = reqwest::Client::new();
        let calls = AtomicU32::new(0);
        let result = send_with_retry(|| {
            calls.fetch_add(1, Ordering::SeqCst);
            client.get("http://127.0.0.1:1") // puerto sin listener: error de conexión inmediato
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), MAX_ATTEMPTS);
    }
}
