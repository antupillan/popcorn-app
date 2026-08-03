pub mod embedded_rqbit;
mod stream_server;

use async_trait::async_trait;
use serde::Serialize;

/// Source to add to the engine — either a magnet URI or raw .torrent bytes.
/// Kept separate from `db::media_items.source_type` (which also covers
/// `archive_org`, a *catalog* origin, not an engine input format).
pub enum AddTorrentSource {
    Magnet(String),
    TorrentBytes(Vec<u8>),
}

#[derive(Serialize, Clone, Debug)]
pub struct TorrentInfo {
    pub id: String,
    pub name: Option<String>,
    pub info_hash: String,
    pub progress_bytes: u64,
    pub total_bytes: u64,
    pub download_speed_mbps: f64,
    pub upload_speed_mbps: f64,
    pub finished: bool,
    pub state: String,
    pub error: Option<String>,
}

/// Abstraction over how torrent data actually gets fetched, per the plan's
/// legal-shielding design: the default embeds librqbit directly, but a user
/// can swap in ExternalQbittorrent/ExternalTransmission so Popcorn only
/// orchestrates a client they already run, never touching bytes itself.
#[async_trait]
pub trait TorrentEngine: Send + Sync {
    async fn add(&self, source: AddTorrentSource) -> anyhow::Result<TorrentInfo>;
    async fn list(&self) -> anyhow::Result<Vec<TorrentInfo>>;
    async fn pause(&self, id: &str) -> anyhow::Result<()>;
    async fn remove(&self, id: &str, delete_files: bool) -> anyhow::Result<()>;
    /// Local HTTP URL the frontend `<video>` can point at with Range support.
    async fn stream_url(&self, id: &str, file_idx: usize) -> anyhow::Result<String>;

    /// Optional capability: register a direct HTTP source to serve reads
    /// from when pure P2P won't deliver bytes (e.g. archive.org items that
    /// rely on BEP19 webseeds, which librqbit doesn't implement — see
    /// ikatson/rqbit#500). No-op by default; only EmbeddedRqbit needs this.
    /// This is a v1 stopgap (prefer-HTTP-when-registered), not real BEP19
    /// piece-level mixing with P2P — that's tracked as a future paid-tier
    /// feature per the plan.
    async fn register_http_fallback(&self, _id: &str, _url: String) -> anyhow::Result<()> {
        Ok(())
    }
}
