use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use async_trait::async_trait;
use librqbit::api::TorrentIdOrHash;
use librqbit::{AddTorrent, AddTorrentOptions, Session, TorrentStatsState};

use super::{AddTorrentSource, TorrentEngine, TorrentInfo};

pub type HttpFallbackMap = Arc<Mutex<HashMap<String, String>>>;
/// Token opaco -> path real en disco (biblioteca Local, ver
/// `TorrentEngine::local_stream_url`). Mismo shape que `HttpFallbackMap`
/// (mapa en memoria, sin persistencia) por la misma razón: nunca se expone
/// el path crudo en la URL que llega al frontend.
pub type LocalFileMap = Arc<Mutex<HashMap<String, std::path::PathBuf>>>;

/// Default TorrentEngine: librqbit embedded directly as a Rust dependency,
/// no sidecar process, no HTTP API of its own exposed. This is the engine
/// that actually downloads bytes; ExternalQbittorrent/ExternalTransmission
/// (later) instead orchestrate a client the user already runs.
pub struct EmbeddedRqbit {
    session: Arc<Session>,
    stream_port: u16,
    http_fallback: HttpFallbackMap,
    local_files: LocalFileMap,
    downloads_dir: std::path::PathBuf,
}

impl EmbeddedRqbit {
    pub async fn new(download_dir: std::path::PathBuf) -> anyhow::Result<Self> {
        let session = Session::new(download_dir.clone())
            .await
            .context("no se pudo iniciar la sesión de librqbit")?;
        let http_fallback: HttpFallbackMap = Arc::new(Mutex::new(HashMap::new()));
        let local_files: LocalFileMap = Arc::new(Mutex::new(HashMap::new()));
        let stream_port = super::stream_server::spawn(session.clone(), http_fallback.clone(), local_files.clone())
            .await
            .context("no se pudo levantar el servidor de streaming local")?;
        Ok(Self {
            session,
            stream_port,
            http_fallback,
            local_files,
            downloads_dir: download_dir,
        })
    }
}

fn state_label(state: &TorrentStatsState) -> &'static str {
    match state {
        TorrentStatsState::Initializing => "initializing",
        TorrentStatsState::Live => "live",
        TorrentStatsState::Paused => "paused",
        TorrentStatsState::Error => "error",
    }
}

impl EmbeddedRqbit {
    async fn add_internal(
        &self,
        source: AddTorrentSource,
        opts: Option<AddTorrentOptions>,
    ) -> anyhow::Result<TorrentInfo> {
        let add = match source {
            AddTorrentSource::Magnet(uri) => AddTorrent::from_url(uri),
            AddTorrentSource::TorrentBytes(bytes) => AddTorrent::from_bytes(bytes),
        };
        let response = self.session.add_torrent(add, opts).await?;
        let handle = response
            .into_handle()
            .context("el torrent quedó en modo solo-listado (list_only), no se agregó a descarga")?;
        Ok(to_info(&handle))
    }
}

#[async_trait]
impl TorrentEngine for EmbeddedRqbit {
    async fn add(&self, source: AddTorrentSource) -> anyhow::Result<TorrentInfo> {
        self.add_internal(source, None).await
    }

    /// Sembrado real (ver commands::seed_archive_org_item_core): el archivo
    /// ya se colocó a mano en el path exacto antes de esta llamada —
    /// `overwrite: true` es necesario porque por default librqbit rechaza
    /// crear un archivo si ya existe algo ahí (protección de seguridad
    /// contra pisar datos ajenos sin querer), confirmado en vivo contra
    /// cosmos-laundromat (error real: "allow_overwrite = false").
    async fn add_seeding_from_disk(&self, source: AddTorrentSource) -> anyhow::Result<TorrentInfo> {
        self.add_internal(
            source,
            Some(AddTorrentOptions {
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
    }

    async fn list(&self) -> anyhow::Result<Vec<TorrentInfo>> {
        Ok(self
            .session
            .with_torrents(|iter| iter.map(|(_, handle)| to_info(handle)).collect()))
    }

    async fn pause(&self, id: &str) -> anyhow::Result<()> {
        let handle = self.get_handle(id)?;
        self.session.pause(&handle).await
    }

    async fn remove(&self, id: &str, delete_files: bool) -> anyhow::Result<()> {
        let numeric_id: usize = id.parse().context("id de torrent inválido")?;
        self.session
            .delete(TorrentIdOrHash::Id(numeric_id), delete_files)
            .await
    }

    async fn stream_url(&self, id: &str, file_idx: usize) -> anyhow::Result<String> {
        // Valida que el torrent exista antes de devolver una URL que
        // apuntaría a un 404 — falla temprano en vez de silencioso.
        self.get_handle(id)?;

        // Si hay un fallback HTTP registrado (fuentes tipo archive.org que
        // dependen de webseeds que librqbit no soporta), servir por ahí en
        // vez de por P2P — el torrent sigue descargando/sembrando en
        // segundo plano igual, esto sólo decide de dónde lee el reproductor.
        let has_fallback = self.http_fallback.lock().unwrap().contains_key(id);
        if has_fallback {
            return Ok(format!("http://127.0.0.1:{}/proxy/{id}", self.stream_port));
        }

        Ok(format!(
            "http://127.0.0.1:{}/stream/{id}/{file_idx}",
            self.stream_port
        ))
    }

    async fn register_http_fallback(&self, id: &str, url: String) -> anyhow::Result<()> {
        self.http_fallback
            .lock()
            .unwrap()
            .insert(id.to_string(), url);
        Ok(())
    }

    async fn local_stream_url(&self, path: std::path::PathBuf) -> anyhow::Result<String> {
        let token = uuid::Uuid::new_v4().to_string();
        self.local_files.lock().unwrap().insert(token.clone(), path);
        Ok(format!("http://127.0.0.1:{}/local/{token}", self.stream_port))
    }

    fn downloads_dir(&self) -> Option<&std::path::Path> {
        Some(&self.downloads_dir)
    }
}

impl EmbeddedRqbit {
    fn get_handle(&self, id: &str) -> anyhow::Result<Arc<librqbit::ManagedTorrent>> {
        let numeric_id: usize = id.parse().context("id de torrent inválido")?;
        self.session
            .get(TorrentIdOrHash::Id(numeric_id))
            .context("torrent no encontrado")
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod e2e {
    use super::*;
    use crate::sources::archive_org;

    /// Verificación real de extremo a extremo del backend (Tarea 9 del
    /// plan, actualizada tras diagnosticar la Tarea de fallback HTTP):
    /// agrega un torrent real de archive.org (dominio público,
    /// `turner_video_444` — Night of the Living Dead, 1968), registra el
    /// fallback HTTP (su .torrent depende de webseeds BEP19 que librqbit
    /// no soporta — ikatson/rqbit#500, confirmado con el test de control
    /// de Ubuntu: el motor y la red funcionan, este source específico no)
    /// y confirma streaming real con Range contra ese fallback.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "red real, no apto para CI por defecto; correr con --test-threads=1 \
                si se ejecuta junto a otro test de este módulo (cada uno abre una \
                sesión DHT real y pueden colisionar de puerto entre sí — nunca \
                ocurre en la app real, que solo abre una sesión)"]
    async fn add_real_archive_org_torrent_and_stream_it() {
        let tmp = tempdir();
        let engine = EmbeddedRqbit::new(tmp.clone()).await.expect("crear sesión");

        let http = reqwest::Client::new();
        let torrent_bytes = archive_org::fetch_torrent_bytes(&http, "turner_video_444")
            .await
            .expect("descargar .torrent de archive.org");
        assert!(
            torrent_bytes.len() > 100,
            "el .torrent descargado es sospechosamente chico: {} bytes",
            torrent_bytes.len()
        );

        let info = TorrentEngine::add(&engine, AddTorrentSource::TorrentBytes(torrent_bytes))
            .await
            .expect("agregar torrent al motor");
        println!("[e2e] torrent agregado: id={} name={:?}", info.id, info.name);

        let fallback_url = archive_org::primary_video_file(&http, "turner_video_444")
            .await
            .expect("resolver archivo reproducible en metadata de archive.org");
        println!("[e2e] fallback HTTP: {fallback_url}");
        TorrentEngine::register_http_fallback(&engine, &info.id, fallback_url)
            .await
            .expect("registrar fallback");

        // Streaming: pide el archivo sin Range (200) y con Range (206),
        // contra el propio HTTP server local, que a su vez reenvía al
        // fallback — no es un mock, son bytes reales de archive.org.
        let url = TorrentEngine::stream_url(&engine, &info.id, 0)
            .await
            .expect("stream_url");
        assert!(
            url.contains("/proxy/"),
            "con fallback registrado, stream_url debe apuntar a /proxy/, no a /stream/: {url}"
        );
        println!("[e2e] stream url: {url}");

        let full = http.get(&url).send().await.expect("request sin range");
        assert_eq!(full.status(), 200);

        let ranged = http
            .get(&url)
            .header("Range", "bytes=0-1023")
            .send()
            .await
            .expect("request con range");
        assert_eq!(ranged.status(), 206, "esperaba 206 Partial Content");
        assert_eq!(
            ranged.headers().get("content-length").unwrap(),
            "1024"
        );
        let body = ranged.bytes().await.expect("leer body");
        assert_eq!(body.len(), 1024, "el cuerpo debe traer exactamente los 1024 bytes pedidos");
    }

    /// Reproducción de bug #18 (plan: "reproducción falla ~75% del video").
    /// Un `<video>` real no pide el archivo entero de una vez — bufferiza en
    /// requests `Range` sucesivos a medida que avanza. Este test imita
    /// exactamente ese patrón contra el mismo ítem real de archive.org que ya
    /// usa `add_real_archive_org_torrent_and_stream_it`, recorriendo el
    /// archivo completo en chunks para ver si algún request falla — en
    /// particular alrededor del 75%, el punto reportado. Si el proxy/origen
    /// nunca falla, la causa no está en este tramo del backend (apunta a
    /// comportamiento de seek del lado del reproductor WebKitGTK, no
    /// reproducible desde acá — ver plan).
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "red real, no apto para CI por defecto; correr manualmente para diagnosticar bug #18"]
    async fn streams_full_file_in_sequential_ranges_like_a_real_player() {
        let tmp = tempdir();
        let engine = EmbeddedRqbit::new(tmp.clone()).await.expect("crear sesión");

        let http = reqwest::Client::new();
        let torrent_bytes = archive_org::fetch_torrent_bytes(&http, "turner_video_444")
            .await
            .expect("descargar .torrent de archive.org");
        let info = TorrentEngine::add(&engine, AddTorrentSource::TorrentBytes(torrent_bytes))
            .await
            .expect("agregar torrent al motor");

        let fallback_url = archive_org::primary_video_file(&http, "turner_video_444")
            .await
            .expect("resolver archivo reproducible en metadata de archive.org");
        TorrentEngine::register_http_fallback(&engine, &info.id, fallback_url)
            .await
            .expect("registrar fallback");

        let url = TorrentEngine::stream_url(&engine, &info.id, 0)
            .await
            .expect("stream_url");

        let head = http.get(&url).send().await.expect("request inicial sin range");
        assert_eq!(head.status(), 200, "request inicial sin Range debe ser 200");
        let total: u64 = head
            .headers()
            .get("content-length")
            .expect("respuesta sin content-length")
            .to_str()
            .unwrap()
            .parse()
            .expect("content-length no numérico");
        println!("[e2e][bug18] tamaño total del archivo: {total} bytes");

        const CHUNK: u64 = 4 * 1024 * 1024; // 4MB, similar al tamaño de buffer real de un <video>
        let mut pos: u64 = 0;
        let mut chunk_n = 0;
        while pos < total {
            let end = (pos + CHUNK - 1).min(total - 1);
            let pct = (pos as f64 / total as f64) * 100.0;
            let start_t = std::time::Instant::now();
            let resp = http
                .get(&url)
                .header("Range", format!("bytes={pos}-{end}"))
                .send()
                .await;
            let elapsed = start_t.elapsed();

            match resp {
                Ok(r) if r.status() == 206 => {
                    let expected_len = end - pos + 1;
                    match r.bytes().await {
                        Ok(body) if body.len() as u64 == expected_len => {
                            println!(
                                "[e2e][bug18] chunk {chunk_n} ok: {pct:.1}% bytes={pos}-{end} elapsed={elapsed:?}"
                            );
                        }
                        Ok(body) => panic!(
                            "chunk {chunk_n} en {pct:.1}% ({pos}-{end}): body incompleto, \
                             esperaba {expected_len} bytes, llegaron {}",
                            body.len()
                        ),
                        Err(e) => panic!(
                            "chunk {chunk_n} en {pct:.1}% ({pos}-{end}): error leyendo el body: {e}"
                        ),
                    }
                }
                Ok(r) => panic!(
                    "chunk {chunk_n} en {pct:.1}% ({pos}-{end}): status inesperado {} (esperaba 206), \
                     headers={:?}",
                    r.status(),
                    r.headers()
                ),
                Err(e) => panic!(
                    "chunk {chunk_n} en {pct:.1}% ({pos}-{end}): request falló: {e} \
                     (is_timeout={} is_connect={})",
                    e.is_timeout(),
                    e.is_connect()
                ),
            }

            pos = end + 1;
            chunk_n += 1;
        }
        println!("[e2e][bug18] archivo completo recorrido sin fallos: {chunk_n} chunks, {total} bytes");
    }

    /// Diagnóstico de control: ¿es P2P en general lo que no conecta, o es
    /// específico de archive.org (que depende de webseeds BEP19, no
    /// soportados por librqbit — ver .torrent url-list de turner_video_444)?
    /// Ubuntu tiene uno de los swarms BitTorrent más sanos que existen —
    /// si esto tampoco conecta peers, el problema es de red/entorno, no de
    /// archive.org específicamente.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "red real + swarm BitTorrent real, no apto para CI por defecto"]
    async fn control_ubuntu_torrent_gets_real_peers() {
        let tmp = tempdir();
        let engine = EmbeddedRqbit::new(tmp.clone()).await.expect("crear sesión");

        let http = reqwest::Client::new();
        let torrent_bytes = http
            .get("https://releases.ubuntu.com/26.04/ubuntu-26.04-desktop-amd64.iso.torrent")
            .send()
            .await
            .expect("descargar .torrent de ubuntu.com")
            .bytes()
            .await
            .expect("leer bytes")
            .to_vec();

        let info = TorrentEngine::add(&engine, AddTorrentSource::TorrentBytes(torrent_bytes))
            .await
            .expect("agregar torrent al motor");
        println!("[control] torrent agregado: id={} name={:?}", info.id, info.name);

        let saw_live_peer = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                if let Ok(handle) = engine.get_handle(&info.id) {
                    if let Some(live) = handle.stats().live {
                        println!("[control][diag] peer_stats: {:?}", live.snapshot.peer_stats);
                        if live.snapshot.peer_stats.live > 0 {
                            return true;
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        })
        .await
        .unwrap_or(false);

        assert!(
            saw_live_peer,
            "ni siquiera el swarm de Ubuntu (miles de peers reales) logró una \
             conexión P2P viva en 60s — esto apunta a un bloqueo de red del \
             entorno (egress a puertos altos), no a un problema específico \
             de archive.org/webseeds"
        );
    }

    fn tempdir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("popcorn-e2e-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

fn to_info(handle: &Arc<librqbit::ManagedTorrent>) -> TorrentInfo {
    let stats = handle.stats();
    let (download_speed_mbps, upload_speed_mbps) = stats
        .live
        .as_ref()
        .map(|l| (l.download_speed.mbps, l.upload_speed.mbps))
        .unwrap_or((0.0, 0.0));
    TorrentInfo {
        id: handle.id().to_string(),
        name: handle.name(),
        info_hash: hex_encode(&handle.info_hash().0),
        progress_bytes: stats.progress_bytes,
        total_bytes: stats.total_bytes,
        download_speed_mbps,
        upload_speed_mbps,
        finished: stats.finished,
        state: state_label(&stats.state).to_string(),
        error: stats.error,
    }
}
