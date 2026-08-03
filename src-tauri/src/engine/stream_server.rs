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

/// Minimal local HTTP server so the frontend `<video>` tag can request byte
/// ranges from a torrent that's still downloading — librqbit's FileStream
/// blocks reads until the relevant piece has arrived, so this is real
/// progressive playback, not a pre-buffered file.
pub async fn spawn(session: Arc<Session>) -> anyhow::Result<u16> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    let app = Router::new()
        .route("/stream/{torrent_id}/{file_idx}", get(stream_handler))
        .with_state(session);

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

async fn stream_handler(
    State(session): State<Arc<Session>>,
    Path((torrent_id, file_idx)): Path<(usize, usize)>,
    headers: HeaderMap,
) -> Response {
    let Some(handle) = session.get(TorrentIdOrHash::Id(torrent_id)) else {
        return (StatusCode::NOT_FOUND, "torrent not found").into_response();
    };

    let mut file_stream = match handle.stream(file_idx) {
        Ok(s) => s,
        Err(e) => return (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    };

    let total = file_stream.len();
    let range = parse_range(&headers);

    let (start, end, status) = match range {
        Some(r) => (r.start, r.end.unwrap_or(total.saturating_sub(1)), StatusCode::PARTIAL_CONTENT),
        None => (0, total.saturating_sub(1), StatusCode::OK),
    };

    if start >= total || end < start {
        return (StatusCode::RANGE_NOT_SATISFIABLE, "invalid range").into_response();
    }

    if let Err(e) = file_stream.seek(std::io::SeekFrom::Start(start)).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    let content_length = end - start + 1;
    let limited = AsyncReadExt::take(file_stream, content_length);
    let body = Body::from_stream(ReaderStream::new(limited));

    let mut response = Response::builder()
        .status(status)
        .header(axum::http::header::CONTENT_TYPE, "application/octet-stream")
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
