use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{
    build_curation_user_text, extract_index_list, extract_structured_query, AiProvider,
    StructuredQuery, CURATE_RESULTS_SYSTEM_PROMPT, PARSE_QUERY_SYSTEM_PROMPT,
};

const API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GeminiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct GenerateContentResponse {
    candidates: Vec<Candidate>,
}
#[derive(Deserialize)]
struct Candidate {
    content: Content,
}
#[derive(Deserialize)]
struct Content {
    parts: Vec<Part>,
}
#[derive(Deserialize)]
struct Part {
    text: String,
}

/// Separado de la llamada HTTP para poder probar el parseo del formato real
/// de respuesta de Gemini con una fixture, sin necesitar una API key.
fn parse_response_body(body: &str) -> anyhow::Result<StructuredQuery> {
    let resp: GenerateContentResponse = serde_json::from_str(body)?;
    let raw = resp
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| p.text)
        .ok_or_else(|| anyhow::anyhow!("Gemini no devolvió contenido en la respuesta"))?;
    extract_structured_query(&raw)
}

/// Igual que `parse_response_body` pero extrayendo una lista de índices en
/// vez de un `StructuredQuery` — separado para poder probar el parseo del
/// formato real de Gemini con una fixture, sin necesitar una API key.
fn parse_curation_response_body(body: &str) -> anyhow::Result<Vec<usize>> {
    let resp: GenerateContentResponse = serde_json::from_str(body)?;
    let raw = resp
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| p.text)
        .ok_or_else(|| anyhow::anyhow!("Gemini no devolvió contenido en la respuesta"))?;
    extract_index_list(&raw)
}

#[async_trait]
impl AiProvider for GeminiProvider {
    fn name(&self) -> &'static str {
        "gemini"
    }

    async fn parse_query(&self, text: &str) -> anyhow::Result<StructuredQuery> {
        let url = format!("{API_BASE}/models/{}:generateContent", self.model);
        let body = json!({
            "contents": [{"parts": [{"text": text}]}],
            "systemInstruction": {"parts": [{"text": PARSE_QUERY_SYSTEM_PROMPT}]},
            "generationConfig": {"responseMimeType": "application/json"}
        });
        let raw_body = crate::http_retry::send_with_retry(|| {
            self.client
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .json(&body)
        })
        .await?
        .error_for_status()?
        .text()
        .await?;
        parse_response_body(&raw_body)
    }

    async fn curate_results(
        &self,
        query: &StructuredQuery,
        candidates: &[String],
    ) -> anyhow::Result<Vec<usize>> {
        let url = format!("{API_BASE}/models/{}:generateContent", self.model);
        let text = build_curation_user_text(query, candidates);
        let body = json!({
            "contents": [{"parts": [{"text": text}]}],
            "systemInstruction": {"parts": [{"text": CURATE_RESULTS_SYSTEM_PROMPT}]},
            "generationConfig": {"responseMimeType": "application/json"}
        });
        let raw_body = crate::http_retry::send_with_retry(|| {
            self.client
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .json(&body)
        })
        .await?
        .error_for_status()?
        .text()
        .await?;
        parse_curation_response_body(&raw_body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_shaped_gemini_response() {
        // Forma real documentada de la API generateContent de Gemini.
        let body = r#"{
            "candidates": [{
                "content": {
                    "parts": [{"text": "{\"title\": \"Sintel\", \"year\": 2010, \"genre\": \"animation\"}"}],
                    "role": "model"
                },
                "finishReason": "STOP"
            }]
        }"#;
        let q = parse_response_body(body).unwrap();
        assert_eq!(q.title, "Sintel");
        assert_eq!(q.year, Some(2010));
    }

    #[test]
    fn errors_when_no_candidates() {
        let body = r#"{"candidates": []}"#;
        assert!(parse_response_body(body).is_err());
    }

    #[test]
    fn parses_real_shaped_curation_response() {
        let body = r#"{
            "candidates": [{
                "content": {
                    "parts": [{"text": "{\"indices\": [2, 0]}"}],
                    "role": "model"
                },
                "finishReason": "STOP"
            }]
        }"#;
        assert_eq!(parse_curation_response_body(body).unwrap(), vec![2, 0]);
    }

    #[test]
    fn curation_errors_when_no_candidates() {
        let body = r#"{"candidates": []}"#;
        assert!(parse_curation_response_body(body).is_err());
    }

    #[tokio::test]
    #[ignore = "requiere GEMINI_API_KEY real — verificación manual contra la API en vivo"]
    async fn parse_query_against_real_gemini_api() {
        let api_key = std::env::var("GEMINI_API_KEY")
            .expect("seteá GEMINI_API_KEY para correr este test manualmente");
        let provider = GeminiProvider::new(api_key, "gemini-2.5-flash".to_string());
        let q = provider
            .parse_query("quiero ver Sintel del 2010, es animación")
            .await
            .unwrap();
        assert!(q.title.to_lowercase().contains("sintel"));
    }
}
