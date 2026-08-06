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

    /// Whether `id` currently resolves in this engine session. Needed
    /// because `EmbeddedRqbit` holds no session persistence — every process
    /// restart loses its in-memory torrents while `media_items.engine_torrent_id`
    /// in SQLite still points at the old (now dangling) id. Callers use this
    /// to detect that and heal by re-adding from `media_items` (source of
    /// truth) instead of surfacing a raw "torrent no encontrado" to the user.
    /// Default impl reuses `stream_url`'s own existence check — cheap, no
    /// extra state per engine implementation.
    async fn exists(&self, id: &str) -> bool {
        self.stream_url(id, 0).await.is_ok()
    }

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

    /// Optional capability: sirve un archivo local (biblioteca Local, no un
    /// torrent) vía el mismo servidor HTTP con soporte de Range que ya usa
    /// `stream_url`/`register_http_fallback`, sin exponer el path crudo en
    /// la URL devuelta. Error por defecto; solo `EmbeddedRqbit` lo
    /// implementa hoy. Nota de arquitectura honesta (ver plan, no resuelta):
    /// esto acopla la biblioteca Local a que el motor activo sea
    /// `EmbeddedRqbit` — el día que exista `ExternalQbittorrent`/
    /// `ExternalTransmission`, este método fallaría con ellos activos.
    async fn local_stream_url(&self, _path: std::path::PathBuf) -> anyhow::Result<String> {
        anyhow::bail!("el motor activo no soporta biblioteca local (requiere EmbeddedRqbit)")
    }
}
