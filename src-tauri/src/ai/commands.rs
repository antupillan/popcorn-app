use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::State;

use super::{gemini::GeminiProvider, keychain, openai_compatible::OpenAiCompatibleProvider, AiProvider, StructuredQuery};
use crate::db::Db;

#[derive(Serialize, Clone)]
pub struct AiProviderConfig {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub model: String,
    pub base_url: Option<String>,
    pub active: bool,
    /// Nunca se expone la key en sí al frontend, solo si está seteada.
    pub has_api_key: bool,
}

fn row_to_config(row: &rusqlite::Row) -> rusqlite::Result<AiProviderConfig> {
    let id: String = row.get(0)?;
    let has_api_key = keychain::get_api_key(&id).ok().flatten().is_some();
    Ok(AiProviderConfig {
        id,
        kind: row.get(1)?,
        label: row.get(2)?,
        model: row.get(3)?,
        base_url: row.get(4)?,
        active: row.get::<_, i64>(5)? != 0,
        has_api_key,
    })
}

#[tauri::command]
pub async fn list_ai_providers(db: State<'_, Db>) -> Result<Vec<AiProviderConfig>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, label, model, base_url, active \
             FROM ai_providers ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_config).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_ai_provider(
    db: State<'_, Db>,
    kind: String,
    label: String,
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<AiProviderConfig, String> {
    if !["gemini", "openai_compatible"].contains(&kind.as_str()) {
        return Err(format!("kind inválido: {kind}"));
    }
    if kind == "openai_compatible" && base_url.as_deref().unwrap_or("").is_empty() {
        return Err("openai_compatible requiere base_url (ej. http://localhost:11434/v1 para Ollama)".to_string());
    }
    let id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO ai_providers (id, kind, label, model, base_url) VALUES (?1, ?2, ?3, ?4, ?5)",
            (&id, &kind, &label, &model, &base_url),
        )
        .map_err(|e| e.to_string())?;
    }
    if let Some(key) = api_key.as_ref().filter(|k| !k.is_empty()) {
        keychain::set_api_key(&id, key).map_err(|e| e.to_string())?;
    }
    Ok(AiProviderConfig {
        has_api_key: keychain::get_api_key(&id).ok().flatten().is_some(),
        id,
        kind,
        label,
        model,
        base_url,
        active: false,
    })
}

#[tauri::command]
pub async fn remove_ai_provider(db: State<'_, Db>, id: String) -> Result<(), String> {
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM ai_providers WHERE id = ?1", [&id])
            .map_err(|e| e.to_string())?;
    }
    keychain::delete_api_key(&id).map_err(|e| e.to_string())?;
    Ok(())
}

/// Un solo proveedor activo a la vez (v1) — el que usa `parse_query`.
#[tauri::command]
pub async fn set_active_ai_provider(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("UPDATE ai_providers SET active = 0", [])
        .map_err(|e| e.to_string())?;
    let changed = conn
        .execute("UPDATE ai_providers SET active = 1 WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("no existe un proveedor con id {id}"));
    }
    Ok(())
}

pub(crate) fn build_provider(
    kind: &str,
    model: String,
    base_url: Option<String>,
    api_key: Option<String>,
) -> anyhow::Result<Box<dyn AiProvider>> {
    match kind {
        "gemini" => {
            let key = api_key
                .ok_or_else(|| anyhow::anyhow!("el proveedor Gemini activo no tiene API key en el keychain"))?;
            Ok(Box::new(GeminiProvider::new(key, model)))
        }
        "openai_compatible" => {
            let base_url = base_url
                .ok_or_else(|| anyhow::anyhow!("el proveedor openai_compatible activo no tiene base_url"))?;
            Ok(Box::new(OpenAiCompatibleProvider::new(base_url, api_key, model)))
        }
        other => anyhow::bail!("kind de proveedor de IA desconocido: {other}"),
    }
}

/// Resuelve el proveedor de IA activo del usuario (config + key del
/// keychain) y lo instancia. `Ok(None)` significa "no hay proveedor activo"
/// — no es un error, lo usan llamadores fail-open como la curación de
/// resultados (ver `curation::curate`); `parse_query` sí lo trata como error
/// porque ahí la IA es el propósito del comando, no un enriquecimiento.
pub(crate) fn try_build_active_provider(db: &Db) -> Result<Option<Box<dyn AiProvider>>, String> {
    let row: Option<(String, String, String, Option<String>)> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT id, kind, model, base_url FROM ai_providers WHERE active = 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
    };
    let Some((id, kind, model, base_url)) = row else {
        return Ok(None);
    };
    let api_key = keychain::get_api_key(&id).map_err(|e| e.to_string())?;
    build_provider(&kind, model, base_url, api_key)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Interpreta una búsqueda en lenguaje natural con el proveedor de IA activo
/// del usuario. Nunca sugiere sitios/indexers — ver PARSE_QUERY_SYSTEM_PROMPT,
/// fijo por la app sin importar qué proveedor lo ejecute (Mandato 8).
#[tauri::command]
pub async fn parse_query(db: State<'_, Db>, text: String) -> Result<StructuredQuery, String> {
    let provider = try_build_active_provider(&db)?
        .ok_or_else(|| "no hay ningún proveedor de IA activo — configurá uno en Ajustes".to_string())?;
    provider
        .parse_query(&text)
        .await
        .map_err(|e| format!("[{}] {e}", provider.name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_provider_rejects_unknown_kind() {
        assert!(build_provider("mistral-direct", "m".into(), None, None).is_err());
    }

    #[test]
    fn build_provider_gemini_requires_api_key() {
        assert!(build_provider("gemini", "gemini-2.5-flash".into(), None, None).is_err());
        assert!(build_provider("gemini", "gemini-2.5-flash".into(), None, Some("key".into())).is_ok());
    }

    #[test]
    fn build_provider_openai_compatible_requires_base_url_but_not_api_key() {
        assert!(build_provider("openai_compatible", "llama3".into(), None, None).is_err());
        assert!(build_provider(
            "openai_compatible",
            "llama3".into(),
            Some("http://localhost:11434/v1".into()),
            None
        )
        .is_ok());
    }
}
