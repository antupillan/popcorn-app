// Subtítulos — Parte A del plan (traducir + editar localmente), ver
// Planes_mejora_popcorn/subtitulos_ia.txt. Solo media_items (Mi Colección)
// y Biblioteca Local: ambos con duración fija, reproducidos por el mismo
// <video> propio de VideoPlayer.tsx — YouTube (iframe cross-origin) e IPTV
// en vivo (sin timeline fijo) quedan fuera por límite técnico real, no por
// elección (ver plan). Parseo/reserializado de SRT vive enteramente en el
// frontend (src/lib/srt.ts) — acá solo se persiste el blob de texto.

use serde::Serialize;
use tauri::State;

use crate::ai::commands::try_build_active_provider;
use crate::ai::AiProvider;
use crate::db::Db;

#[derive(Serialize, Clone)]
pub struct Subtitle {
    pub id: String,
    pub media_item_id: String,
    pub language: String,
    pub origin: String,
    pub content: String,
    pub created_at: String,
}

fn valid_origin(origin: &str) -> bool {
    matches!(origin, "original" | "ai_translated" | "human_edited")
}

fn row_to_subtitle(row: &rusqlite::Row) -> rusqlite::Result<Subtitle> {
    Ok(Subtitle {
        id: row.get(0)?,
        media_item_id: row.get(1)?,
        language: row.get(2)?,
        origin: row.get(3)?,
        content: row.get(4)?,
        created_at: row.get(5)?,
    })
}

const SELECT_COLUMNS: &str = "id, media_item_id, language, origin, content, created_at";

#[tauri::command]
pub async fn list_subtitles(db: State<'_, Db>, media_item_id: String) -> Result<Vec<Subtitle>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLUMNS} FROM subtitles WHERE media_item_id = ?1 ORDER BY created_at ASC"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([&media_item_id], row_to_subtitle).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Núcleo testable de `add_subtitle_text` — recibe la conexión ya
/// bloqueada para no depender de `State`/`Db` en tests (mismo criterio de
/// separación que `translate_subtitle_texts_inner`).
fn add_subtitle_text_core(
    conn: &rusqlite::Connection,
    media_item_id: &str,
    language: &str,
    origin: &str,
    content: &str,
) -> Result<Subtitle, String> {
    if !valid_origin(origin) {
        return Err(format!("origin inválido: {origin} (debe ser original, ai_translated o human_edited)"));
    }
    if content.trim().is_empty() {
        return Err("el subtítulo no puede estar vacío".to_string());
    }
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO subtitles (id, media_item_id, language, origin, content) VALUES (?1, ?2, ?3, ?4, ?5)",
        (&id, media_item_id, language, origin, content),
    )
    .map_err(|e| e.to_string())?;
    conn.query_row(&format!("SELECT {SELECT_COLUMNS} FROM subtitles WHERE id = ?1"), [&id], row_to_subtitle)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_subtitle_text(
    db: State<'_, Db>,
    media_item_id: String,
    language: String,
    origin: String,
    content: String,
) -> Result<Subtitle, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    add_subtitle_text_core(&conn, &media_item_id, &language, &origin, &content)
}

#[tauri::command]
pub async fn remove_subtitle(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM subtitles WHERE id = ?1", [&id]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Núcleo testable de `translate_subtitle_texts` — separado del comando
/// Tauri para no depender de `State` en tests, mismo patrón que
/// `search_added_content_with_ai_inner` (`ai/commands.rs`). Passthrough
/// delgado a `AiProvider::translate`: la validación de cantidad/orden ya
/// vive ahí (cada implementación de proveedor la hace contra su propia
/// respuesta), acá no se duplica.
async fn translate_subtitle_texts_inner(
    provider: Option<&dyn AiProvider>,
    texts: &[String],
    target_lang: &str,
) -> Result<Vec<String>, String> {
    let provider = provider
        .ok_or_else(|| "no hay ningún proveedor de IA activo — configura uno en Ajustes".to_string())?;
    provider
        .translate(texts, target_lang)
        .await
        .map_err(|e| format!("[{}] {e}", provider.name()))
}

#[tauri::command]
pub async fn translate_subtitle_texts(
    db: State<'_, Db>,
    texts: Vec<String>,
    target_lang: String,
) -> Result<Vec<String>, String> {
    let provider = try_build_active_provider(&db)?;
    translate_subtitle_texts_inner(provider.as_deref(), &texts, &target_lang).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rusqlite::Connection;

    fn migrated_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(std::sync::Mutex::new(conn))
    }

    struct FakeProvider;
    #[async_trait]
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
        async fn translate(&self, texts: &[String], target_lang: &str) -> anyhow::Result<Vec<String>> {
            assert_eq!(target_lang, "en");
            Ok(texts.iter().map(|t| format!("[{target_lang}] {t}")).collect())
        }
        async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
    }

    #[test]
    fn valid_origin_accepts_the_three_known_values_and_rejects_others() {
        assert!(valid_origin("original"));
        assert!(valid_origin("ai_translated"));
        assert!(valid_origin("human_edited"));
        assert!(!valid_origin("community_rated"));
    }

    #[test]
    fn add_subtitle_text_core_rejects_invalid_origin() {
        let db = migrated_db();
        let conn = db.0.lock().unwrap();
        let result = add_subtitle_text_core(&conn, "m1", "es", "community_rated", "contenido real");
        assert!(result.is_err());
    }

    #[test]
    fn add_subtitle_text_core_rejects_empty_content() {
        let db = migrated_db();
        let conn = db.0.lock().unwrap();
        let result = add_subtitle_text_core(&conn, "m1", "es", "original", "   ");
        assert!(result.is_err());
    }

    #[test]
    fn add_subtitle_text_core_persists_and_returns_the_real_row() {
        let db = migrated_db();
        let conn = db.0.lock().unwrap();
        let sub = add_subtitle_text_core(&conn, "m1", "es", "original", "1\n00:00:00,000 --> 00:00:01,000\nHola").unwrap();
        assert_eq!(sub.media_item_id, "m1");
        assert_eq!(sub.origin, "original");
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM subtitles WHERE id = ?1", [&sub.id], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn translate_subtitle_texts_fails_explicitly_without_active_provider() {
        let result = translate_subtitle_texts_inner(None, &["Hola".to_string()], "en").await;
        assert!(result.is_err(), "sin proveedor activo debe fallar explícito, no devolver vacío en silencio");
    }

    #[tokio::test]
    async fn translate_subtitle_texts_delegates_to_the_active_provider() {
        let result = translate_subtitle_texts_inner(Some(&FakeProvider), &["Hola".to_string()], "en")
            .await
            .unwrap();
        assert_eq!(result, vec!["[en] Hola".to_string()]);
    }
}
