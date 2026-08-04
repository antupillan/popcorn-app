pub mod commands;
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
}

/// Prompt fijo por la app (no editable por el usuario ni por config) —
/// extrae parámetros de búsqueda, nunca sugiere sitios o indexers. Se
/// reutiliza igual sin importar qué proveedor lo ejecute.
pub(crate) const PARSE_QUERY_SYSTEM_PROMPT: &str = "Sos un extractor de parámetros de búsqueda de video (películas, series, documentales). \
A partir del texto en lenguaje natural del usuario, devolvé ÚNICAMENTE un objeto JSON con esta forma exacta, sin texto adicional: \
{\"title\": string, \"year\": number|null, \"genre\": string|null}. \
`title` es el título del contenido tal como lo escribiría el usuario, sin el año ni el género incluidos. \
Nunca incluyas URLs, nombres de sitios web, nombres de indexers, ni sugerencias de dónde buscar — tu única tarea es extraer estos tres campos.";

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
}
