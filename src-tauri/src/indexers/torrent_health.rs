// Detector de salud real de torrents (ver Planes_mejora_popcorn/
// detector_salud_indexers.txt). Consulta el swarm de verdad (tracker UDP +
// DHT como respaldo), a diferencia del `seeders` que ya trae IndexerResult
// (lo que el propio indexer reporta, sin verificar).
//
// Hallazgo real durante la implementación (Mandato 4, verificado leyendo el
// fuente vendorizado de librqbit-tracker-comms 3.0.0, no asumido del plan
// original): `AnnounceFields`/`AnnounceResponse`/`TrackerAddr` viven en
// `mod tracker_comms_udp` (privado, sin `pub`) — `UdpTrackerClient::announce`
// es literalmente imposible de llamar desde afuera de esa crate pese a ser
// `pub fn`, porque sus tipos de parámetro/retorno no son nombrables. Eso
// tira abajo la idea original de "tracker da seeders/leechers reales,
// distinto de la aproximación de DHT". Lo único invocable desde acá es
// `TrackerComms::start(...)`, que da un stream de `SocketAddr` de peers
// encontrados — mismo tipo de aproximación que el DHT, sin desglose. v1
// reporta conteo de peers únicos sin distinguir seeders/leechers; el
// protocolo BEP15 a mano (para el número real) queda de backlog explícito,
// decisión del usuario.

use std::collections::HashSet;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use librqbit_core::magnet::Magnet;
use librqbit_core::Id20;
use librqbit_tracker_comms::{TorrentStatsProvider, TrackerComms, UdpTrackerClient};
use serde::Serialize;
use tokio::sync::OnceCell;

/// Bootstrap del DHT tarda unos segundos la primera vez (ver
/// librqbit_dht::DhtState::new, confirmado en su código fuente) — timeout
/// generoso solo para no colgar la app entera si la red no responde nada.
const DHT_BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(15);
/// Ninguno de los dos streams (tracker ni DHT) termina solo — hay que
/// cortarlos a mano con un timeout y quedarse con las direcciones únicas
/// vistas en esa ventana (ver RequestPeersStream/TrackerComms::start,
/// ambos reintentan indefinidamente).
const TRACKER_TIMEOUT: Duration = Duration::from_secs(5);
const DHT_TIMEOUT: Duration = Duration::from_secs(6);
const HEALTH_CHECK_CONCURRENCY: usize = 8;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct TorrentHealth {
    /// `None` solo si no se pudo consultar ninguna fuente (magnet
    /// malformado, DHT no pudo arrancar y no había trackers). Un
    /// `Some(0)` es un resultado real: se consultó el swarm y no
    /// respondió nadie en la ventana de tiempo — no es lo mismo que "no
    /// se pudo consultar", nunca se mezclan ambos casos.
    pub peers_found: Option<u32>,
    pub source: &'static str, // "tracker" | "dht" | "sin datos"
}

/// DHT y cliente de tracker UDP compartidos, uno por proceso — bootstrapear
/// cualquiera de los dos toma segundos reales de red, no tiene sentido
/// pagarlo en cada búsqueda. Se inicializan perezosamente en el primer uso
/// real (primer `check_torrent_health_batch`), no en `setup()`. Independiente
/// del motor de torrents activo (`EmbeddedRqbit`/`ExternalQbittorrent`):
/// esto es sondeo de red aparte, nunca pasa por `TorrentEngine`.
#[derive(Default)]
pub struct TorrentHealthState {
    dht: OnceCell<librqbit_dht::Dht>,
    udp_tracker: OnceCell<UdpTrackerClient>,
}

impl TorrentHealthState {
    async fn dht(&self) -> anyhow::Result<librqbit_dht::Dht> {
        self.dht
            .get_or_try_init(|| async {
                let dht = tokio::time::timeout(DHT_BOOTSTRAP_TIMEOUT, librqbit_dht::DhtBuilder::new())
                    .await
                    .map_err(|_| anyhow::anyhow!("timeout arrancando el cliente DHT"))??;
                Ok::<_, anyhow::Error>(dht)
            })
            .await
            .map(|d| d.clone())
    }

    async fn udp_tracker(&self) -> anyhow::Result<UdpTrackerClient> {
        self.udp_tracker
            .get_or_try_init(|| UdpTrackerClient::new(tokio_util::sync::CancellationToken::new()))
            .await
            .map(|c| c.clone())
    }
}

async fn check_tracker(
    state: &TorrentHealthState,
    http: &reqwest::Client,
    info_hash: Id20,
    trackers: &[String],
) -> Option<u32> {
    let urls: HashSet<url::Url> = trackers.iter().filter_map(|t| url::Url::parse(t).ok()).collect();
    if urls.is_empty() {
        return None;
    }
    let udp_client = match state.udp_tracker().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[popcorn] no se pudo iniciar el cliente de tracker UDP: {e}");
            return None;
        }
    };
    // Peer id efímero solo para identificarnos en el announce — no
    // representa una sesión de descarga real, no hace falta que coincida
    // con el peer_id de EmbeddedRqbit.
    let peer_id = librqbit_core::peer_id::generate_azereus_style(*b"pc", (0, 1, 0, 0));
    let mut stream = TrackerComms::start(
        info_hash,
        peer_id,
        urls,
        Box::new(()) as Box<dyn TorrentStatsProvider>,
        None,
        None,
        http.clone(),
        udp_client,
    )?;

    let mut peers = HashSet::new();
    let _ = tokio::time::timeout(TRACKER_TIMEOUT, async {
        while let Some(addr) = stream.next().await {
            peers.insert(addr);
        }
    })
    .await;
    Some(peers.len() as u32)
}

async fn check_dht(state: &TorrentHealthState, info_hash: Id20) -> Option<u32> {
    let dht = match state.dht().await {
        Ok(d) => d,
        Err(e) => {
            eprintln!("[popcorn] no se pudo arrancar el cliente DHT: {e}");
            return None;
        }
    };
    let mut stream = dht.get_peers(info_hash, None);
    let mut peers = HashSet::new();
    let _ = tokio::time::timeout(DHT_TIMEOUT, async {
        while let Some(addr) = stream.next().await {
            peers.insert(addr);
        }
    })
    .await;
    Some(peers.len() as u32)
}

/// Tracker primero (más rápido si el magnet trae `tr=udp://...`), DHT
/// siempre como respaldo — cubre magnets sin tracker o donde el tracker no
/// respondió, incluso si el tracker sí respondió pero con 0 peers (el
/// swarm puede tener peers que solo anuncian por DHT). Nunca panic ante
/// magnet malformado — degrada a "sin datos" en vez de propagar error, el
/// caller (`check_torrent_health_batch`) no tiene que tratar errores por
/// ítem.
pub async fn check_health(state: &TorrentHealthState, http: &reqwest::Client, magnet: &str) -> TorrentHealth {
    let parsed = match Magnet::parse(magnet) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[popcorn] magnet malformado, sin chequeo de salud: {e}");
            return TorrentHealth { peers_found: None, source: "sin datos" };
        }
    };
    let Some(info_hash) = parsed.as_id20() else {
        eprintln!("[popcorn] magnet sin info_hash v1 (BEP15/DHT no soportan BitTorrent v2 puro), sin chequeo de salud");
        return TorrentHealth { peers_found: None, source: "sin datos" };
    };

    let tracker_result = check_tracker(state, http, info_hash, &parsed.trackers).await;
    if let Some(n) = tracker_result {
        if n > 0 {
            return TorrentHealth { peers_found: Some(n), source: "tracker" };
        }
    }
    if let Some(n) = check_dht(state, info_hash).await {
        return TorrentHealth { peers_found: Some(n), source: "dht" };
    }
    // DHT no pudo arrancar/consultar — si el tracker sí respondió (aunque
    // con 0 peers), ese dato real vale más que degradar a "sin datos".
    match tracker_result {
        Some(n) => TorrentHealth { peers_found: Some(n), source: "tracker" },
        None => TorrentHealth { peers_found: None, source: "sin datos" },
    }
}

/// Concurrencia acotada con `buffered` (no `tokio::spawn`/`JoinSet`):
/// `state`/`http` llegan como `tauri::State<'_, _>`, con lifetime atado a
/// esta invocación de comando, no `'static` — `JoinSet` exige tareas
/// `'static`. `buffered` corre hasta `HEALTH_CHECK_CONCURRENCY` futuros a
/// la vez dentro del mismo stack, sin ese requisito, y preserva el orden
/// de entrada (a diferencia de `buffer_unordered`) para que el frontend
/// pueda mergear por índice.
#[tauri::command]
pub async fn check_torrent_health_batch(
    state: tauri::State<'_, TorrentHealthState>,
    http: tauri::State<'_, crate::commands::HttpClient>,
    magnets: Vec<String>,
) -> Result<Vec<TorrentHealth>, String> {
    let health_state = state.inner();
    let http_client = http.inner().0.clone();

    let results: Vec<TorrentHealth> = stream::iter(magnets)
        .map(|magnet| {
            let http_client = http_client.clone();
            async move { check_health(health_state, &http_client, &magnet).await }
        })
        .buffered(HEALTH_CHECK_CONCURRENCY)
        .collect()
        .await;

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> TorrentHealthState {
        TorrentHealthState::default()
    }

    #[tokio::test]
    async fn malformed_magnet_reports_sin_datos_without_network() {
        let s = state();
        let http = reqwest::Client::new();
        let result = check_health(&s, &http, "no es un magnet").await;
        assert_eq!(result, TorrentHealth { peers_found: None, source: "sin datos" });
    }

    #[tokio::test]
    async fn v2_only_magnet_reports_sin_datos_without_network() {
        // BEP15 (tracker UDP) y la DHT mainline (BEP5) trabajan sobre
        // info_hash v1 (20 bytes) — un magnet solo-v2 (btmh) no tiene forma
        // de consultarse con ninguno de los dos mecanismos de este módulo.
        let s = state();
        let http = reqwest::Client::new();
        let magnet = "magnet:?xt=urn:btmh:1220caf1e1c30e81cb361b9ee167c4aa64228a7fa4fa9f6105232b28ad099f3a302e&dn=bittorrent-v2-test";
        let result = check_health(&s, &http, magnet).await;
        assert_eq!(result, TorrentHealth { peers_found: None, source: "sin datos" });
    }

    #[tokio::test]
    #[ignore = "red real: consulta un tracker UDP conocido, requiere conectividad saliente UDP"]
    async fn tracker_path_finds_real_peers_for_a_known_active_torrent() {
        let s = state();
        let http = reqwest::Client::new();
        // Sintel (Blender Foundation, dominio público) vía uno de sus
        // trackers públicos documentados — mismo contenido que ya sirve
        // `sources::blender_foundation` en este repo. Correr a mano
        // (Mandato 12): si el tracker público elegido murió, el test lo
        // dice explícito en vez de fallar en silencio (assert de source).
        let magnet = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel&tr=udp://tracker.opentrackr.org:1337/announce";
        let result = check_health(&s, &http, magnet).await;
        eprintln!("[test] tracker_path result: {result:?}");
        assert_eq!(result.source, "tracker", "el tracker público elegido puede haber muerto — revisar y actualizar la URL si esto falla");
        assert!(result.peers_found.unwrap_or(0) > 0, "swarm de Sintel debería tener peers reales");
    }

    #[tokio::test]
    #[ignore = "red real: bootstrapea la DHT mainline real, tarda varios segundos"]
    async fn dht_fallback_finds_peers_for_a_magnet_without_trackers() {
        let s = state();
        let http = reqwest::Client::new();
        // Mismo info_hash de Sintel, sin ningún `tr=` — fuerza el camino
        // DHT-only.
        let magnet = "magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel";
        let result = check_health(&s, &http, magnet).await;
        eprintln!("[test] dht_fallback result: {result:?}");
        assert_eq!(result.source, "dht");
    }
}
