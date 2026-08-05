use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::{
    build_curation_user_text, extract_index_list, extract_structured_query, AiProvider,
    StructuredQuery, CURATE_RESULTS_SYSTEM_PROMPT, PARSE_QUERY_SYSTEM_PROMPT,
};

/// Un solo adaptador para cualquier proveedor que hable el formato de
/// Chat Completions de OpenAI — cubre OpenAI, DeepSeek, Mistral y Ollama
/// local (sin key) porque los cuatro comparten esa forma de API. `base_url`
/// y `model` los define el usuario en su config local, nunca hardcodeados
/// acá (Mandato de Configurabilidad Soberana): distintos proveedores
/// implican distintos endpoints y modelos.
pub struct OpenAiCompatibleProvider {
    base_url: String,
    api_key: Option<String>,
    model: String,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(base_url: String, api_key: Option<String>, model: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}
#[derive(Deserialize)]
struct Choice {
    message: Message,
}
#[derive(Deserialize)]
struct Message {
    content: String,
}

/// Separado de la llamada HTTP para poder probar el parseo del formato real
/// de Chat Completions con una fixture, sin necesitar una API key.
fn parse_response_body(body: &str) -> anyhow::Result<StructuredQuery> {
    let resp: ChatCompletionResponse = serde_json::from_str(body)?;
    let raw = resp
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| anyhow::anyhow!("el proveedor no devolvió ningún choice"))?;
    extract_structured_query(&raw)
}

/// Igual que `parse_response_body` pero extrayendo una lista de índices en
/// vez de un `StructuredQuery` — separado para poder probar el parseo del
/// formato real de Chat Completions con una fixture, sin necesitar key.
fn parse_curation_response_body(body: &str) -> anyhow::Result<Vec<usize>> {
    let resp: ChatCompletionResponse = serde_json::from_str(body)?;
    let raw = resp
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| anyhow::anyhow!("el proveedor no devolvió ningún choice"))?;
    extract_index_list(&raw)
}

#[async_trait]
impl AiProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &'static str {
        "openai_compatible"
    }

    async fn parse_query(&self, text: &str) -> anyhow::Result<StructuredQuery> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": PARSE_QUERY_SYSTEM_PROMPT},
                {"role": "user", "content": text}
            ],
            "response_format": {"type": "json_object"}
        });
        let raw_body = crate::http_retry::send_with_retry(|| {
            let mut req = self.client.post(&url).json(&body);
            if let Some(key) = &self.api_key {
                req = req.bearer_auth(key);
            }
            req
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
        let url = format!("{}/chat/completions", self.base_url);
        let text = build_curation_user_text(query, candidates);
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": CURATE_RESULTS_SYSTEM_PROMPT},
                {"role": "user", "content": text}
            ],
            "response_format": {"type": "json_object"}
        });
        let raw_body = crate::http_retry::send_with_retry(|| {
            let mut req = self.client.post(&url).json(&body);
            if let Some(key) = &self.api_key {
                req = req.bearer_auth(key);
            }
            req
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
    fn parses_real_shaped_chat_completion_response() {
        // Forma real documentada del endpoint /chat/completions, compartida
        // por OpenAI/DeepSeek/Mistral/Ollama.
        let body = r#"{
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "{\"title\": \"Cosmos Laundromat\", \"year\": null, \"genre\": null}"
                },
                "finish_reason": "stop"
            }]
        }"#;
        let q = parse_response_body(body).unwrap();
        assert_eq!(q.title, "Cosmos Laundromat");
        assert_eq!(q.year, None);
    }

    #[test]
    fn errors_when_no_choices() {
        let body = r#"{"choices": []}"#;
        assert!(parse_response_body(body).is_err());
    }

    #[test]
    fn parses_real_shaped_curation_response() {
        let body = r#"{
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "{\"indices\": [2, 0]}"
                },
                "finish_reason": "stop"
            }]
        }"#;
        assert_eq!(parse_curation_response_body(body).unwrap(), vec![2, 0]);
    }

    #[test]
    fn curation_errors_when_no_choices() {
        let body = r#"{"choices": []}"#;
        assert!(parse_curation_response_body(body).is_err());
    }

    #[test]
    fn trims_trailing_slash_from_base_url() {
        let p = OpenAiCompatibleProvider::new(
            "http://localhost:11434/v1/".to_string(),
            None,
            "llama3".to_string(),
        );
        assert_eq!(p.base_url, "http://localhost:11434/v1");
    }

    #[tokio::test]
    #[ignore = "requiere un servidor OpenAI-compatible real (Ollama local, OpenAI, DeepSeek, Mistral) — verificación manual"]
    async fn parse_query_against_real_ollama_local() {
        let base_url = std::env::var("OPENAI_COMPATIBLE_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
        let model = std::env::var("OPENAI_COMPATIBLE_MODEL")
            .expect("seteá OPENAI_COMPATIBLE_MODEL para correr este test manualmente");
        let api_key = std::env::var("OPENAI_COMPATIBLE_API_KEY").ok();
        let provider = OpenAiCompatibleProvider::new(base_url, api_key, model);
        let q = provider
            .parse_query("quiero ver Sintel del 2010, es animación")
            .await
            .unwrap();
        assert!(q.title.to_lowercase().contains("sintel"));
    }
}
