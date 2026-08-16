use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use qbit_rs::model::{
    AddTorrentArg, Credential, GetTorrentListArg, Priority, State as QbState, TorrentFile, TorrentSource,
};
use qbit_rs::Qbit;

use super::stream_server::{LocalFileEntry, LocalFileMap, ReadinessProbe};
use super::{AddTorrentSource, TorrentEngine, TorrentInfo};

/// Orquesta un qBittorrent que el usuario ya instaló y corre por su cuenta
/// (Web API, no un cliente embebido) — Popcorn nunca toca bytes de la red
/// en este camino, solo agrega/lista/pausa/quita vía HTTP contra el daemon
/// externo. Streaming: reusa el mismo servidor `/local/{token}` que sirve
/// la Biblioteca Local, leyendo el archivo ya descargado directo del disco
/// (asume que el daemon corre en la misma máquina — caso normal de una app
/// de escritorio de un solo usuario, documentado en el plan).
pub struct ExternalQbittorrent {
    /// `Arc` porque `Qbit` no es `Clone` (confirmado en el crate) y
    /// `QbitFileProbe` necesita su propia referencia compartida para
    /// consultar `ready_bytes` en cada poll del servidor de streaming,
    /// independiente del ciclo de vida de este motor.
    client: Arc<Qbit>,
    stream_port: u16,
    local_files: LocalFileMap,
    /// Cacheado una sola vez al conectar (`app/defaultSavePath`) — el
    /// trait expone `downloads_dir` como método sync, así que no se puede
    /// re-consultar por red en cada llamada. Best-effort: si qBittorrent
    /// usa autoTMM con categorías que apuntan a otra carpeta, este valor
    /// puede no coincidir con el save_path real de un torrent puntual;
    /// `file_path` sí consulta el save_path real por torrent vía la API,
    /// así que el streaming/remux no depende de este caché.
    downloads_dir: Option<PathBuf>,
}

impl ExternalQbittorrent {
    /// Falla si no logra autenticar — el caller (`lib.rs::setup`) decide
    /// qué hacer con el error (caer a `EmbeddedRqbit` con aviso visible,
    /// ver plan "motor_externo_qbittorrent", nunca un fallback silencioso).
    pub async fn new(
        base_url: &str,
        username: &str,
        password: &str,
        stream_port: u16,
        local_files: LocalFileMap,
    ) -> anyhow::Result<Self> {
        // Validar la URL nosotros mismos antes de pasarla a `Qbit::new`:
        // confirmado en vivo que el crate hace `.unwrap()` interno sobre el
        // parseo (panic real con `RelativeUrlWithoutBase` ante un
        // `base_url` vacío/inválido — tira todo el proceso, no un Result).
        // Un `anyhow::Result::Err` acá sí lo puede manejar el caller
        // (fallback a EmbeddedRqbit), un panic no.
        let url = url::Url::parse(base_url)
            .with_context(|| format!("URL de la WebUI de qBittorrent inválida: {base_url:?}"))?;
        let client = Arc::new(Qbit::new(url, Credential::new(username.to_string(), password.to_string())));
        client
            .login(false)
            .await
            .context("no se pudo autenticar contra la Web API de qBittorrent")?;
        let downloads_dir = client.get_default_save_path().await.ok();
        Ok(Self {
            client,
            stream_port,
            local_files,
            downloads_dir,
        })
    }

    async fn add_internal(&self, source: AddTorrentSource) -> anyhow::Result<TorrentInfo> {
        let torrent_source = match source {
            AddTorrentSource::Magnet(uri) => {
                let url = url::Url::parse(&uri).context("magnet URI inválida")?;
                TorrentSource::Urls { urls: vec![url].into() }
            }
            AddTorrentSource::TorrentBytes(bytes) => TorrentSource::TorrentFiles {
                torrents: vec![TorrentFile {
                    filename: "torrent".to_string(),
                    data: bytes,
                }],
            },
        };
        let arg = AddTorrentArg::builder().source(torrent_source).build();
        self.client.add_torrent(arg).await.context("qBittorrent rechazó el torrent")?;
        // El endpoint de agregar no devuelve el hash del torrent creado —
        // hay que buscarlo en la lista recién agregada (ordenada por fecha
        // de alta descendente, el nuestro es el primero).
        let torrents = self
            .client
            .get_torrent_list(GetTorrentListArg::builder().sort("added_on".to_string()).reverse(true).build())
            .await
            .context("no se pudo confirmar el torrent recién agregado")?;
        let torrent = torrents
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("qBittorrent no reportó ningún torrent tras agregarlo"))?;
        // Fire-and-forget: sin esto, qBittorrent baja piezas rarest-first y
        // `ready_bytes` (usado por el streaming progresivo, ver
        // `stream_url`) no puede asumir que los primeros N bytes de un
        // archivo ya están escritos solo porque su `progress` lo sugiere.
        // No fatal si falla — el peor caso es el mismo error explícito que
        // ya existía antes de este cambio si el swarm no entrega a tiempo
        // (ver poll con timeout en `stream_server::local_stream_handler`).
        if let Some(hash) = torrent.hash.clone() {
            if let Err(e) = self.client.toggle_sequential_download(vec![hash]).await {
                eprintln!("[popcorn] no se pudo activar descarga secuencial: {e}");
            }
        }
        Ok(to_info(torrent))
    }

    /// Bytes seguros de leer desde el byte 0 de `file_idx` sin tocar datos
    /// que el swarm todavía no escribió — `progress` de qBittorrent es por
    /// archivo (no del torrent completo) y, con descarga secuencial activa,
    /// ya refleja bytes contiguos desde el byte 0 de ESE archivo (no hace
    /// falta recalcularlo desde `piece_range`). Se resta un `piece_size` de
    /// margen por si la última pieza contada todavía se está escribiendo a
    /// disco cuando se consulta.
    // Producción consulta esto vía el trait `ReadyBytesProbe` (impl más
    // abajo en este archivo, usado por stream_server.rs) - este wrapper
    // directo solo lo llaman tests que ya tienen un ExternalQbittorrent
    // concreto a mano.
    #[cfg(test)]
    pub(crate) async fn ready_bytes(&self, id: &str, file_idx: usize) -> anyhow::Result<u64> {
        ready_bytes_via_client(&self.client, id, file_idx).await
    }
}

/// Lógica compartida por `ExternalQbittorrent::ready_bytes` y
/// `QbitFileProbe` (este último solo tiene `Arc<Qbit>`, no el motor
/// completo, así que no puede llamar al método de instancia).
async fn ready_bytes_via_client(client: &Qbit, id: &str, file_idx: usize) -> anyhow::Result<u64> {
    let props = client
        .get_torrent_properties(id)
        .await
        .context("no se pudo leer las propiedades del torrent en qBittorrent")?;
    let piece_margin = props.piece_size.unwrap_or(0).max(0) as u64;
    let contents = client
        .get_torrent_contents(id, None)
        .await
        .context("no se pudo leer el listado de archivos del torrent en qBittorrent")?;
    let file = contents
        .into_iter()
        .find(|f| f.index == file_idx as u64)
        .ok_or_else(|| anyhow::anyhow!("file_idx {file_idx} fuera de rango"))?;
    let ready = (file.progress * file.size as f64) as u64;
    Ok(ready.saturating_sub(piece_margin))
}

/// Consulta `ready_bytes` bajo demanda desde el servidor de streaming (ver
/// `ReadinessProbe`) — solo necesita `Arc<Qbit>` + identificadores, nunca el
/// motor completo, para no atarse a su ciclo de vida.
struct QbitFileProbe {
    client: Arc<Qbit>,
    hash: String,
    file_idx: usize,
}

#[async_trait]
impl ReadinessProbe for QbitFileProbe {
    // Nunca falla hacia arriba: un error de red acá debe leerse como "no
    // hay nada seguro para leer todavía", no interrumpir el poll del
    // servidor de streaming con un panic/Result que no tiene a dónde ir
    // (la firma del trait es infalible a propósito).
    async fn ready_bytes(&self) -> u64 {
        ready_bytes_via_client(&self.client, &self.hash, self.file_idx)
            .await
            .unwrap_or(0)
    }
}

fn bytes_per_sec_to_mbps(bytes_per_sec: i64) -> f64 {
    // Mismo factor que `librqbit_core::speed_estimator` (bps / 1024 / 1024)
    // — el campo se llama "mbps" en todo el código pero es MiB/s real, no
    // megabits (confirmado leyendo el crate, no asumido).
    bytes_per_sec.max(0) as f64 / 1024.0 / 1024.0
}

fn state_label(state: Option<QbState>) -> &'static str {
    use QbState::*;
    match state {
        Some(Error) | Some(MissingFiles) => "error",
        Some(PausedUP) | Some(PausedDL) => "paused",
        Some(Downloading) | Some(Uploading) | Some(ForcedUP) | Some(ForcedDL) | Some(QueuedUP)
        | Some(QueuedDL) | Some(StalledUP) | Some(StalledDL) => "live",
        // Checking*/Allocating/MetaDL/Moving/Unknown/None: transicional o
        // desconocido — "initializing" es el bucket más honesto del
        // vocabulario existente (ver `TorrentInfo.state`, unión estricta
        // en el frontend) para un estado que no es ni pausa ni error ni
        // transferencia activa confirmada.
        _ => "initializing",
    }
}

fn to_info(t: qbit_rs::model::Torrent) -> TorrentInfo {
    let total_bytes = t.size.unwrap_or(0).max(0) as u64;
    let progress = t.progress.unwrap_or(0.0);
    let progress_bytes = (progress * total_bytes as f64) as u64;
    let is_error = matches!(t.state, Some(QbState::Error) | Some(QbState::MissingFiles));
    TorrentInfo {
        id: t.hash.clone().unwrap_or_default(),
        name: t.name,
        info_hash: t.hash.unwrap_or_default(),
        progress_bytes,
        total_bytes,
        download_speed_mbps: bytes_per_sec_to_mbps(t.dlspeed.unwrap_or(0)),
        upload_speed_mbps: bytes_per_sec_to_mbps(t.upspeed.unwrap_or(0)),
        uploaded_bytes: t.uploaded.unwrap_or(0).max(0) as u64,
        finished: progress >= 1.0,
        state: state_label(t.state).to_string(),
        error: is_error.then(|| "qBittorrent reportó un error en este torrent".to_string()),
    }
}

#[async_trait]
impl TorrentEngine for ExternalQbittorrent {
    async fn add(&self, source: AddTorrentSource) -> anyhow::Result<TorrentInfo> {
        self.add_internal(source).await
    }

    async fn list(&self) -> anyhow::Result<Vec<TorrentInfo>> {
        let torrents = self
            .client
            .get_torrent_list(GetTorrentListArg::default())
            .await
            .context("no se pudo listar torrents de qBittorrent")?;
        Ok(torrents.into_iter().map(to_info).collect())
    }

    async fn pause(&self, id: &str) -> anyhow::Result<()> {
        self.client
            .stop_torrents(vec![id.to_string()])
            .await
            .context("no se pudo pausar el torrent en qBittorrent")
    }

    async fn remove(&self, id: &str, delete_files: bool) -> anyhow::Result<()> {
        self.client
            .delete_torrents(vec![id.to_string()], Some(delete_files))
            .await
            .context("no se pudo quitar el torrent de qBittorrent")
    }

    /// A diferencia de `EmbeddedRqbit::stream_url` (streaming P2P
    /// progresivo real vía `/stream/*`, que espera cada pieza vía
    /// `handle.stream()`), este motor reusa `/local/*` pero con un
    /// `ReadinessProbe` que hace lo mismo desde afuera: descarga secuencial
    /// activada al agregar el torrent (`add_internal`) + prioridad de
    /// archivo acá (para el caso multi-archivo, evita bajar todo el resto
    /// del torrent en orden antes de llegar al que el usuario quiere ver) +
    /// poll de `ready_bytes` en `stream_server::local_stream_handler` antes
    /// de servir cada rango. Reemplaza el bloqueo anterior
    /// ("esperá a que termine") que dejaba este motor notablemente menos
    /// usable que `EmbeddedRqbit` — ver plan de este cambio.
    async fn stream_url(&self, id: &str, file_idx: usize) -> anyhow::Result<String> {
        let contents = self
            .client
            .get_torrent_contents(id, None)
            .await
            .context("no se pudo leer el listado de archivos del torrent en qBittorrent")?;
        if contents.len() > 1 {
            let other_indexes: Vec<i64> = contents
                .iter()
                .map(|f| f.index as i64)
                .filter(|&idx| idx != file_idx as i64)
                .collect();
            if !other_indexes.is_empty() {
                if let Err(e) = self.client.set_file_priority(id, other_indexes, Priority::DoNotDownload).await {
                    eprintln!("[popcorn] no se pudo bajar prioridad de archivos no elegidos: {e}");
                }
            }
            if let Err(e) = self.client.set_file_priority(id, vec![file_idx as i64], Priority::Normal).await {
                eprintln!("[popcorn] no se pudo priorizar el archivo elegido para reproducir: {e}");
            }
        }

        let path = self.file_path(id, file_idx).await?;
        let token = uuid::Uuid::new_v4().to_string();
        let probe: Arc<dyn ReadinessProbe> = Arc::new(QbitFileProbe {
            client: self.client.clone(),
            hash: id.to_string(),
            file_idx,
        });
        self.local_files
            .lock()
            .unwrap()
            .insert(token.clone(), LocalFileEntry { path, probe: Some(probe) });
        Ok(format!("http://127.0.0.1:{}/local/{token}", self.stream_port))
    }

    async fn exists(&self, id: &str) -> bool {
        self.client.get_torrent_properties(id).await.is_ok()
    }

    fn downloads_dir(&self) -> Option<&std::path::Path> {
        self.downloads_dir.as_deref()
    }

    async fn file_path(&self, id: &str, file_idx: usize) -> anyhow::Result<PathBuf> {
        let props = self
            .client
            .get_torrent_properties(id)
            .await
            .context("no se pudo leer las propiedades del torrent en qBittorrent")?;
        let save_path = props
            .save_path
            .ok_or_else(|| anyhow::anyhow!("qBittorrent no reportó save_path para este torrent"))?;
        let contents = self
            .client
            .get_torrent_contents(id, None)
            .await
            .context("no se pudo leer el listado de archivos del torrent en qBittorrent")?;
        let file = contents
            .into_iter()
            .find(|f| f.index == file_idx as u64)
            .ok_or_else(|| anyhow::anyhow!("file_idx {file_idx} fuera de rango"))?;
        Ok(PathBuf::from(save_path).join(&file.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_conversion_matches_librqbit_units_mib_per_sec() {
        // 1 MiB/s = 1024*1024 bytes/seg — mismo factor que
        // `librqbit_core::speed_estimator::SpeedEstimator::mbps` (ver
        // comentario en `bytes_per_sec_to_mbps`), no megabits.
        assert!((bytes_per_sec_to_mbps(1024 * 1024) - 1.0).abs() < 1e-9);
        assert_eq!(bytes_per_sec_to_mbps(0), 0.0);
        // dlspeed/upspeed nunca deberían venir negativos, pero si la API
        // alguna vez lo hiciera, no debe underflow-ear al castear a u64
        // en otro lado — clamp defensivo, cubierto acá.
        assert_eq!(bytes_per_sec_to_mbps(-5), 0.0);
    }

    #[test]
    fn state_label_maps_every_qbittorrent_state_to_the_frontends_closed_vocabulary() {
        assert_eq!(state_label(Some(QbState::Error)), "error");
        assert_eq!(state_label(Some(QbState::MissingFiles)), "error");
        assert_eq!(state_label(Some(QbState::PausedDL)), "paused");
        assert_eq!(state_label(Some(QbState::PausedUP)), "paused");
        assert_eq!(state_label(Some(QbState::Downloading)), "live");
        assert_eq!(state_label(Some(QbState::StalledUP)), "live");
        assert_eq!(state_label(Some(QbState::CheckingDL)), "initializing");
        assert_eq!(state_label(Some(QbState::Unknown)), "initializing");
        assert_eq!(state_label(None), "initializing");
    }

    /// `Torrent` no expone builder ni `Default` (confirmado en docs.rs) —
    /// se construye vía su propio `Deserialize` con solo los campos que el
    /// test necesita, mismo tipo de dato que llegaría de la Web API real.
    fn torrent_with(hash: &str, size: i64, progress: f64) -> qbit_rs::model::Torrent {
        serde_json::from_value(serde_json::json!({
            "hash": hash,
            "size": size,
            "progress": progress,
        }))
        .expect("deserializar un Torrent mínimo")
    }

    #[test]
    fn to_info_computes_finished_from_progress_not_from_a_guessed_sentinel() {
        assert!(to_info(torrent_with("abc123", 1000, 1.0)).finished);

        let info = to_info(torrent_with("abc123", 1000, 0.5));
        assert!(!info.finished);
        assert_eq!(info.progress_bytes, 500);
    }

    /// Red real, deshabilitado por defecto: requiere un qBittorrent local
    /// con la WebUI habilitada (Herramientas -> Opciones -> Web UI, puerto
    /// por defecto 8080) y credenciales reales. No se pudo correr en esta
    /// sesión (sin una instancia real disponible en este entorno) — dejar
    /// constancia explícita (Mandato 12) en vez de simularlo. Ejercita el
    /// ciclo completo: agregar, listar, confirmar progreso, pausar, quitar,
    /// contra un torrent de dominio público real.
    #[tokio::test]
    #[ignore = "requiere un qBittorrent real corriendo local con WebUI habilitada \
                (Herramientas -> Opciones -> Web UI); ajustar host/usuario/password \
                hardcodeados abajo si no coinciden con la instancia de prueba"]
    async fn add_list_pause_and_remove_a_real_torrent_against_a_real_qbittorrent() {
        // Credenciales por variable de entorno, nunca hardcodeadas en el
        // repo (Mandato 5) — default apunta al setup local más común
        // (WebUI recién habilitada, sin cambiar usuario/contraseña).
        let base_url = std::env::var("QBIT_TEST_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
        let username = std::env::var("QBIT_TEST_USERNAME").unwrap_or_else(|_| "admin".to_string());
        let password = std::env::var("QBIT_TEST_PASSWORD").unwrap_or_else(|_| "adminadmin".to_string());

        let local_files: LocalFileMap = std::sync::Arc::new(std::sync::Mutex::new(Default::default()));
        let engine = ExternalQbittorrent::new(&base_url, &username, &password, 0, local_files)
            .await
            .expect("conectar y autenticar contra qBittorrent local");

        let http = reqwest::Client::new();
        let torrent_bytes = crate::sources::archive_org::fetch_torrent_bytes(&http, "turner_video_444")
            .await
            .expect("descargar .torrent de archive.org");

        let info = TorrentEngine::add(&engine, AddTorrentSource::TorrentBytes(torrent_bytes))
            .await
            .expect("agregar torrent a qBittorrent");
        assert!(!info.id.is_empty(), "el hash del torrent agregado no debería venir vacío");

        let listed = TorrentEngine::list(&engine).await.expect("listar torrents");
        assert!(listed.iter().any(|t| t.id == info.id), "el torrent agregado debe aparecer en la lista");

        TorrentEngine::pause(&engine, &info.id).await.expect("pausar el torrent");

        TorrentEngine::remove(&engine, &info.id, true)
            .await
            .expect("quitar el torrent (con archivos)");
    }

    /// Red real, deshabilitado por defecto — mismo criterio que el test
    /// anterior. Reusa "Doctorin1946" (item real de archive.org,
    /// multi-archivo, el mismo que expuso el bug original de streaming
    /// progresivo) para confirmar que `add_internal` deja la descarga
    /// secuencial activa y que `stream_url` aplica las prioridades de
    /// archivo correctas — Mandato 12, verificación contra qBittorrent
    /// real, no solo revisión de código.
    #[tokio::test]
    #[ignore = "requiere un qBittorrent real corriendo local con WebUI habilitada; \
                ajustar host/usuario/password hardcodeados abajo si no coinciden \
                con la instancia de prueba"]
    async fn stream_url_activates_sequential_download_and_file_priority_against_a_real_qbittorrent() {
        let base_url = std::env::var("QBIT_TEST_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
        let username = std::env::var("QBIT_TEST_USERNAME").unwrap_or_else(|_| "admin".to_string());
        let password = std::env::var("QBIT_TEST_PASSWORD").unwrap_or_else(|_| "adminadmin".to_string());

        let local_files: LocalFileMap = std::sync::Arc::new(std::sync::Mutex::new(Default::default()));
        let engine = ExternalQbittorrent::new(&base_url, &username, &password, 0, local_files)
            .await
            .expect("conectar y autenticar contra qBittorrent local");

        let http = reqwest::Client::new();
        let torrent_bytes = crate::sources::archive_org::fetch_torrent_bytes(&http, "Doctorin1946")
            .await
            .expect("descargar .torrent de archive.org");

        let info = TorrentEngine::add(&engine, AddTorrentSource::TorrentBytes(torrent_bytes))
            .await
            .expect("agregar torrent a qBittorrent");

        let arg = qbit_rs::model::GetTorrentListArg::builder().hashes(info.id.clone()).build();
        let torrents = engine.client.get_torrent_list(arg).await.expect("listar tras agregar");
        assert_eq!(
            torrents.first().and_then(|t| t.seq_dl),
            Some(true),
            "add_internal debería haber activado descarga secuencial"
        );

        let contents = engine.client.get_torrent_contents(&info.id, None).await.expect("listar archivos");
        assert!(
            contents.len() > 1,
            "Doctorin1946 debe ser multi-archivo para ejercitar el caso real que expuso el bug"
        );
        let file_idx = 0usize;

        TorrentEngine::stream_url(&engine, &info.id, file_idx)
            .await
            .expect("armar stream_url");

        let contents_after = engine
            .client
            .get_torrent_contents(&info.id, None)
            .await
            .expect("listar archivos tras stream_url");
        for f in &contents_after {
            if f.index == file_idx as u64 {
                assert_eq!(
                    f.priority,
                    qbit_rs::model::Priority::Normal,
                    "el archivo elegido para reproducir debe quedar en prioridad normal"
                );
            } else {
                assert_eq!(
                    f.priority,
                    qbit_rs::model::Priority::DoNotDownload,
                    "los archivos no elegidos deben quedar sin descargar"
                );
            }
        }

        let ready = engine.ready_bytes(&info.id, file_idx).await.expect("consultar ready_bytes");
        println!("ready_bytes tras stream_url: {ready} bytes");

        TorrentEngine::remove(&engine, &info.id, true)
            .await
            .expect("quitar el torrent (con archivos)");
    }
}
