use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use librqbit::api::TorrentIdOrHash;
use librqbit::{AddTorrent, Session, TorrentStatsState};

use super::{AddTorrentSource, TorrentEngine, TorrentInfo};

/// Default TorrentEngine: librqbit embedded directly as a Rust dependency,
/// no sidecar process, no HTTP API of its own exposed. This is the engine
/// that actually downloads bytes; ExternalQbittorrent/ExternalTransmission
/// (later) instead orchestrate a client the user already runs.
pub struct EmbeddedRqbit {
    session: Arc<Session>,
    stream_port: u16,
}

impl EmbeddedRqbit {
    pub async fn new(download_dir: std::path::PathBuf) -> anyhow::Result<Self> {
        let session = Session::new(download_dir)
            .await
            .context("no se pudo iniciar la sesión de librqbit")?;
        let stream_port = super::stream_server::spawn(session.clone())
            .await
            .context("no se pudo levantar el servidor de streaming local")?;
        Ok(Self {
            session,
            stream_port,
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

#[async_trait]
impl TorrentEngine for EmbeddedRqbit {
    async fn add(&self, source: AddTorrentSource) -> anyhow::Result<TorrentInfo> {
        let add = match source {
            AddTorrentSource::Magnet(uri) => AddTorrent::from_url(uri),
            AddTorrentSource::TorrentBytes(bytes) => AddTorrent::from_bytes(bytes),
        };
        let response = self.session.add_torrent(add, None).await?;
        let handle = response
            .into_handle()
            .context("el torrent quedó en modo solo-listado (list_only), no se agregó a descarga")?;
        Ok(to_info(&handle))
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
        Ok(format!(
            "http://127.0.0.1:{}/stream/{id}/{file_idx}",
            self.stream_port
        ))
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
    /// plan): agrega un torrent real de archive.org (dominio público,
    /// `turner_video_444` — Night of the Living Dead, 1968) y confirma
    /// descarga real (no simulada) más streaming HTTP con Range.
    /// Depende de red/swarm reales — no determinista por naturaleza; si el
    /// swarm no responde en la ventana de esta prueba, falla honestamente
    /// en vez de fingir éxito.
    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "red real + swarm BitTorrent real, no apto para CI por defecto"]
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

        // Diagnóstico: ¿hay peers conectados? Si esto queda siempre en 0,
        // es un problema de red del entorno (egress bloqueado), no del
        // motor. Se imprime aparte porque TorrentStats no expone esto vía
        // el trait TorrentEngine (es deliberadamente delgado).
        tokio::time::sleep(std::time::Duration::from_secs(15)).await;
        if let Ok(handle) = engine.get_handle(&info.id) {
            println!("[e2e][diag] live stats: {:#?}", handle.stats().live);
        }

        // Espera bytes reales de progreso, no sólo metadata. 90s: piezas de
        // archive.org suelen ser rápidas, pero es red real, no un mock.
        let saw_progress = tokio::time::timeout(std::time::Duration::from_secs(75), async {
            loop {
                let list = TorrentEngine::list(&engine).await.expect("list");
                let t = list.iter().find(|t| t.id == info.id).expect("torrent en la lista");
                println!(
                    "[e2e] estado={} progreso={}/{} bytes error={:?}",
                    t.state, t.progress_bytes, t.total_bytes, t.error
                );
                if t.progress_bytes > 0 {
                    return true;
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        })
        .await
        .unwrap_or(false);

        assert!(
            saw_progress,
            "no se observaron bytes de descarga real en 90s — revisar conectividad \
             del sandbox al swarm BitTorrent antes de asumir que el motor funciona"
        );

        // Streaming: pide el archivo 0 sin Range (200) y con Range (206),
        // contra el propio HTTP server local, no un mock.
        let url = TorrentEngine::stream_url(&engine, &info.id, 0)
            .await
            .expect("stream_url");
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
