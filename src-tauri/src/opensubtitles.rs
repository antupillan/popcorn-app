// Parte B del plan de subtítulos (buscar/descargar desde OpenSubtitles),
// ver Planes_mejora_popcorn/subtitulos_ia.txt. Verificado contra la
// documentación real y una implementación de referencia real (no
// inventado, Mandato 4): dos credenciales distintas — Api-Key de
// aplicación (header `Api-Key`) + login usuario/contraseña -> JWT (header
// `Authorization: Bearer`, requerido para /download). Sin cuenta: 5
// descargas/día; con cuenta gratis: 20/día. Búsqueda sin límite.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::keychain;

const API_BASE: &str = "https://www.opensubtitles.com/api/v1";

/// OpenSubtitles exige este header con formato "AppName vX.Y.Z" — sin él
/// (o mal formado) responde 403 en todos los endpoints, confirmado en vivo
/// (ver foro oficial: forum.opensubtitles.com/t/rest-api-always-give-403-even-user-agent-sent/2623).
/// Deriva de `CARGO_PKG_VERSION` (Cargo.toml) para no mantener un número de
/// versión duplicado y potencialmente desincronizado.
const USER_AGENT: &str = concat!("Popcorn v", env!("CARGO_PKG_VERSION"));

/// Api-Key de **aplicación** — identifica a Popcorn ante OpenSubtitles, no
/// es un secreto de usuario (mismo espíritu que un bundle identifier, ver
/// plan). Se inyecta en tiempo de compilación vía la variable de entorno
/// `POPCORN_OPENSUBTITLES_API_KEY` (build-time/CI, Mandato 5) — nunca como
/// literal en este archivo, para no dejarla en texto plano en el historial
/// de git una vez que el repo tenga remoto. Mientras esté vacía, los
/// comandos de acá fallan explícito en vez de mandar una key falsa a la
/// API real.
const APPLICATION_API_KEY: &str = match option_env!("POPCORN_OPENSUBTITLES_API_KEY") {
    Some(key) => key,
    None => "",
};

const USERNAME_SECRET_ID: &str = "opensubtitles_username";
const PASSWORD_SECRET_ID: &str = "opensubtitles_password";

fn require_application_key() -> Result<&'static str, String> {
    require_application_key_from(APPLICATION_API_KEY)
}

// Separado de `require_application_key` para poder testear la rama vacía
// sin depender de que `POPCORN_OPENSUBTITLES_API_KEY` esté ausente al
// compilar los tests — si un dev (o CI) la tiene seteada en el entorno,
// `APPLICATION_API_KEY` deja de estar vacía y ese test dejaría de probar
// lo que dice probar (bug real, visto en vivo al correr el test de red
// real de esta misma sesión con la key exportada).
fn require_application_key_from(key: &'static str) -> Result<&'static str, String> {
    if key.is_empty() {
        return Err(
            "OpenSubtitles no está configurado todavía — falta que el mantenedor de Popcorn registre \
             la aplicación en opensubtitles.com y agregue la Api-Key real."
                .to_string(),
        );
    }
    Ok(key)
}

#[derive(Serialize, Clone)]
pub struct OpenSubtitlesResult {
    pub file_id: i64,
    pub file_name: String,
}

#[derive(Deserialize)]
struct LoginResponse {
    token: String,
}

/// Separado del fetch para poder testear el parseo contra una fixture real
/// sin necesitar credenciales — mismo criterio que `parse_response_body`
/// en `ai/gemini.rs`.
fn parse_login_response(body: &str) -> anyhow::Result<String> {
    let parsed: LoginResponse = serde_json::from_str(body).context("respuesta de /login no es el JSON esperado")?;
    Ok(parsed.token)
}

async fn login(client: &reqwest::Client, username: &str, password: &str) -> anyhow::Result<String> {
    let api_key = require_application_key().map_err(|e| anyhow::anyhow!(e))?;
    let resp = crate::http_retry::send_with_retry(|| {
        client
            .post(format!("{API_BASE}/login"))
            .header("Api-Key", api_key)
            .header("User-Agent", USER_AGENT)
            .json(&serde_json::json!({"username": username, "password": password}))
    })
    .await
    .context("no se pudo contactar OpenSubtitles (login)")?;
    if !resp.status().is_success() {
        anyhow::bail!("OpenSubtitles devolvió {} al iniciar sesión — revisa tu usuario y contraseña", resp.status());
    }
    let body = resp.text().await.context("no se pudo leer la respuesta de login")?;
    parse_login_response(&body)
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Vec<SearchDatum>,
}
#[derive(Deserialize)]
struct SearchDatum {
    attributes: SearchAttributes,
}
#[derive(Deserialize)]
struct SearchAttributes {
    files: Vec<SearchFile>,
}
#[derive(Deserialize)]
struct SearchFile {
    file_id: i64,
    file_name: String,
}

/// Separado del fetch para poder testear el parseo con una fixture real.
/// Nota honesta (Mandato 4): solo se extraen `file_id`/`file_name`,
/// confirmados contra una implementación de referencia real — otros
/// campos de metadata (idioma, release) que la API probablemente también
/// trae no se incluyen porque no se pudo verificar su nombre exacto en
/// esta sesión, agregarlos a ciegas hubiera sido adivinar el contrato.
fn parse_search_response(body: &str) -> anyhow::Result<Vec<OpenSubtitlesResult>> {
    let parsed: SearchResponse =
        serde_json::from_str(body).context("respuesta de /subtitles no es el JSON esperado")?;
    Ok(parsed
        .data
        .into_iter()
        .flat_map(|d| d.attributes.files)
        .map(|f| OpenSubtitlesResult { file_id: f.file_id, file_name: f.file_name })
        .collect())
}

async fn search_opensubtitles_inner(
    client: &reqwest::Client,
    api_key: &str,
    query: &str,
    language: &str,
) -> Result<Vec<OpenSubtitlesResult>, String> {
    let resp = crate::http_retry::send_with_retry(|| {
        client
            .get(format!("{API_BASE}/subtitles"))
            .header("Api-Key", api_key)
            .header("User-Agent", USER_AGENT)
            .query(&[("query", query), ("languages", language)])
    })
    .await
    .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("OpenSubtitles devolvió {} al buscar", resp.status()));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    parse_search_response(&body).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_opensubtitles(
    http: State<'_, crate::commands::HttpClient>,
    query: String,
    language: String,
) -> Result<Vec<OpenSubtitlesResult>, String> {
    let api_key = require_application_key()?;
    search_opensubtitles_inner(&http.0, api_key, &query, &language).await
}

#[derive(Deserialize)]
struct DownloadResponse {
    link: String,
}

fn parse_download_response(body: &str) -> anyhow::Result<String> {
    let parsed: DownloadResponse =
        serde_json::from_str(body).context("respuesta de /download no es el JSON esperado")?;
    Ok(parsed.link)
}

/// Devuelve el contenido de texto del subtítulo ya descargado — guardarlo
/// como fila real en `subtitles` (origin: "original") es responsabilidad
/// del frontend, vía el comando `add_subtitle_text` ya existente (Parte A)
/// en vez de duplicar esa lógica acá.
#[tauri::command]
pub async fn download_opensubtitles_subtitle(
    http: State<'_, crate::commands::HttpClient>,
    file_id: i64,
) -> Result<String, String> {
    let api_key = require_application_key()?;
    let username = keychain::get_secret(USERNAME_SECRET_ID)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "configura tu usuario y contraseña de OpenSubtitles en Ajustes".to_string())?;
    let password = keychain::get_secret(PASSWORD_SECRET_ID)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "configura tu usuario y contraseña de OpenSubtitles en Ajustes".to_string())?;
    let token = login(&http.0, &username, &password).await.map_err(|e| e.to_string())?;

    let resp = crate::http_retry::send_with_retry(|| {
        http.0
            .post(format!("{API_BASE}/download"))
            .header("Api-Key", api_key)
            .header("User-Agent", USER_AGENT)
            .bearer_auth(&token)
            .json(&serde_json::json!({"file_id": file_id}))
    })
    .await
    .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "OpenSubtitles devolvió {} al pedir el link de descarga — puede que se haya agotado tu cuota diaria",
            resp.status()
        ));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    let link = parse_download_response(&body).map_err(|e| e.to_string())?;

    let file_resp = crate::http_retry::send_with_retry(|| http.0.get(&link)).await.map_err(|e| e.to_string())?;
    if !file_resp.status().is_success() {
        return Err(format!("no se pudo descargar el archivo de subtítulo ({})", file_resp.status()));
    }
    file_resp.text().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_opensubtitles_credentials(username: String, password: String) -> Result<(), String> {
    keychain::set_secret(USERNAME_SECRET_ID, &username).map_err(|e| e.to_string())?;
    keychain::set_secret(PASSWORD_SECRET_ID, &password).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_opensubtitles_credentials_status() -> Result<bool, String> {
    Ok(keychain::get_secret(USERNAME_SECRET_ID).map_err(|e| e.to_string())?.is_some())
}

#[tauri::command]
pub async fn remove_opensubtitles_credentials() -> Result<(), String> {
    keychain::delete_secret(USERNAME_SECRET_ID).map_err(|e| e.to_string())?;
    keychain::delete_secret(PASSWORD_SECRET_ID).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_login_response_extracts_token() {
        let body = r#"{"user": {"id": 1}, "token": "eyJabc123", "status": 200}"#;
        assert_eq!(parse_login_response(body).unwrap(), "eyJabc123");
    }

    #[test]
    fn parse_login_response_errors_on_missing_token() {
        assert!(parse_login_response(r#"{"status": 200}"#).is_err());
    }

    #[test]
    fn parse_search_response_extracts_file_id_and_name_from_real_shaped_response() {
        let body = r#"{
            "data": [
                {
                    "attributes": {
                        "files": [
                            {"file_id": 12345, "file_name": "Sintel.2010.srt"}
                        ]
                    }
                },
                {
                    "attributes": {
                        "files": [
                            {"file_id": 67890, "file_name": "Sintel.alt.srt"}
                        ]
                    }
                }
            ]
        }"#;
        let results = parse_search_response(body).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].file_id, 12345);
        assert_eq!(results[0].file_name, "Sintel.2010.srt");
    }

    #[test]
    fn parse_search_response_handles_empty_results() {
        assert_eq!(parse_search_response(r#"{"data": []}"#).unwrap().len(), 0);
    }

    /// Red real, deshabilitado por defecto. Corre con
    /// `POPCORN_OPENSUBTITLES_API_KEY=<key> cargo test --lib opensubtitles::tests::search_with_real_application_key -- --ignored`
    /// — confirma que la Api-Key de aplicación recién registrada funciona
    /// contra la API real, no solo que el código compila (Mandato 12).
    #[tokio::test]
    #[ignore]
    async fn search_with_real_application_key_returns_results() {
        let api_key = require_application_key()
            .expect("configura POPCORN_OPENSUBTITLES_API_KEY antes de correr este test");
        let client = reqwest::Client::new();
        let results = search_opensubtitles_inner(&client, api_key, "Sintel", "es")
            .await
            .expect("la búsqueda contra la API real falló");
        assert!(!results.is_empty(), "esperaba al menos un resultado para 'Sintel'");
    }

    #[test]
    fn parse_download_response_extracts_link() {
        let body = r#"{"link": "https://example.com/download/xyz.srt", "requests": 1}"#;
        assert_eq!(parse_download_response(body).unwrap(), "https://example.com/download/xyz.srt");
    }

    #[test]
    fn require_application_key_fails_explicitly_while_unconfigured() {
        // Documenta el estado real: sin Api-Key registrada, todo comando
        // de este módulo falla explícito en vez de intentar una llamada
        // que de todos modos rebotaría con 401/403 de forma menos clara.
        // Prueba la rama vacía explícito vía `_from`, no `require_application_key()`
        // directo — esa depende de `APPLICATION_API_KEY`, resuelta en
        // compilación desde `POPCORN_OPENSUBTITLES_API_KEY`; si esa variable
        // está seteada al compilar (como en esta sesión, con la key real),
        // el test dejaría de probar el caso vacío.
        assert!(require_application_key_from("").is_err());
    }
}
