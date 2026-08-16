use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::State;

use crate::db::Db;

const UPLOAD_KEY: &str = "upload_limit_kbps";
const DOWNLOAD_KEY: &str = "download_limit_kbps";

const ENGINE_KIND_KEY: &str = "active_torrent_engine";
const QBITTORRENT_BASE_URL_KEY: &str = "qbittorrent_base_url";
const QBITTORRENT_USERNAME_KEY: &str = "qbittorrent_username";
/// Id fijo en el keychain del SO para la password de qBittorrent — solo
/// existe una config de motor externo activa a la vez (v1), a diferencia de
/// los proveedores de IA que sí admiten varios guardados en simultáneo.
const QBITTORRENT_SECRET_ID: &str = "qbittorrent";
const DEFAULT_ENGINE_KIND: &str = "embedded";
pub(crate) const TORRENT_PROXY_SECRET_ID: &str = "torrent_socks_proxy_url";

#[derive(Serialize, Clone)]
pub struct SpeedLimits {
    pub upload_kbps: Option<u32>,
    pub download_kbps: Option<u32>,
}

fn read_kbps(db: &Db, key: &str) -> Result<Option<u32>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let raw: Option<String> = conn
        .query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(raw.and_then(|s| s.parse().ok()))
}

/// Bytes/seg para pasarle al motor (`TorrentEngine::add_with_limits`) — la
/// UI trabaja en KB/s, el motor en bytes/seg.
pub fn read_speed_limits_bps(db: &Db) -> Result<(Option<u32>, Option<u32>), String> {
    let download_bps = read_kbps(db, DOWNLOAD_KEY)?.map(|kbps| kbps * 1000);
    let upload_bps = read_kbps(db, UPLOAD_KEY)?.map(|kbps| kbps * 1000);
    Ok((download_bps, upload_bps))
}

#[tauri::command]
pub async fn get_speed_limits(db: State<'_, Db>) -> Result<SpeedLimits, String> {
    Ok(SpeedLimits {
        upload_kbps: read_kbps(&db, UPLOAD_KEY)?,
        download_kbps: read_kbps(&db, DOWNLOAD_KEY)?,
    })
}

/// `None` borra el límite (upsert de un NULL no tiene sentido en una tabla
/// key/value de texto — se borra la fila en vez de guardar vacío).
#[tauri::command]
pub async fn set_speed_limits(
    db: State<'_, Db>,
    upload_kbps: Option<u32>,
    download_kbps: Option<u32>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    for (key, value) in [(UPLOAD_KEY, upload_kbps), (DOWNLOAD_KEY, download_kbps)] {
        match value {
            Some(v) => conn
                .execute(
                    "INSERT INTO app_settings (key, value) VALUES (?1, ?2) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    (key, v.to_string()),
                )
                .map_err(|e| e.to_string())?,
            None => conn
                .execute("DELETE FROM app_settings WHERE key = ?1", [key])
                .map_err(|e| e.to_string())?,
        };
    }
    Ok(())
}

fn read_string(db: &Db, key: &str) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())
}

/// `None` borra la fila — mismo criterio que `set_speed_limits`.
fn write_string(conn: &rusqlite::Connection, key: &str, value: Option<String>) -> Result<(), String> {
    match value {
        Some(v) => conn
            .execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                (key, v),
            )
            .map(|_| ())
            .map_err(|e| e.to_string()),
        None => conn
            .execute("DELETE FROM app_settings WHERE key = ?1", [key])
            .map(|_| ())
            .map_err(|e| e.to_string()),
    }
}

#[derive(Serialize, Clone)]
pub struct TorrentEngineConfig {
    pub kind: String,
    pub qbittorrent_base_url: Option<String>,
    pub qbittorrent_username: Option<String>,
    /// Nunca se expone la password en sí al frontend, solo si está seteada
    /// — mismo patrón que `AiProviderConfig::has_api_key`.
    pub qbittorrent_has_password: bool,
}

/// Usado por `lib.rs::setup` al arrancar, antes de que exista cualquier
/// `State<Db>` de Tauri (todavía se está construyendo el estado) — recibe
/// la conexión ya abierta directo.
pub fn read_active_engine_kind(db: &Db) -> Result<String, String> {
    Ok(read_string(db, ENGINE_KIND_KEY)?.unwrap_or_else(|| DEFAULT_ENGINE_KIND.to_string()))
}

/// También usado por `lib.rs::setup` — arma `(base_url, username, password)`
/// para construir `ExternalQbittorrent`. `base_url` ausente es un error
/// explícito (no hay motor externo sin saber a dónde conectarse); usuario y
/// password vacíos son válidos (ej. WebUI con "bypass de autenticación para
/// clientes en localhost" activo, caso real verificado esta sesión).
pub fn read_qbittorrent_connection(db: &Db) -> Result<(String, String, String), String> {
    let base_url = read_string(db, QBITTORRENT_BASE_URL_KEY)?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "falta la URL de la WebUI de qBittorrent en la configuración".to_string())?;
    let username = read_string(db, QBITTORRENT_USERNAME_KEY)?.unwrap_or_default();
    let password = crate::keychain::get_secret(QBITTORRENT_SECRET_ID)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok((base_url, username, password))
}

#[tauri::command]
pub async fn get_torrent_engine_config(db: State<'_, Db>) -> Result<TorrentEngineConfig, String> {
    let qbittorrent_has_password = crate::keychain::get_secret(QBITTORRENT_SECRET_ID)
        .map_err(|e| e.to_string())?
        .is_some();
    Ok(TorrentEngineConfig {
        kind: read_active_engine_kind(&db)?,
        qbittorrent_base_url: read_string(&db, QBITTORRENT_BASE_URL_KEY)?,
        qbittorrent_username: read_string(&db, QBITTORRENT_USERNAME_KEY)?,
        qbittorrent_has_password,
    })
}

/// `qbittorrent_password`: `None` o vacío deja la password ya guardada tal
/// cual (el frontend nunca la recibe de vuelta, así que no puede reenviarla
/// al guardar solo un cambio de usuario/URL) — únicamente se actualiza el
/// keychain cuando llega un valor no vacío nuevo. Separada del comando
/// Tauri (mismo patrón `_inner` del resto del código) para poder probarla
/// sin necesitar un `State<'_, Db>` real.
pub(crate) fn set_torrent_engine_config_inner(
    db: &Db,
    kind: &str,
    qbittorrent_base_url: Option<String>,
    qbittorrent_username: Option<String>,
    qbittorrent_password: Option<String>,
) -> Result<(), String> {
    if !["embedded", "qbittorrent"].contains(&kind) {
        return Err(format!("motor inválido: {kind}"));
    }
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        write_string(&conn, ENGINE_KIND_KEY, Some(kind.to_string()))?;
        write_string(&conn, QBITTORRENT_BASE_URL_KEY, qbittorrent_base_url)?;
        write_string(&conn, QBITTORRENT_USERNAME_KEY, qbittorrent_username)?;
    }
    if let Some(password) = qbittorrent_password.filter(|p| !p.is_empty()) {
        crate::keychain::set_secret(QBITTORRENT_SECRET_ID, &password).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn set_torrent_engine_config(
    db: State<'_, Db>,
    kind: String,
    qbittorrent_base_url: Option<String>,
    qbittorrent_username: Option<String>,
    qbittorrent_password: Option<String>,
) -> Result<(), String> {
    set_torrent_engine_config_inner(
        &db,
        &kind,
        qbittorrent_base_url,
        qbittorrent_username,
        qbittorrent_password,
    )
}

/// Prueba de conexión desde la UI de Ajustes ("Probar conexión") — no
/// construye un `ExternalQbittorrent` completo (eso requiere el puerto y
/// los mapas del servidor de streaming compartido, ver `lib.rs::setup`),
/// solo confirma login + una llamada de lectura real contra la Web API.
/// No persiste nada: el usuario puede probar antes de guardar.
#[tauri::command]
pub async fn test_torrent_engine(base_url: String, username: String, password: String) -> Result<(), String> {
    // Validar acá, no dejar que `Qbit::new` lo reciba crudo: confirmado en
    // vivo que el crate hace `.unwrap()` interno sobre el parseo de la URL
    // (panic real con una URL vacía/inválida, tira todo el proceso).
    let url = url::Url::parse(&base_url).map_err(|e| format!("URL inválida: {e}"))?;
    let client = qbit_rs::Qbit::new(url, qbit_rs::model::Credential::new(username, password));
    client
        .login(false)
        .await
        .map_err(|e| format!("no se pudo autenticar: {e}"))?;
    client
        .get_torrent_list(qbit_rs::model::GetTorrentListArg::default())
        .await
        .map_err(|e| format!("autenticó pero falló al listar torrents: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn set_torrent_proxy_url(url: String) -> Result<(), String> {
    crate::keychain::set_secret(TORRENT_PROXY_SECRET_ID, &url).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_torrent_proxy_status() -> Result<bool, String> {
    Ok(crate::keychain::get_secret(TORRENT_PROXY_SECRET_ID)
        .map_err(|e| e.to_string())?
        .is_some())
}

#[tauri::command]
pub async fn remove_torrent_proxy_url() -> Result<(), String> {
    crate::keychain::delete_secret(TORRENT_PROXY_SECRET_ID).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn migrated_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(std::sync::Mutex::new(conn))
    }

    #[test]
    fn speed_limits_start_unset_and_round_trip_in_kbps() {
        let db = migrated_db();
        assert_eq!(read_kbps(&db, UPLOAD_KEY).unwrap(), None);

        {
            let conn = db.0.lock().unwrap();
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES (?1, ?2)",
                (UPLOAD_KEY, "500"),
            )
            .unwrap();
        }
        assert_eq!(read_kbps(&db, UPLOAD_KEY).unwrap(), Some(500));

        let (download_bps, upload_bps) = read_speed_limits_bps(&db).unwrap();
        assert_eq!(download_bps, None);
        assert_eq!(upload_bps, Some(500_000));
    }

    #[test]
    fn active_engine_kind_defaults_to_embedded_when_unset() {
        let db = migrated_db();
        assert_eq!(read_active_engine_kind(&db).unwrap(), "embedded");
    }

    /// No pasa password acá a propósito — evita tocar el keychain real del
    /// SO en un test que no está marcado `#[ignore]` (ver
    /// `keychain::tests::set_get_delete_roundtrip_against_real_os_keychain`
    /// para el test que sí lo hace).
    #[test]
    fn torrent_engine_config_round_trips_kind_and_connection_fields() {
        let db = migrated_db();
        set_torrent_engine_config_inner(
            &db,
            "qbittorrent",
            Some("http://127.0.0.1:8080".to_string()),
            Some("admin".to_string()),
            None,
        )
        .unwrap();

        assert_eq!(read_active_engine_kind(&db).unwrap(), "qbittorrent");
        assert_eq!(
            read_string(&db, QBITTORRENT_BASE_URL_KEY).unwrap(),
            Some("http://127.0.0.1:8080".to_string())
        );
        assert_eq!(
            read_string(&db, QBITTORRENT_USERNAME_KEY).unwrap(),
            Some("admin".to_string())
        );
    }

    #[test]
    fn set_torrent_engine_config_rejects_unknown_kind() {
        let db = migrated_db();
        let err = set_torrent_engine_config_inner(&db, "bittorrent-magico", None, None, None)
            .unwrap_err();
        assert!(err.contains("inválido"), "debe explicar el motor rechazado: {err}");
    }
}
