use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tokio::io::AsyncWriteExt;

use super::hls_playlist;
use crate::db::Db;

/// id de grabación -> flag de parada. En memoria únicamente — por diseño,
/// no sobrevive un reinicio (ver `reconcile_interrupted_recordings`).
#[derive(Default)]
pub struct RecorderState(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

#[derive(Serialize, Clone)]
pub struct RecordingInfo {
    pub id: String,
    pub source_id: Option<String>,
    pub channel_name: String,
    pub manifest_url: String,
    pub file_name: String,
    pub status: String, // "recording" | "stopped" | "error"
    pub error: Option<String>,
    pub bytes_written: i64,
    pub started_at: String,
    pub stopped_at: Option<String>,
}

fn row_to_recording(row: &rusqlite::Row) -> rusqlite::Result<RecordingInfo> {
    Ok(RecordingInfo {
        id: row.get(0)?,
        source_id: row.get(1)?,
        channel_name: row.get(2)?,
        manifest_url: row.get(3)?,
        file_name: row.get(4)?,
        status: row.get(5)?,
        error: row.get(6)?,
        bytes_written: row.get(7)?,
        started_at: row.get(8)?,
        stopped_at: row.get(9)?,
    })
}

/// app_data_dir/iptv/recordings — separado de playlists/ (ver
/// `super::playlist_file_path`), mismo criterio de directorio-no-blob.
fn recordings_dir(app: &AppHandle) -> anyhow::Result<std::path::PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("no se pudo resolver el directorio de datos de la app")?
        .join("iptv")
        .join("recordings");
    std::fs::create_dir_all(&dir).with_context(|| format!("no se pudo crear {}", dir.display()))?;
    Ok(dir)
}

/// Al arrancar la app, `RecorderState` siempre empieza vacío — cualquier
/// fila `status='recording'` de una corrida anterior quedó huérfana por
/// definición (el proceso que la manejaba ya no existe). Sin intento de
/// auto-resumir: una lista en vivo ya rotó, resumir a ciegas arriesga un
/// archivo corrupto disfrazado de completo (Mandato 4).
pub fn reconcile_interrupted_recordings(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE iptv_recordings SET status = 'stopped', error = 'interrumpida por reinicio de la app', stopped_at = datetime('now') \
         WHERE status = 'recording'",
        [],
    )?;
    Ok(())
}

#[tauri::command]
pub async fn list_recordings(db: State<'_, Db>) -> Result<Vec<RecordingInfo>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, source_id, channel_name, manifest_url, file_name, status, error, bytes_written, started_at, stopped_at \
             FROM iptv_recordings ORDER BY started_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_recording).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Borra la fila y el archivo `.ts` real — rechaza explícito si sigue en
/// curso (`status = 'recording'`) en vez de arriesgar borrar un archivo
/// que el loop de grabación todavía está escribiendo.
#[tauri::command]
pub async fn delete_recording(app: AppHandle, db: State<'_, Db>, id: String) -> Result<(), String> {
    let (status, file_name): (String, String) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT status, file_name FROM iptv_recordings WHERE id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?
    };
    if status == "recording" {
        return Err("la grabación sigue en curso — detenela antes de quitarla".to_string());
    }

    let path = recordings_dir(&app).map_err(|e| e.to_string())?.join(&file_name);
    if let Err(e) = tokio::fs::remove_file(&path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("no se pudo borrar {}: {e}", path.display()));
        }
    }

    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM iptv_recordings WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Valida el manifest una vez arriba (rechaza playlist maestra/cifrado/
/// byte-range con error explícito, nunca arranca una grabación que sabemos
/// que va a quedar corrupta o incompleta — Mandato 4), inserta la fila y
/// lanza el loop de descarga en segundo plano. Fire-and-forget: devuelve de
/// inmediato, el loop actualiza `bytes_written`/`status` por su cuenta.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn start_recording(
    app: AppHandle,
    http: State<'_, crate::commands::HttpClient>,
    db: State<'_, Db>,
    recorder: State<'_, RecorderState>,
    source_id: Option<String>,
    channel_name: String,
    manifest_url: String,
    max_duration_minutes: Option<u32>,
) -> Result<RecordingInfo, String> {
    let resp = crate::http_retry::send_with_retry(|| http.0.get(&manifest_url))
        .await
        .map_err(|e| e.to_string())?;
    let base_url = resp.url().clone();
    let manifest_bytes = resp.bytes().await.map_err(|e| e.to_string())?;

    // Si es playlist maestra (multi-bitrate), resuelve la variante de
    // mejor calidad y graba esa de ahí en más — el loop de grabación no
    // elige variantes en cada poll, necesita una URL de media playlist
    // directa desde el arranque.
    let manifest_url = match hls_playlist::resolve_master_variant(&manifest_bytes, &base_url)
        .map_err(|e| e.to_string())?
    {
        Some(variant_url) => {
            let variant_bytes = crate::http_retry::send_with_retry(|| http.0.get(&variant_url))
                .await
                .map_err(|e| e.to_string())?
                .bytes()
                .await
                .map_err(|e| e.to_string())?;
            hls_playlist::parse_media_playlist(&variant_bytes).map_err(|e| e.to_string())?;
            variant_url
        }
        None => {
            hls_playlist::parse_media_playlist(&manifest_bytes).map_err(|e| e.to_string())?;
            manifest_url
        }
    };

    let id = uuid::Uuid::new_v4().to_string();
    let file_name = format!("{id}.ts");
    let started_at: String = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO iptv_recordings (id, source_id, channel_name, manifest_url, file_name, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, 'recording')",
            (&id, &source_id, &channel_name, &manifest_url, &file_name),
        )
        .map_err(|e| e.to_string())?;
        conn.query_row("SELECT started_at FROM iptv_recordings WHERE id = ?1", [&id], |r| r.get(0))
            .map_err(|e| e.to_string())?
    };

    let stop_flag = Arc::new(AtomicBool::new(false));
    {
        let mut map = recorder.0.lock().map_err(|e| e.to_string())?;
        map.insert(id.clone(), stop_flag.clone());
    }

    let task_app = app.clone();
    let task_client = http.0.clone();
    let task_id = id.clone();
    let task_manifest_url = manifest_url.clone();
    tokio::spawn(async move {
        recording_loop(task_app, task_client, task_id, task_manifest_url, stop_flag, max_duration_minutes).await;
    });

    Ok(RecordingInfo {
        id,
        source_id,
        channel_name,
        manifest_url,
        file_name,
        status: "recording".to_string(),
        error: None,
        bytes_written: 0,
        started_at,
        stopped_at: None,
    })
}

/// Solo señaliza — el propio loop hace la transición de estado final al
/// notar el flag, para que haya un único escritor de `status` (evita dos
/// caminos de código pisándose la fila). Corta en el siguiente punto seguro
/// (entre segmentos), no a mitad de un write.
#[tauri::command]
pub async fn stop_recording(recorder: State<'_, RecorderState>, id: String) -> Result<(), String> {
    let flag = {
        let map = recorder.0.lock().map_err(|e| e.to_string())?;
        map.get(&id).cloned()
    };
    match flag {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            Ok(())
        }
        None => Err(format!("no hay ninguna grabación en curso con id {id}")),
    }
}

enum PollOutcome {
    Continue { target_duration: u64 },
    Done,
    Stopped,
}

/// Un ciclo de poll: descarga el manifest, descarga los segmentos todavía
/// no vistos (`seen`, para no repetir en el próximo poll de una lista en
/// vivo que rota), y devuelve qué hacer después. `stop_flag` se chequea
/// antes de cada segmento — así una parada corta entre segmentos, nunca a
/// mitad de un write.
async fn run_one_poll(
    client: &reqwest::Client,
    manifest_url: &str,
    file: &mut tokio::fs::File,
    seen: &mut HashSet<String>,
    total_bytes: &mut i64,
    stop_flag: &AtomicBool,
) -> anyhow::Result<PollOutcome> {
    let manifest_resp = crate::http_retry::send_with_retry(|| client.get(manifest_url))
        .await
        .context("no se pudo obtener el manifest")?;
    let base_url = manifest_resp.url().clone();
    let body = manifest_resp.bytes().await.context("no se pudo leer el manifest")?;
    let playlist = hls_playlist::parse_media_playlist(&body)?;

    for seg in &playlist.segments {
        if stop_flag.load(Ordering::SeqCst) {
            return Ok(PollOutcome::Stopped);
        }
        if !seen.insert(seg.uri.clone()) {
            continue;
        }
        let seg_url = base_url
            .join(&seg.uri)
            .with_context(|| format!("URI de segmento inválida '{}'", seg.uri))?;
        let seg_bytes = crate::http_retry::send_with_retry(|| client.get(seg_url.clone()))
            .await
            .with_context(|| format!("no se pudo descargar el segmento {seg_url}"))?
            .bytes()
            .await
            .with_context(|| format!("no se pudo leer el segmento {seg_url}"))?;
        file.write_all(&seg_bytes)
            .await
            .with_context(|| format!("no se pudo escribir el segmento {seg_url}"))?;
        *total_bytes += seg_bytes.len() as i64;
    }
    file.flush().await.context("no se pudo flushear el archivo de grabación")?;

    if stop_flag.load(Ordering::SeqCst) {
        return Ok(PollOutcome::Stopped);
    }
    if playlist.end_list {
        return Ok(PollOutcome::Done);
    }
    Ok(PollOutcome::Continue { target_duration: playlist.target_duration })
}

enum RecordingEnd {
    Stopped,
    Error(String),
}

async fn finish(app: &AppHandle, id: &str, end: RecordingEnd) {
    let (status, error): (&str, Option<String>) = match end {
        RecordingEnd::Stopped => ("stopped", None),
        RecordingEnd::Error(msg) => ("error", Some(msg)),
    };
    let db = app.state::<Db>();
    let result = (|| -> rusqlite::Result<()> {
        let conn = db.0.lock().map_err(|_| rusqlite::Error::ExecuteReturnedResults)?;
        conn.execute(
            "UPDATE iptv_recordings SET status = ?1, error = ?2, stopped_at = datetime('now') WHERE id = ?3",
            (status, &error, id),
        )?;
        Ok(())
    })();
    if let Err(e) = result {
        eprintln!("[popcorn] no se pudo actualizar el estado final de la grabación {id}: {e}");
    }
    let recorder = app.state::<RecorderState>();
    if let Ok(mut map) = recorder.0.lock() {
        map.remove(id);
    };
}

fn update_bytes_written(app: &AppHandle, id: &str, bytes: i64) {
    let db = app.state::<Db>();
    let Ok(conn) = db.0.lock() else { return };
    if let Err(e) = conn.execute("UPDATE iptv_recordings SET bytes_written = ?1 WHERE id = ?2", (bytes, id)) {
        eprintln!("[popcorn] no se pudo actualizar bytes_written de la grabación {id}: {e}");
    }
}

async fn recording_loop(
    app: AppHandle,
    client: reqwest::Client,
    id: String,
    manifest_url: String,
    stop_flag: Arc<AtomicBool>,
    max_duration_minutes: Option<u32>,
) {
    // Tope de duración real (determinístico, no depende de que el usuario
    // mire la pantalla) — evita que una grabación quede corriendo para
    // siempre si nadie la para a mano.
    let deadline = max_duration_minutes
        .map(|m| std::time::Instant::now() + std::time::Duration::from_secs(m as u64 * 60));
    let file_path = match recordings_dir(&app) {
        Ok(dir) => dir.join(format!("{id}.ts")),
        Err(e) => return finish(&app, &id, RecordingEnd::Error(e.to_string())).await,
    };
    let mut file = match tokio::fs::OpenOptions::new().create(true).append(true).open(&file_path).await {
        Ok(f) => f,
        Err(e) => {
            return finish(
                &app,
                &id,
                RecordingEnd::Error(format!("no se pudo crear {}: {e}", file_path.display())),
            )
            .await
        }
    };

    let mut seen = HashSet::new();
    let mut total_bytes: i64 = 0;

    loop {
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            update_bytes_written(&app, &id, total_bytes);
            return finish(&app, &id, RecordingEnd::Stopped).await;
        }
        match run_one_poll(&client, &manifest_url, &mut file, &mut seen, &mut total_bytes, &stop_flag).await {
            Ok(PollOutcome::Continue { target_duration }) => {
                update_bytes_written(&app, &id, total_bytes);
                let backoff = std::time::Duration::from_secs(target_duration.clamp(1, 5));
                tokio::time::sleep(backoff).await;
            }
            Ok(PollOutcome::Done) => {
                update_bytes_written(&app, &id, total_bytes);
                return finish(&app, &id, RecordingEnd::Stopped).await;
            }
            Ok(PollOutcome::Stopped) => {
                update_bytes_written(&app, &id, total_bytes);
                return finish(&app, &id, RecordingEnd::Stopped).await;
            }
            Err(e) => {
                update_bytes_written(&app, &id, total_bytes);
                return finish(&app, &id, RecordingEnd::Error(e.to_string())).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use rusqlite::Connection;

    fn migrated_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn reconcile_marks_orphaned_recording_rows_stopped_with_explanation() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO iptv_recordings (id, channel_name, manifest_url, file_name, status) \
             VALUES ('r1', 'Canal Test', 'https://example.org/live.m3u8', 'r1.ts', 'recording')",
            [],
        )
        .unwrap();

        reconcile_interrupted_recordings(&conn).unwrap();

        let (status, error): (String, Option<String>) = conn
            .query_row("SELECT status, error FROM iptv_recordings WHERE id = 'r1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(status, "stopped");
        assert!(error.unwrap().contains("reinicio"));
    }

    #[test]
    fn reconcile_leaves_already_finished_rows_untouched() {
        let conn = migrated_conn();
        conn.execute(
            "INSERT INTO iptv_recordings (id, channel_name, manifest_url, file_name, status, error) \
             VALUES ('r1', 'Canal Test', 'https://example.org/live.m3u8', 'r1.ts', 'error', 'motivo original')",
            [],
        )
        .unwrap();

        reconcile_interrupted_recordings(&conn).unwrap();

        let error: Option<String> = conn
            .query_row("SELECT error FROM iptv_recordings WHERE id = 'r1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(error.as_deref(), Some("motivo original"), "no debe pisar una fila que ya no está en curso");
    }

    async fn spawn_hls_server(manifest_body: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new()
            .route("/live.m3u8", get(move || async move { manifest_body }))
            .route("/seg1.ts", get(|| async { "AAAA" }))
            .route("/seg2.ts", get(|| async { "BBBBBB" }));
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        port
    }

    const VOD_MANIFEST: &str = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXTINF:4.0,\nseg1.ts\n#EXTINF:6.0,\nseg2.ts\n#EXT-X-ENDLIST\n";
    const LIVE_MANIFEST: &str = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXTINF:4.0,\nseg1.ts\n#EXTINF:6.0,\nseg2.ts\n";

    #[tokio::test]
    async fn run_one_poll_downloads_relative_segments_and_reports_done_for_vod() {
        let port = spawn_hls_server(VOD_MANIFEST).await;
        let manifest_url = format!("http://127.0.0.1:{port}/live.m3u8");
        let client = reqwest::Client::new();

        let tmp = std::env::temp_dir().join(format!("popcorn-rec-test-{}.ts", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&tmp).await.unwrap();
        let mut seen = HashSet::new();
        let mut total_bytes: i64 = 0;
        let stop_flag = AtomicBool::new(false);

        let outcome = run_one_poll(&client, &manifest_url, &mut file, &mut seen, &mut total_bytes, &stop_flag)
            .await
            .unwrap();

        assert!(matches!(outcome, PollOutcome::Done), "playlist VOD con EXT-X-ENDLIST debe reportar Done");
        assert_eq!(total_bytes, 10, "\"AAAA\" (4) + \"BBBBBB\" (6) = 10 bytes reales escritos");
        assert_eq!(seen.len(), 2);

        let written = tokio::fs::read_to_string(&tmp).await.unwrap();
        assert_eq!(written, "AAAABBBBBB", "las URIs relativas del manifest deben resolverse contra su propia URL base y concatenarse en orden");

        tokio::fs::remove_file(&tmp).await.ok();
    }

    #[tokio::test]
    async fn run_one_poll_does_not_redownload_segments_already_seen() {
        let port = spawn_hls_server(LIVE_MANIFEST).await;
        let manifest_url = format!("http://127.0.0.1:{port}/live.m3u8");
        let client = reqwest::Client::new();

        let tmp = std::env::temp_dir().join(format!("popcorn-rec-test-{}.ts", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&tmp).await.unwrap();
        let mut seen = HashSet::new();
        let mut total_bytes: i64 = 0;
        let stop_flag = AtomicBool::new(false);

        run_one_poll(&client, &manifest_url, &mut file, &mut seen, &mut total_bytes, &stop_flag).await.unwrap();
        let after_first_poll = total_bytes;
        // Mismo manifest (misma lista de segmentos) en el segundo poll —
        // simula una playlist en vivo que todavía no rotó.
        run_one_poll(&client, &manifest_url, &mut file, &mut seen, &mut total_bytes, &stop_flag).await.unwrap();

        assert_eq!(total_bytes, after_first_poll, "no debe volver a descargar segmentos ya vistos");

        tokio::fs::remove_file(&tmp).await.ok();
    }

    #[tokio::test]
    async fn run_one_poll_stops_before_downloading_when_flag_already_set() {
        let port = spawn_hls_server(LIVE_MANIFEST).await;
        let manifest_url = format!("http://127.0.0.1:{port}/live.m3u8");
        let client = reqwest::Client::new();

        let tmp = std::env::temp_dir().join(format!("popcorn-rec-test-{}.ts", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&tmp).await.unwrap();
        let mut seen = HashSet::new();
        let mut total_bytes: i64 = 0;
        let stop_flag = AtomicBool::new(true);

        let outcome = run_one_poll(&client, &manifest_url, &mut file, &mut seen, &mut total_bytes, &stop_flag)
            .await
            .unwrap();

        assert!(matches!(outcome, PollOutcome::Stopped));
        assert_eq!(total_bytes, 0, "no debe descargar ningún segmento si la parada ya estaba señalizada");

        tokio::fs::remove_file(&tmp).await.ok();
    }

    #[tokio::test]
    #[ignore = "red real, no apto para CI por defecto — correr manualmente contra un canal público real"]
    async fn run_one_poll_downloads_real_segments_from_live_public_broadcaster() {
        // URL de variante real (no la maestra) de RTVE "24 Horas" HD, misma
        // fuente semilla iptv_org_public — confirmada en vivo: manifest sin
        // cifrado ni EXT-X-BYTERANGE, segmentos .ts reales con ruta relativa
        // por fecha/hora (formato real del stream, no inventado).
        let manifest_url = "http://185.47.212.25:8080/24h_HD/tracks-v1a1/mono.ts.m3u8";
        let client = reqwest::Client::new();

        let tmp = std::env::temp_dir().join(format!("popcorn-rec-real-test-{}.ts", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::OpenOptions::new().create(true).write(true).truncate(true).open(&tmp).await.unwrap();
        let mut seen = HashSet::new();
        let mut total_bytes: i64 = 0;
        let stop_flag = AtomicBool::new(false);

        let outcome = run_one_poll(&client, manifest_url, &mut file, &mut seen, &mut total_bytes, &stop_flag)
            .await
            .expect("el canal público real debe responder con una media playlist grabable");

        assert!(
            matches!(outcome, PollOutcome::Continue { .. }),
            "es un canal en vivo real, sin EXT-X-ENDLIST — debe seguir en Continue"
        );
        assert!(total_bytes > 0, "debe haber descargado bytes reales de al menos un segmento");
        assert!(!seen.is_empty());

        let metadata = tokio::fs::metadata(&tmp).await.unwrap();
        assert_eq!(
            metadata.len(),
            total_bytes as u64,
            "el tamaño real en disco debe coincidir con los bytes contados"
        );
        println!(
            "[e2e][iptv] grabación real: {} bytes, {} segmentos, archivo {}",
            total_bytes,
            seen.len(),
            tmp.display()
        );

        tokio::fs::remove_file(&tmp).await.ok();
    }
}
