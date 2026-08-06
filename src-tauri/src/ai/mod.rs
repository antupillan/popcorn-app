pub mod commands;
pub mod curation;
pub mod gemini;
pub mod keychain;
pub mod openai_compatible;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Resultado de interpretar una búsqueda en lenguaje natural. Nunca incluye
/// URLs ni nombres de sitios/indexers — el proveedor de IA solo extrae
/// parámetros de búsqueda; el usuario ya decidió contra qué indexers propios
/// correrlos (ver Mandato de indexers tipo Jackett/Prowlarr en el plan).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct StructuredQuery {
    pub title: String,
    pub year: Option<i32>,
    pub genre: Option<String>,
}

/// Abstracción agnóstica de proveedor de IA (Mandato 8) — mismo patrón que
/// `TorrentEngine`. Cada implementación usa la(s) API key(s) propia(s) del
/// usuario desde `keychain`, o ninguna para un proveedor local sin key.
#[async_trait]
pub trait AiProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn parse_query(&self, text: &str) -> anyhow::Result<StructuredQuery>;
    /// Filtra/reordena resultados de búsqueda ya normalizados por relevancia
    /// contra `query`. Devuelve índices (base 0) sobre `candidates`, en orden
    /// de relevancia — nunca los ítems en sí, para que la misma capa sirva
    /// tanto a `ArchiveOrgItem` como a `IndexerResult` sin lógica por fuente
    /// (ver plan, decisión "curación por proveedor de búsqueda").
    async fn curate_results(
        &self,
        query: &StructuredQuery,
        candidates: &[String],
    ) -> anyhow::Result<Vec<usize>>;
    /// Igual forma que `curate_results` (índices base 0, orden de
    /// relevancia) pero sin `query` que matchear — `hint` es el criterio en
    /// texto libre que define quién manda: para una fuente semilla (ej.
    /// `iptv_org_public`) es la app la que lo fija ("radiodifusores públicos
    /// oficiales"); para una fuente BYO que el usuario agregó él mismo, el
    /// criterio es el que el usuario haya escrito (o ninguno). La app nunca
    /// impone su propio juicio de legitimidad sobre una lista que el usuario
    /// eligió agregar — mismo principio que `indexers` (cero filas semilla,
    /// contenido BYO es responsabilidad de quien lo agrega). Sirve tanto a
    /// canales IPTV como a catálogo Online (Biblioteca unificada) — el
    /// criterio vive enteramente en `hint`, no en el prompt fijo. Ver
    /// `CURATE_BY_HINT_SYSTEM_PROMPT`.
    async fn curate_by_hint(
        &self,
        candidates: &[String],
        hint: Option<&str>,
    ) -> anyhow::Result<Vec<usize>>;
}

/// Prompt fijo por la app (no editable por el usuario ni por config) —
/// extrae parámetros de búsqueda, nunca sugiere sitios o indexers. Se
/// reutiliza igual sin importar qué proveedor lo ejecute.
pub(crate) const PARSE_QUERY_SYSTEM_PROMPT: &str = "Sos un extractor de parámetros de búsqueda de video (películas, series, documentales). \
A partir del texto en lenguaje natural del usuario, devolvé ÚNICAMENTE un objeto JSON con esta forma exacta, sin texto adicional: \
{\"title\": string, \"year\": number|null, \"genre\": string|null}. \
`title` es el título del contenido tal como lo escribiría el usuario, sin el año ni el género incluidos. \
Nunca incluyas URLs, nombres de sitios web, nombres de indexers, ni sugerencias de dónde buscar — tu única tarea es extraer estos tres campos.";

/// Prompt fijo por la app para la curación de resultados (Mandato 8) — el
/// mismo criterio sin importar la fuente (archive.org o un indexer BYO), ver
/// `curation::curate`. Pide un objeto (no un array suelto) porque el modo
/// JSON forzado de la API real de OpenAI exige que la respuesta sea un
/// objeto `{...}` — un array top-level la rechaza.
pub(crate) const CURATE_RESULTS_SYSTEM_PROMPT: &str = "Sos un filtro de relevancia para resultados de búsqueda de video (películas, series, documentales). \
Se te da una búsqueda y una lista numerada de títulos candidatos tal como aparecen en un índice real, con ruido: uploads no relacionados, duplicados, contenido ajeno al pedido. \
Devolvé ÚNICAMENTE un objeto JSON con esta forma exacta, sin texto adicional: {\"indices\": [number, ...]}. \
`indices` son los índices (enteros, base 0) de los candidatos que son coincidencias razonables para la búsqueda, ordenados del más al menos relevante. \
No inventes índices que no estén en la lista de candidatos. Si ningún candidato es relevante, devolvé {\"indices\": []}. \
Nunca agregues texto, explicación ni markdown fuera del objeto JSON.";

/// Arma el texto de usuario para la llamada de curación: búsqueda + lista
/// numerada de candidatos, mismo formato para cualquier proveedor.
pub(crate) fn build_curation_user_text(query: &StructuredQuery, candidates: &[String]) -> String {
    let list = candidates
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{i}: {t}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("Búsqueda: {}\n\nCandidatos:\n{list}", query.title)
}

/// Prompt fijo para curar por criterio en texto libre (Mandato 8) — sirve
/// tanto a canales IPTV como a catálogo Online (Biblioteca unificada), a
/// diferencia de `CURATE_RESULTS_SYSTEM_PROMPT` no hay búsqueda que
/// matchear, hay un `hint` que define el criterio: cuando lo da la app
/// (fuente semilla, ej. `iptv_org_public`) es su propio criterio de
/// legitimidad/calidad; cuando lo da el usuario (fuente BYO) es lo que el
/// usuario haya escrito. El prompt en sí queda fijo — el criterio variable
/// vive en el mensaje de usuario (`build_hint_curation_user_text`), igual
/// que la búsqueda ya viaja en el mensaje de usuario para `curate_results`,
/// nunca en el prompt de sistema. Mismo contrato `{"indices": [...]}` que
/// `CURATE_RESULTS_SYSTEM_PROMPT` por la misma razón (modo JSON forzado de
/// OpenAI exige objeto, no array).
pub(crate) const CURATE_BY_HINT_SYSTEM_PROMPT: &str = "Sos un filtro de curación para listas de contenido agregadas de internet (canales de TV en vivo o catálogo de video), que mezclan entradas legítimas con basura, duplicados, entradas rotas o de dudosa procedencia. \
Se te da un criterio de curación y una lista numerada de candidatos (nombre de canal o título, con categoría/género entre paréntesis cuando está disponible). \
Devolvé ÚNICAMENTE un objeto JSON con esta forma exacta, sin texto adicional: {\"indices\": [number, ...]}. \
`indices` son los índices (enteros, base 0) de los candidatos que cumplen el criterio dado, ordenados del más al menos ajustado a ese criterio. \
Si el criterio pide verificar legitimidad como radiodifusor público/oficial, juzgalo por el nombre y la categoría — excluí spam, placeholders, duplicados evidentes y canales comerciales/privados sin relación con radiodifusión pública. \
Si el criterio pide contenido real de un catálogo (ej. películas), excluí archivos de prueba, demos técnicos, vlogs genéricos y entradas rotas o sin relación evidente con el criterio. \
Si no se da ningún criterio, aplicá el criterio de legitimidad/calidad que mejor corresponda al tipo de candidatos dado. \
No inventes índices que no estén en la lista de candidatos. Si ningún candidato cumple el criterio, devolvé {\"indices\": []}. \
Nunca sugieras sitios, URLs, ni agregues texto, explicación o markdown fuera del objeto JSON.";

/// Arma el texto de usuario para `curate_by_hint`: criterio (`hint`, si lo
/// hay) + lista numerada de candidatos, mismo formato para cualquier
/// proveedor. Sin `hint`, el prompt de sistema ya sabe aplicar el default
/// de legitimidad/calidad — acá simplemente no se agrega la línea de
/// criterio.
pub(crate) fn build_hint_curation_user_text(candidates: &[String], hint: Option<&str>) -> String {
    let list = candidates
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{i}: {t}"))
        .collect::<Vec<_>>()
        .join("\n");
    match hint {
        Some(h) if !h.trim().is_empty() => format!("Criterio: {h}\n\nCandidatos:\n{list}"),
        _ => format!("Candidatos:\n{list}"),
    }
}

#[derive(Deserialize)]
struct IndexListResponse {
    indices: Vec<usize>,
}

/// Intenta extraer el primer bloque `{...}` de un texto y deserializarlo como
/// `{"indices": [...]}` — mismo patrón defensivo que `extract_structured_query`
/// (algunos proveedores envuelven la respuesta en ```json pese a pedírsela
/// sin adornos). Objeto, no array top-level: ver nota en
/// `CURATE_RESULTS_SYSTEM_PROMPT` sobre el modo JSON forzado de OpenAI.
pub(crate) fn extract_index_list(raw: &str) -> anyhow::Result<Vec<usize>> {
    let start = raw
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("la respuesta del proveedor no contiene un objeto JSON"))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("la respuesta del proveedor no contiene un objeto JSON"))?;
    anyhow::ensure!(end >= start, "delimitadores de JSON inválidos en la respuesta");
    let candidate = &raw[start..=end];
    let parsed: IndexListResponse = serde_json::from_str(candidate)
        .map_err(|e| anyhow::anyhow!("no se pudo parsear la respuesta del proveedor como {{indices: [...]}}: {e}"))?;
    Ok(parsed.indices)
}

/// Intenta extraer el primer bloque `{...}` de un texto (algunos proveedores
/// envuelven el JSON en ```json ... ``` pese a pedírselo sin adornos) y
/// deserializarlo como StructuredQuery.
pub(crate) fn extract_structured_query(raw: &str) -> anyhow::Result<StructuredQuery> {
    let start = raw
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("la respuesta del proveedor no contiene un objeto JSON"))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("la respuesta del proveedor no contiene un objeto JSON"))?;
    anyhow::ensure!(end >= start, "delimitadores de JSON inválidos en la respuesta");
    let candidate = &raw[start..=end];
    serde_json::from_str(candidate)
        .map_err(|e| anyhow::anyhow!("no se pudo parsear la respuesta del proveedor como StructuredQuery: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_structured_query_parses_bare_json() {
        let raw = r#"{"title": "Sintel", "year": 2010, "genre": "animation"}"#;
        let q = extract_structured_query(raw).unwrap();
        assert_eq!(
            q,
            StructuredQuery {
                title: "Sintel".to_string(),
                year: Some(2010),
                genre: Some("animation".to_string()),
            }
        );
    }

    #[test]
    fn extract_structured_query_strips_markdown_fence() {
        let raw = "```json\n{\"title\": \"Sintel\", \"year\": null, \"genre\": null}\n```";
        let q = extract_structured_query(raw).unwrap();
        assert_eq!(q.title, "Sintel");
        assert_eq!(q.year, None);
    }

    #[test]
    fn extract_structured_query_rejects_response_without_json() {
        assert!(extract_structured_query("lo siento, no puedo ayudar con eso").is_err());
    }

    #[test]
    fn extract_index_list_parses_indices_object() {
        assert_eq!(extract_index_list(r#"{"indices": [2, 0, 1]}"#).unwrap(), vec![2, 0, 1]);
    }

    #[test]
    fn extract_index_list_parses_empty_indices() {
        assert_eq!(extract_index_list(r#"{"indices": []}"#).unwrap(), Vec::<usize>::new());
    }

    #[test]
    fn extract_index_list_strips_markdown_fence() {
        let raw = "```json\n{\"indices\": [1, 3]}\n```";
        assert_eq!(extract_index_list(raw).unwrap(), vec![1, 3]);
    }

    #[test]
    fn extract_index_list_rejects_response_without_object() {
        assert!(extract_index_list("no hay ningún objeto acá").is_err());
    }

    #[test]
    fn extract_index_list_rejects_object_without_indices_field() {
        assert!(extract_index_list(r#"{"other": [1, 2]}"#).is_err());
    }
}
