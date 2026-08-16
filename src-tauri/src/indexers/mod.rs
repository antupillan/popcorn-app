mod parse;
pub mod torrent_health;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::{commands::try_build_active_provider, AiProvider};
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

fn enabled_indexers(db: &Db) -> Result<Vec<Indexer>, String> {
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
    Ok(rows)
}

/// Despacha la query contra todos los indexers habilitados del usuario y
/// fusiona resultados. Un indexer que falla no tira abajo a los demás —
/// se omite y sigue con el resto. Sin curación por IA a propósito
/// (Mandato, aclarado en vivo por el usuario): curación es para ordenar/
/// filtrar un catálogo ya cargado en un grid (archive.org, YouTube, IPTV),
/// no para una búsqueda puntual que el usuario tipea — mandar cada
/// resultado de acá a un proveedor de IA en cada búsqueda solo agregaba
/// latencia real (visto en vivo: Nyaa devuelve 20-75 ítems por página,
/// todos en una sola llamada síncrona) sin que el usuario lo hubiera
/// pedido. Buscar "más amplio" con IA es una acción aparte y explícita
/// (ver `search_indexers_with_ai`), no algo que la búsqueda cruda haga
/// por su cuenta.
#[tauri::command]
pub async fn search_indexers(
    http: State<'_, crate::commands::HttpClient>,
    db: State<'_, Db>,
    query: String,
) -> Result<Vec<IndexerResult>, String> {
    let enabled = enabled_indexers(&db)?;
    let mut all = Vec::new();
    for indexer in &enabled {
        match search_one(&http.0, indexer, &query).await {
            Ok(mut results) => all.append(&mut results),
            Err(e) => eprintln!("[popcorn] indexer '{}' falló: {e}", indexer.name),
        }
    }
    Ok(all)
}

/// Separado del comando para poder testear sin `State` (mismo patrón que
/// `search_added_content_with_ai_inner` en `ai/commands.rs`). Acción
/// explícita del usuario (no un enriquecimiento pasivo como la curación
/// de grids) — falla claro sin proveedor activo, no cae en silencio a la
/// búsqueda cruda. La query original siempre entra al set de términos,
/// nunca depende 100% de lo que devuelva la IA. Dedup por `magnet`: el
/// mismo torrent puede aparecer con varios términos distintos.
async fn search_indexers_with_ai_inner(
    http: &reqwest::Client,
    provider: Option<&dyn AiProvider>,
    enabled: &[Indexer],
    query: &str,
) -> Result<Vec<IndexerResult>, String> {
    let provider = provider
        .ok_or_else(|| "no hay ningún proveedor de IA activo — configura uno en Ajustes".to_string())?;
    let broadened = provider.broaden_query(query).await.map_err(|e| format!("[{}] {e}", provider.name()))?;

    let mut terms = vec![query.to_string()];
    for t in broadened {
        if !terms.contains(&t) {
            terms.push(t);
        }
    }

    let mut seen = std::collections::HashSet::new();
    let mut all = Vec::new();
    for indexer in enabled {
        for term in &terms {
            match search_one(http, indexer, term).await {
                Ok(results) => {
                    for r in results {
                        if seen.insert(r.magnet.clone()) {
                            all.push(r);
                        }
                    }
                }
                Err(e) => eprintln!("[popcorn] indexer '{}' falló (término '{term}'): {e}", indexer.name),
            }
        }
    }
    Ok(all)
}

#[tauri::command]
pub async fn search_indexers_with_ai(
    http: State<'_, crate::commands::HttpClient>,
    db: State<'_, Db>,
    query: String,
) -> Result<Vec<IndexerResult>, String> {
    let enabled = enabled_indexers(&db)?;
    let provider = try_build_active_provider(&db)?;
    search_indexers_with_ai_inner(&http.0, provider.as_deref(), &enabled, &query).await
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

    #[tokio::test]
    async fn search_indexers_with_ai_fails_explicitly_without_active_provider() {
        let client = reqwest::Client::new();
        let result = search_indexers_with_ai_inner(&client, None, &[], "one piece").await;
        assert!(result.is_err(), "sin proveedor activo debe fallar explícito, no caer en silencio a la búsqueda cruda");
    }

    #[tokio::test]
    async fn search_indexers_with_ai_returns_empty_without_indexers_but_still_calls_the_provider() {
        struct FakeProvider;
        #[async_trait::async_trait]
        impl AiProvider for FakeProvider {
            fn name(&self) -> &'static str {
                "fake"
            }
            async fn parse_query(&self, _text: &str) -> anyhow::Result<crate::ai::StructuredQuery> {
                unreachable!("no lo usa este test")
            }
            async fn curate_results(
                &self,
                _query: &crate::ai::StructuredQuery,
                _candidates: &[String],
            ) -> anyhow::Result<Vec<usize>> {
                unreachable!("no lo usa este test")
            }
            async fn curate_by_hint(
                &self,
                _candidates: &[String],
                _hint: Option<&str>,
            ) -> anyhow::Result<Vec<crate::ai::ScoredCandidate>> {
                unreachable!("no lo usa este test")
            }
            async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
                unreachable!("no lo usa este test")
            }
            async fn broaden_query(&self, query: &str) -> anyhow::Result<Vec<String>> {
                assert_eq!(query, "one piece");
                Ok(vec!["ワンピース".to_string()])
            }
        }

        let client = reqwest::Client::new();
        let result = search_indexers_with_ai_inner(&client, Some(&FakeProvider), &[], "one piece")
            .await
            .expect("con proveedor activo, sin indexers habilitados no debe fallar");
        assert!(result.is_empty(), "sin indexers habilitados no hay dónde buscar los términos ampliados");
    }
}
