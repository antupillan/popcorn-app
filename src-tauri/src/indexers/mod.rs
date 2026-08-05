mod parse;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::{commands::try_build_active_provider, curation, StructuredQuery};
use crate::db::Db;

#[derive(Serialize, Deserialize, Clone)]
pub struct Indexer {
    pub id: String,
    pub name: String,
    pub search_url_template: String,
    pub result_format: String, // "magnet_list" | "rss" | "json"
    pub json_paths: Option<JsonPaths>,
    pub enabled: bool,
}

/// Mapeo de campos por dot-path simple (sin índices de array salvo
/// `items_path` apuntando al array de resultados) — no es JSONPath
/// completo, alcanza para la enorme mayoría de APIs de búsqueda reales.
#[derive(Serialize, Deserialize, Clone)]
pub struct JsonPaths {
    pub items_path: String, // "" = la respuesta ya es el array
    pub title_field: String,
    pub magnet_field: String,
    pub size_field: Option<String>,
    pub seeders_field: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct IndexerResult {
    pub title: String,
    pub magnet: String,
    pub size: Option<String>,
    pub seeders: Option<String>,
    pub source_indexer: String,
}

fn row_to_indexer(row: &rusqlite::Row) -> rusqlite::Result<Indexer> {
    let json_paths_raw: Option<String> = row.get(4)?;
    Ok(Indexer {
        id: row.get(0)?,
        name: row.get(1)?,
        search_url_template: row.get(2)?,
        result_format: row.get(3)?,
        json_paths: json_paths_raw.and_then(|s| serde_json::from_str(&s).ok()),
        enabled: row.get::<_, i64>(5)? != 0,
    })
}

#[tauri::command]
pub async fn list_indexers(db: State<'_, Db>) -> Result<Vec<Indexer>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, search_url_template, result_format, json_paths, enabled \
             FROM indexers ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], row_to_indexer)
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_indexer(
    db: State<'_, Db>,
    name: String,
    search_url_template: String,
    result_format: String,
    json_paths: Option<JsonPaths>,
) -> Result<Indexer, String> {
    if !["magnet_list", "rss", "json"].contains(&result_format.as_str()) {
        return Err(format!("result_format inválido: {result_format}"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let json_paths_raw = json_paths
        .as_ref()
        .map(|p| serde_json::to_string(p))
        .transpose()
        .map_err(|e| e.to_string())?;
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO indexers (id, name, search_url_template, result_format, json_paths) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (&id, &name, &search_url_template, &result_format, &json_paths_raw),
        )
        .map_err(|e| e.to_string())?;
        // Toda fuente de búsqueda necesita su fila de curación (ver
        // source_settings) — sin esto, list_source_settings no la lista.
        conn.execute("INSERT INTO source_settings (id) VALUES (?1)", [&id])
            .map_err(|e| e.to_string())?;
    }
    Ok(Indexer {
        id,
        name,
        search_url_template,
        result_format,
        json_paths,
        enabled: true,
    })
}

#[tauri::command]
pub async fn remove_indexer(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM indexers WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    // source_settings no tiene FK real a indexers (ver migración) — el
    // borrado en cascada se hace a mano acá.
    conn.execute("DELETE FROM source_settings WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn toggle_indexer(db: State<'_, Db>, id: String, enabled: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE indexers SET enabled = ?1 WHERE id = ?2",
        (enabled as i64, &id),
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Corre una query contra un indexer que todavía no se guardó — para que
/// el usuario pueda validar la plantilla antes de confirmar el alta.
#[tauri::command]
pub async fn test_indexer(
    http: State<'_, crate::commands::HttpClient>,
    indexer: Indexer,
    query: String,
) -> Result<Vec<IndexerResult>, String> {
    search_one(&http.0, &indexer, &query)
        .await
        .map_err(|e| e.to_string())
}

/// Lee `source_settings.curation_enabled` para una fuente puntual (acá
/// siempre un `indexers.id`) — curación es config por fuente, no global, así
/// que se consulta por cada indexer en vez de una sola vez para todos.
fn curation_enabled_for(db: &Db, source_id: &str) -> Result<bool, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let enabled: i64 = conn
        .query_row(
            "SELECT curation_enabled FROM source_settings WHERE id = ?1",
            [source_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(enabled != 0)
}

/// Despacha la query contra todos los indexers habilitados del usuario y
/// fusiona resultados. Un indexer que falla no tira abajo a los demás —
/// se omite y sigue con el resto. Curación por IA (si el indexer la tiene
/// activada y hay un proveedor activo — fail-open si no) se aplica por
/// indexer antes de fusionar, no sobre el resultado ya mezclado.
#[tauri::command]
pub async fn search_indexers(
    http: State<'_, crate::commands::HttpClient>,
    db: State<'_, Db>,
    query: String,
) -> Result<Vec<IndexerResult>, String> {
    let enabled: Vec<Indexer> = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, name, search_url_template, result_format, json_paths, enabled \
                 FROM indexers WHERE enabled = 1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], row_to_indexer)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    let provider = try_build_active_provider(&db)?;
    let sq = StructuredQuery { title: query.clone(), ..Default::default() };

    let mut all = Vec::new();
    for indexer in &enabled {
        match search_one(&http.0, indexer, &query).await {
            Ok(mut results) => {
                if let Some(provider) = &provider {
                    if curation_enabled_for(&db, &indexer.id)? {
                        results = curation::curate(provider.as_ref(), &sq, results, |r| r.title.as_str()).await;
                    }
                }
                all.append(&mut results);
            }
            Err(e) => eprintln!("[popcorn] indexer '{}' falló: {e}", indexer.name),
        }
    }
    Ok(all)
}

async fn search_one(
    client: &reqwest::Client,
    indexer: &Indexer,
    query: &str,
) -> anyhow::Result<Vec<IndexerResult>> {
    let url = indexer
        .search_url_template
        .replace("{query}", &urlencoding::encode(query));
    let body = crate::http_retry::send_with_retry(|| client.get(&url))
        .await
        .with_context(|| format!("no se pudo contactar {url}"))?
        .text()
        .await?;

    let mut results = match indexer.result_format.as_str() {
        "magnet_list" => parse::magnet_list(&body),
        "rss" => parse::rss(&body),
        "json" => parse::json(&body, indexer.json_paths.as_ref())?,
        other => anyhow::bail!("formato de indexer desconocido: {other}"),
    };
    for r in &mut results {
        r.source_indexer = indexer.name.clone();
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "red real, no apto para CI por defecto — correr manualmente para verificar contra un indexer BYO real"]
    async fn search_one_against_real_nyaa_rss_returns_real_results() {
        let client = reqwest::Client::new();
        let indexer = Indexer {
            id: "test".to_string(),
            name: "Nyaa (test)".to_string(),
            search_url_template: "https://nyaa.si/?page=rss&c=1_2&f=0&q={query}".to_string(),
            result_format: "rss".to_string(),
            json_paths: None,
            enabled: true,
        };

        let results = search_one(&client, &indexer, "one piece")
            .await
            .expect("nyaa.si debe responder con un feed RSS parseable");

        assert!(!results.is_empty(), "una query popular no debería devolver cero resultados");
        for r in &results {
            assert!(r.magnet.starts_with("magnet:?xt=urn:btih:"));
            assert!(!r.title.is_empty());
            assert_eq!(r.source_indexer, "Nyaa (test)");
        }
    }
}
