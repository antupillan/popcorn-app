use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use librqbit::api::TorrentIdOrHash;
use librqbit::Session;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use tower_http::cors::CorsLayer;

use std::collections::HashMap;
use std::sync::Mutex;

/// Token opaco -> URL de fallback HTTP registrada (ver
/// `TorrentEngine::register_http_fallback`). Solo `EmbeddedRqbit` la usa
/// hoy; motores externos la dejan vacía (su cliente ya resuelve sus propias
/// fuentes).
pub type HttpFallbackMap = Arc<Mutex<HashMap<String, String>>>;
/// Token opaco -> archivo real en disco (biblioteca Local, o archivo de un
/// torrent ya descargado/en descarga por un motor externo — ver
/// `TorrentEngine::local_stream_url`/`stream_url`). Nunca se expone el path
/// crudo en la URL que llega al frontend.
pub type LocalFileMap = Arc<Mutex<HashMap<String, LocalFileEntry>>>;

#[derive(Clone)]
pub struct LocalFileEntry {
    pub path: std::path::PathBuf,
    /// `None` = comportamiento actual exacto (Biblioteca Local, archivo ya
    /// completo, sin poll). `Some` = el archivo puede tener bytes finales
    /// (piezas) todavía sin escribir más allá del punto que devuelve el
    /// probe — ver `ExternalQbittorrent::stream_url`.
    pub probe: Option<Arc<dyn ReadinessProbe>>,
}

/// Cuánto de un archivo es seguro leer desde el byte 0 sin tocar datos que
/// el swarm todavía no escribió a disco — implementado por motores externos
/// sin piece-awareness nativa (ver `QbitFileProbe`). `EmbeddedRqbit` no lo
/// necesita: su `/stream/*` ya espera cada pieza vía `handle.stream()`.
#[async_trait::async_trait]
pub trait ReadinessProbe: Send + Sync {
    async fn ready_bytes(&self) -> u64;
}

#[derive(Clone)]
struct AppState {
    // `None` cuando el motor activo no es `EmbeddedRqbit` — el servidor se
    // levanta una sola vez en `lib.rs::setup` independiente de qué motor
    // esté activo, así que no siempre hay una sesión de librqbit real.
    session: Option<Arc<Session>>,
    http_fallback: HttpFallbackMap,
    local_files: LocalFileMap,
    http_client: reqwest::Client,
}

/// Minimal local HTTP server so the frontend `<video>` tag can request byte
/// ranges from a torrent that's still downloading — librqbit's FileStream
/// blocks reads until the relevant piece has arrived, so this is real
/// progressive playback, not a pre-buffered file. Also proxies to a direct
/// HTTP source when one was registered as a fallback (see
/// TorrentEngine::register_http_fallback) — same Range semantics either way,
/// so the frontend never needs to know which path served a given torrent.
/// Se levanta una sola vez desde `lib.rs::setup`, independiente del motor
/// activo — `/stream/*` (P2P en vivo) solo funciona con `session: Some(_)`
/// (EmbeddedRqbit); `/local/*` y `/proxy/*` no dependen de librqbit.
pub async fn spawn(
    session: Option<Arc<Session>>,
    http_fallback: HttpFallbackMap,
    local_files: LocalFileMap,
) -> anyhow::Result<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let state = AppState {
        session,
        http_fallback,
        local_files,
        http_client: reqwest::Client::new(),
    };

    let app = Router::new()
        .route("/stream/{torrent_id}/{file_idx}", get(stream_handler))
        .route("/proxy/{torrent_id}", get(proxy_handler))
        .route("/local/{token}", get(local_stream_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[popcorn] stream server crashed: {e}");
        }
    });

    Ok(port)
}

struct ByteRange {
    start: u64,
    end: Option<u64>,
}

fn parse_range(headers: &HeaderMap) -> Option<ByteRange> {
    let raw = headers.get(axum::http::header::RANGE)?.to_str().ok()?;
    let spec = raw.strip_prefix("bytes=")?;
    let (start_s, end_s) = spec.split_once('-')?;
    let start = start_s.parse().ok()?;
    let end = if end_s.is_empty() {
        None
    } else {
        end_s.parse().ok()
    };
    Some(ByteRange { start, end })
}

/// Resuelve el rango efectivo a servir dado un `total` conocido — compartido
/// entre `stream_handler` (torrents) y `local_stream_handler` (archivos
/// locales), misma semántica de Range para ambos. `Err` ya trae la
/// respuesta 416 lista para devolver tal cual.
fn resolve_range(headers: &HeaderMap, total: u64) -> Result<(u64, u64, StatusCode), Response> {
    let range = parse_range(headers);
    let (start, end, status) = match range {
        Some(r) => (r.start, r.end.unwrap_or(total.saturating_sub(1)), StatusCode::PARTIAL_CONTENT),
        None => (0, total.saturating_sub(1), StatusCode::OK),
    };
    if start >= total || end < start {
        return Err((StatusCode::RANGE_NOT_SATISFIABLE, "invalid range").into_response());
    }
    Ok((start, end, status))
}

/// Adivina el MIME real por extensión del nombre del torrent — sin esto,
/// WebKitGTK recibe `application/octet-stream` y directamente no intenta
/// reproducir contenedores que no reconoce de entrada (Matroska/.mkv,
/// confirmado en vivo: MEDIA_ERR_SRC_NOT_SUPPORTED pese a que el sistema sí
/// tiene el demuxer de GStreamer instalado — WebKit decide si intentar
/// según el Content-Type declarado, no solo el contenido). Prueba barata
/// antes de encarar remux real: si esto alcanza, no hace falta.
/// Limitación honesta: `handle.name()` es el nombre del torrent, no del
/// archivo específico en `file_idx` — correcto para el caso común de un
/// solo archivo de video, puede errar en un torrent multi-archivo.
fn guess_content_type(torrent_name: Option<&str>) -> &'static str {
    let lower = torrent_name.unwrap_or_default().to_lowercase();
    if lower.ends_with(".mkv") {
        "video/x-matroska"
    } else if lower.ends_with(".mp4") || lower.ends_with(".m4v") {
        "video/mp4"
    } else if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".avi") {
        "video/x-msvideo"
    } else {
        "application/octet-stream"
    }
}

async fn stream_handler(
    State(state): State<AppState>,
    Path((torrent_id, file_idx)): Path<(usize, usize)>,
    headers: HeaderMap,
) -> Response {
    let Some(session) = state.session.as_ref() else {
        eprintln!("[popcorn] stream_handler 404: motor embebido no activo, sin sesión de librqbit");
        return (StatusCode::NOT_FOUND, "motor embebido no activo").into_response();
    };
    let Some(handle) = session.get(TorrentIdOrHash::Id(torrent_id)) else {
        eprintln!("[popcorn] stream_handler 404: torrent={torrent_id} no encontrado en la sesión");
        return (StatusCode::NOT_FOUND, "torrent not found").into_response();
    };
    // handle.stream() abajo consume el Arc (toma self por valor, no &self)
    // — el nombre hay que sacarlo antes de perder acceso a handle.
    let content_type = guess_content_type(handle.name().as_deref());

    let mut file_stream = match handle.stream(file_idx) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[popcorn] stream_handler 404: torrent={torrent_id} file={file_idx} error={e}");
            return (StatusCode::NOT_FOUND, e.to_string()).into_response();
        }
    };

    let total = file_stream.len();
    let (start, end, status) = match resolve_range(&headers, total) {
        Ok(v) => v,
        Err(resp) => {
            eprintln!(
                "[popcorn] stream_handler 416: torrent={torrent_id} file={file_idx} total={total}"
            );
            return resp;
        }
    };

    if let Err(e) = file_stream.seek(std::io::SeekFrom::Start(start)).await {
        eprintln!("[popcorn] stream_handler 500: torrent={torrent_id} file={file_idx} seek_error={e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    let content_length = end - start + 1;
    let limited = AsyncReadExt::take(file_stream, content_length);
    let body = Body::from_stream(ReaderStream::new(limited));

    let mut response = Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .header(axum::http::header::ACCEPT_RANGES, "bytes")
        .header(axum::http::header::CONTENT_LENGTH, content_length);

    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            axum::http::header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        );
    }

    response.body(body).unwrap().into_response()
}

/// Sirve un torrent vía HTTP directo en vez de P2P (ver
/// TorrentEngine::register_http_fallback) — reenvía el header Range tal
/// cual al origen y devuelve su respuesta (200/206/416) sin modificarla,
/// más allá de streamear el cuerpo en vez de bufferizarlo entero.
async fn proxy_handler(
    State(state): State<AppState>,
    Path(torrent_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(url) = state.http_fallback.lock().unwrap().get(&torrent_id).cloned() else {
        eprintln!("[popcorn] proxy_handler 404: torrent={torrent_id} sin fallback HTTP registrado");
        return (StatusCode::NOT_FOUND, "no hay fallback HTTP registrado para este torrent")
            .into_response();
    };

    let range_header = headers
        .get(axum::http::header::RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("(sin Range)")
        .to_string();

    // Reintento con backoff ante 5xx/error de conexión — evidencia real
    // (bug #18, ver plan): el datanode de archive.org devuelve 500
    // intermitente en ~50% de los requests, sin relación con el rango
    // pedido (confirmado repitiendo el mismo Range exacto: 206/500/206/500
    // en la misma corrida). No es un bug de este proxy ni del parseo de
    // Range — es inestabilidad transitoria del origen; ver `http_retry`
    // (aplicado también al resto de llamadas HTTP salientes de la app).
    let upstream = crate::http_retry::send_with_retry(|| {
        let mut req = state.http_client.get(&url);
        if let Some(range) = headers.get(axum::http::header::RANGE) {
            req = req.header(axum::http::header::RANGE, range.clone());
        }
        req
    })
    .await;

    let upstream = match upstream {
        Ok(r) => r,
        Err(e) => {
            eprintln!(
                "[popcorn] proxy_handler 502: torrent={torrent_id} url={url} range={range_header} error={e}"
            );
            return (StatusCode::BAD_GATEWAY, e.to_string()).into_response();
        }
    };

    let status = upstream.status();
    // Loguear también los errores que el origen devuelve pero que igual se
    // reenvían tal cual (4xx no reintentable, o 5xx que persistió tras
    // agotar los reintentos) — sin esto quedan invisibles del lado del
    // servidor y solo se ven como fallo genérico en el <video>.
    if status.is_client_error() || status.is_server_error() {
        eprintln!(
            "[popcorn] proxy_handler upstream error: torrent={torrent_id} url={url} range={range_header} status={status}"
        );
    }
    let mut response = Response::builder().status(status);
    for header in [
        axum::http::header::CONTENT_TYPE,
        axum::http::header::CONTENT_LENGTH,
        axum::http::header::CONTENT_RANGE,
        axum::http::header::ACCEPT_RANGES,
    ] {
        if let Some(v) = upstream.headers().get(&header) {
            response = response.header(header, v.clone());
        }
    }

    let body = Body::from_stream(upstream.bytes_stream());
    response.body(body).unwrap().into_response()
}

/// Tope de espera por datos todavía no llegados del swarm externo (ver
/// `ReadinessProbe`) y cada cuánto se reconsulta mientras se espera — mismo
/// espíritu que el bloqueo de `handle.stream()` en `EmbeddedRqbit`, pero con
/// timeout explícito porque acá no hay garantía de que el swarm externo
/// vaya a entregar la pieza (a diferencia del proceso propio).
const READINESS_POLL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const READINESS_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(750);

/// Sirve un archivo de la biblioteca Local con soporte de Range, misma
/// semántica que `stream_handler` pero leyendo de `tokio::fs::File` en vez
/// de un handle de librqbit. `token` es opaco (ver `LocalFileMap`) — nunca
/// se expone el path real en la URL que llega al frontend. Si el registro
/// trae un `probe` (motor externo sirviendo un archivo aún en descarga),
/// espera a que el rango pedido esté realmente escrito antes de leerlo —
/// sin esto, leer piezas rarest-first como si fueran contiguas produce
/// bytes corruptos (bug real confirmado en vivo, ver plan de este cambio).
async fn local_stream_handler(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(entry) = state.local_files.lock().unwrap().get(&token).cloned() else {
        eprintln!("[popcorn] local_stream_handler 404: token={token} sin archivo registrado");
        return (StatusCode::NOT_FOUND, "archivo local no encontrado").into_response();
    };
    let path = entry.path;

    let mut file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[popcorn] local_stream_handler 500: path={} error={e}", path.display());
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let total = match file.metadata().await {
        Ok(m) => m.len(),
        Err(e) => {
            eprintln!("[popcorn] local_stream_handler 500: path={} metadata_error={e}", path.display());
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let (start, mut end, mut status) = match resolve_range(&headers, total) {
        Ok(v) => v,
        Err(resp) => {
            eprintln!("[popcorn] local_stream_handler 416: path={} total={total}", path.display());
            return resp;
        }
    };

    if let Some(probe) = entry.probe.as_ref() {
        let deadline = tokio::time::Instant::now() + READINESS_POLL_TIMEOUT;
        loop {
            let ready = probe.ready_bytes().await;
            if ready > end {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                if ready <= start {
                    eprintln!(
                        "[popcorn] local_stream_handler 503: path={} token={token} swarm no entregó \
                         datos suficientes (ready={ready} start={start})",
                        path.display()
                    );
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "todavía no llegan suficientes datos del swarm — puede estar sin peers",
                    )
                        .into_response();
                }
                // Servir lo que haya, nunca más allá de `ready_bytes` (evita
                // el bug original de leer huecos rarest-first como si fueran
                // contiguos) — status pasa a 206 aunque el pedido original
                // fuera sin Range, porque el cuerpo servido es menor que
                // `total` y hace falta declarar el rango real vía
                // Content-Range.
                end = ready - 1;
                status = StatusCode::PARTIAL_CONTENT;
                break;
            }
            tokio::time::sleep(READINESS_POLL_INTERVAL).await;
        }
    }

    if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
        eprintln!("[popcorn] local_stream_handler 500: path={} seek_error={e}", path.display());
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    // A diferencia de stream_handler (que solo tiene el nombre del
    // torrent, no del archivo específico), acá el path real ya apunta al
    // archivo exacto — extensión confiable al 100%, no una adivinanza.
    let content_type = guess_content_type(path.file_name().and_then(|n| n.to_str()));
    let content_length = end - start + 1;
    let limited = AsyncReadExt::take(file, content_length);
    let body = Body::from_stream(ReaderStream::new(limited));

    let mut response = Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, content_type)
        .header(axum::http::header::ACCEPT_RANGES, "bytes")
        .header(axum::http::header::CONTENT_LENGTH, content_length);

    if status == StatusCode::PARTIAL_CONTENT {
        response = response.header(
            axum::http::header::CONTENT_RANGE,
            format!("bytes {start}-{end}/{total}"),
        );
    }

    response.body(body).unwrap().into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with_range(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(axum::http::header::RANGE, value.parse().unwrap());
        h
    }

    #[test]
    fn parses_bounded_range() {
        let r = parse_range(&headers_with_range("bytes=0-1023")).unwrap();
        assert_eq!(r.start, 0);
        assert_eq!(r.end, Some(1023));
    }

    #[test]
    fn parses_open_ended_range() {
        let r = parse_range(&headers_with_range("bytes=500-")).unwrap();
        assert_eq!(r.start, 500);
        assert_eq!(r.end, None);
    }

    #[test]
    fn rejects_malformed_range() {
        assert!(parse_range(&headers_with_range("not-a-range")).is_none());
    }

    #[test]
    fn no_range_header_returns_none() {
        assert!(parse_range(&HeaderMap::new()).is_none());
    }
}
