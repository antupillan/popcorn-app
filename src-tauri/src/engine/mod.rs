pub mod embedded_rqbit;
pub mod external_qbittorrent;
pub(crate) mod remux;
pub(crate) mod stream_server;

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
    /// Bytes subidos acumulados desde que se agregó el torrent — usado para
    /// detectar relación 1:1 en el sembrado automático (ver
    /// online_library::add_online_item_inner). 0 si el motor no expone
    /// stats en vivo (torrent recién agregado, todavía inicializando).
    pub uploaded_bytes: u64,
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

    /// Como `add`, pero acotando velocidad de bajada/subida (bytes/seg,
    /// `None` = sin límite) — usado por el límite global configurable en
    /// Ajustes (`app_settings::get_speed_limits`). Solo aplica al torrent
    /// que se agrega en este momento: no hay forma de reconfigurar en
    /// caliente uno ya agregado (confirmado leyendo `librqbit` 8.1.1 — el
    /// setter del rate limiter existe pero es privado al crate). Delega a
    /// `add` por defecto para motores que no distinguen el caso.
    async fn add_with_limits(
        &self,
        source: AddTorrentSource,
        download_bps: Option<u32>,
        upload_bps: Option<u32>,
    ) -> anyhow::Result<TorrentInfo> {
        let _ = (download_bps, upload_bps);
        self.add(source).await
    }

    /// Como `add`, pero para cuando el archivo completo ya se colocó a mano
    /// en el destino antes de llamar (sembrado real, ver
    /// `commands::seed_archive_org_item_core`) — necesita permiso explícito
    /// para reusar/sobreescribir lo que ya esté ahí en vez de rechazarlo por
    /// seguridad. `upload_bps` acota la velocidad de subida (`None` = sin
    /// límite) — valor operacional que decide el caller, nunca hardcodeado
    /// acá (Mandato 5). Delega a `add` por defecto (comportamiento idéntico,
    /// ignora el límite) para motores que no distinguen el caso; solo
    /// `EmbeddedRqbit` lo necesita de verdad.
    async fn add_seeding_from_disk(
        &self,
        source: AddTorrentSource,
        upload_bps: Option<u32>,
    ) -> anyhow::Result<TorrentInfo> {
        let _ = upload_bps;
        self.add(source).await
    }
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

    /// Carpeta de descargas real del motor — necesaria para colocar bytes
    /// en el lugar exacto que espera un `.torrent` antes de agregarlo (ver
    /// sembrado real de archive.org, `commands::seed_archive_org_item_core`).
    /// `None` por defecto. `EmbeddedRqbit` la expone porque ya la recibe en
    /// `new()`; `ExternalQbittorrent` la cachea al conectar (`app/defaultSavePath`,
    /// una sola vez — best-effort si autoTMM cambia el save path real por
    /// torrent/categoría, no se recalcula en cada llamada porque este método
    /// es sync y consultarlo de nuevo requeriría una llamada de red).
    fn downloads_dir(&self) -> Option<&std::path::Path> {
        None
    }

    /// Ruta real en disco de un archivo dentro de un torrent — necesaria
    /// para remuxear (ver `engine::remux`), que opera sobre el archivo ya
    /// descargado, no sobre el stream P2P. Error por defecto. Async (a
    /// diferencia de `downloads_dir`) porque `ExternalQbittorrent` necesita
    /// una llamada de red real (`torrents/properties` + `torrents/files`)
    /// para resolverla — `EmbeddedRqbit` la resuelve en memoria (sin I/O)
    /// pero igual expone el método como async para cumplir un único
    /// contrato de trait.
    async fn file_path(&self, _id: &str, _file_idx: usize) -> anyhow::Result<std::path::PathBuf> {
        anyhow::bail!("el motor activo no expone rutas de archivo (requiere EmbeddedRqbit o ExternalQbittorrent)")
    }
}
