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
