pub mod commands;
pub mod curation;
pub mod gemini;
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

/// Resultado de `curate_by_hint` para UN candidato incluido — a diferencia
/// de `curate_results` (orden relativo dentro de una sola tanda,
/// `Vec<usize>`), acá el `score` es absoluto (0-100) para que sea
/// comparable entre llamadas separadas y así se pueda cachear en SQLite
/// y fusionar con resultados de otras sesiones sin perder coherencia de
/// orden (ver `ai::curation::curate_by_hint` y el caché de curación en el
/// plan). Candidatos no incluidos simplemente no aparecen en el `Vec`.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct ScoredCandidate {
    pub index: usize,
    pub score: i32,
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
    /// `CURATE_BY_HINT_SYSTEM_PROMPT`. A diferencia de `curate_results`,
    /// devuelve un puntaje absoluto por candidato (`ScoredCandidate`), no
    /// solo un orden relativo — necesario para que el caché de curación
    /// (SQLite) pueda comparar resultados de llamadas separadas entre sí.
    async fn curate_by_hint(
        &self,
        candidates: &[String],
        hint: Option<&str>,
    ) -> anyhow::Result<Vec<ScoredCandidate>>;
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
/// nunca en el prompt de sistema. A diferencia de
/// `CURATE_RESULTS_SYSTEM_PROMPT`, pide un puntaje absoluto por candidato
/// (0-100) en vez de solo un orden relativo dentro de la tanda — el caché
/// de curación (SQLite) persiste ese puntaje y lo compara contra el de
/// llamadas separadas (otras sesiones, otros lotes de ítems nuevos) para
/// poder fusionar todo en un único orden coherente sin re-curar lo ya
/// evaluado. Riesgo conocido, no ocultado (Mandato 1): no hay garantía de
/// que un LLM aplique la misma escala interna en evaluaciones separadas
/// — la ancla explícita 0/100 y la instrucción de "absoluto, no relativo
/// a esta tanda" son la mitigación disponible a nivel de prompt, no una
/// garantía de ingeniería.
pub(crate) const CURATE_BY_HINT_SYSTEM_PROMPT: &str = "Sos un filtro de curación para listas de contenido agregadas de internet (canales de TV en vivo o catálogo de video), que mezclan entradas legítimas con basura, duplicados, entradas rotas o de dudosa procedencia. \
Se te da un criterio de curación y una lista numerada de candidatos (nombre de canal o título, con categoría/género entre paréntesis cuando está disponible). \
Devolvé ÚNICAMENTE un objeto JSON con esta forma exacta, sin texto adicional: {\"items\": [{\"index\": number, \"score\": number}, ...]}. \
`index` es el índice (entero, base 0) del candidato sobre la lista dada. `score` es un puntaje entero de 0 a 100 que mide qué tan bien ese candidato cumple el criterio dado: 100 significa que lo cumple perfectamente, 0 que no lo cumple en absoluto. \
El puntaje tiene que ser ABSOLUTO, no relativo a los demás candidatos de esta lista puntual — se va a comparar contra puntajes que vos mismo (u otra instancia tuya) le asignes a otros candidatos en llamadas completamente separadas, posiblemente días después, así que aplicá siempre el mismo criterio de 0 a 100 sin ajustarlo a lo que veas en esta tanda en particular. \
Si el criterio pide verificar legitimidad como radiodifusor público/oficial, juzgalo por el nombre y la categoría — puntuá bajo (cerca de 0) spam, placeholders, duplicados evidentes y canales comerciales/privados sin relación con radiodifusión pública. \
Si el criterio pide contenido real de un catálogo (ej. películas), puntuá bajo archivos de prueba, demos técnicos, vlogs genéricos y entradas rotas o sin relación evidente con el criterio. \
Si no se da ningún criterio, aplicá el criterio de legitimidad/calidad que mejor corresponda al tipo de candidatos dado. \
Solo incluí en `items` los candidatos con score mayor a 0 — omití directamente (no incluyas en la lista) los que no cumplen el criterio en absoluto. No inventes índices que no estén en la lista de candidatos. Si ningún candidato cumple el criterio, devolvé {\"items\": []}. \
Nunca sugieras sitios, URLs, ni agregues texto, explicación o markdown fuera del objeto JSON.";

/// Ejemplo few-shot (par usuario/asistente) enviado junto al system prompt
/// en `curate_by_hint`, mismo par en ambos providers — refuerza en
/// concreto las tres reglas del prompt (excluir irrelevante, excluir
/// duplicado, puntaje absoluto 0-100 sobre lo que queda) en vez de
/// dejarlas solo en prosa. No es una llamada de red extra: viaja en la
/// misma request, como mensajes previos de la conversación.
pub(crate) const CURATE_BY_HINT_EXAMPLE_USER: &str =
    "Criterio: radiodifusores públicos oficiales\n\nCandidatos:\n0: Radio Nacional (Noticias)\n1: MegaVideos XXX (Adultos)\n2: Radio Nacional (Noticias)";
pub(crate) const CURATE_BY_HINT_EXAMPLE_ASSISTANT: &str = "{\"items\": [{\"index\": 0, \"score\": 95}]}";

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

#[derive(Deserialize)]
struct ScoredItemsResponse {
    items: Vec<ScoredCandidate>,
}

/// Análogo a `extract_index_list` pero para el contrato de `curate_by_hint`
/// (`{"items": [{"index", "score"}, ...]}`, puntaje absoluto — ver
/// `CURATE_BY_HINT_SYSTEM_PROMPT`). `extract_index_list` queda intacto y
/// lo sigue usando exclusivamente `curate_results`.
pub(crate) fn extract_scored_list(raw: &str) -> anyhow::Result<Vec<ScoredCandidate>> {
    let start = raw
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("la respuesta del proveedor no contiene un objeto JSON"))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("la respuesta del proveedor no contiene un objeto JSON"))?;
    anyhow::ensure!(end >= start, "delimitadores de JSON inválidos en la respuesta");
    let candidate = &raw[start..=end];
    let parsed: ScoredItemsResponse = serde_json::from_str(candidate)
        .map_err(|e| anyhow::anyhow!("no se pudo parsear la respuesta del proveedor como {{items: [...]}}: {e}"))?;
    Ok(parsed.items)
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

    #[test]
    fn extract_scored_list_parses_items_object() {
        let raw = r#"{"items": [{"index": 2, "score": 90}, {"index": 0, "score": 40}]}"#;
        assert_eq!(
            extract_scored_list(raw).unwrap(),
            vec![
                ScoredCandidate { index: 2, score: 90 },
                ScoredCandidate { index: 0, score: 40 },
            ]
        );
    }

    #[test]
    fn extract_scored_list_parses_empty_items() {
        assert_eq!(extract_scored_list(r#"{"items": []}"#).unwrap(), Vec::new());
    }

    #[test]
    fn extract_scored_list_strips_markdown_fence() {
        let raw = "```json\n{\"items\": [{\"index\": 1, \"score\": 75}]}\n```";
        assert_eq!(extract_scored_list(raw).unwrap(), vec![ScoredCandidate { index: 1, score: 75 }]);
    }

    #[test]
    fn extract_scored_list_rejects_response_without_object() {
        assert!(extract_scored_list("no hay ningún objeto acá").is_err());
    }

    #[test]
    fn extract_scored_list_rejects_object_without_items_field() {
        assert!(extract_scored_list(r#"{"indices": [1, 2]}"#).is_err());
    }
}
