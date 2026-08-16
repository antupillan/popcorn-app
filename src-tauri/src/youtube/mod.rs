mod parse;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::{commands::try_build_active_provider, curation, AiProvider};
use crate::db::Db;
use crate::keychain;

/// Único secreto global de la integración (no hay múltiples "proveedores"
/// como con `AiProvider`, es una sola API key de YouTube Data API v3) —
/// mismo criterio que `SERVICE` en `keychain.rs`: identidad fija de la
/// integración, no config de usuario, por eso es un literal permitido acá
/// bajo el Mandato 5.
const API_KEY_ID: &str = "youtube_api_key";

const YOUTUBE_API_BASE: &str = "https://www.googleapis.com/youtube/v3";

/// Tope de videos por fuente por refresco (4 páginas de 50) — evita
/// paginar canales con miles de uploads en cada carga de la pestaña.
/// Valor de partida razonable, no un límite técnico duro.
const MAX_VIDEOS_PER_SOURCE: usize = 200;
const PAGE_SIZE: &str = "50";

#[derive(Serialize, Clone)]
pub struct YoutubeSource {
    pub id: String,
    pub name: String,
    pub channel_url: String,
    pub channel_id: Option<String>,
    pub category: String,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct YoutubeVideo {
    pub video_id: String,
    pub title: String,
    pub published_at: String,
    pub thumbnail_url: Option<String>,
    pub source_id: String,
    pub category: String,
}

fn valid_category(category: &str) -> bool {
    matches!(category, "cine" | "series" | "anime")
}

fn row_to_source(row: &rusqlite::Row) -> rusqlite::Result<YoutubeSource> {
    Ok(YoutubeSource {
        id: row.get(0)?,
        name: row.get(1)?,
        channel_url: row.get(2)?,
        channel_id: row.get(3)?,
        category: row.get(4)?,
        enabled: row.get::<_, i64>(5)? != 0,
    })
}

#[tauri::command]
pub async fn list_youtube_sources(db: State<'_, Db>) -> Result<Vec<YoutubeSource>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, channel_url, channel_id, category, enabled \
             FROM youtube_sources ORDER BY created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_source).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

async fn resolve_channel(
    client: &reqwest::Client,
    api_key: &str,
    channel_url: &str,
) -> anyhow::Result<parse::ResolvedChannel> {
    let reference = parse::extract_channel_reference(channel_url)?;
    let (param_name, param_value) = match &reference {
        parse::ChannelReference::Handle(h) => ("forHandle", h.as_str()),
        parse::ChannelReference::Id(id) => ("id", id.as_str()),
    };
    let url = format!("{YOUTUBE_API_BASE}/channels");
    let resp = crate::http_retry::send_with_retry(|| {
        client
            .get(&url)
            .query(&[("part", "contentDetails"), (param_name, param_value), ("key", api_key)])
    })
    .await
    .context("no se pudo contactar la YouTube Data API (channels.list)")?;
    if !resp.status().is_success() {
        anyhow::bail!("YouTube Data API devolvió {} al resolver el canal", resp.status());
    }
    let body = resp.text().await.context("no se pudo leer la respuesta de channels.list")?;
    parse::parse_channels_response(&body)
}

#[tauri::command]
pub async fn add_youtube_source(
    http: State<'_, crate::commands::HttpClient>,
    db: State<'_, Db>,
    name: String,
    channel_url: String,
    category: String,
) -> Result<YoutubeSource, String> {
    if !valid_category(&category) {
        return Err(format!("categoría inválida: {category} (debe ser cine, series o anime)"));
    }
    let api_key = keychain::get_secret(API_KEY_ID)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "configura tu YouTube Data API key en Ajustes antes de agregar un canal".to_string())?;
    let resolved = resolve_channel(&http.0, &api_key, &channel_url)
        .await
        .map_err(|e| e.to_string())?;

    let id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO youtube_sources (id, name, channel_url, channel_id, uploads_playlist_id, category) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (&id, &name, &channel_url, &resolved.channel_id, &resolved.uploads_playlist_id, &category),
        )
        .map_err(|e| e.to_string())?;
        // Toda fuente necesita su fila de curación, mismo contrato que
        // add_indexer/add_iptv_source_url (ver source_settings).
        conn.execute("INSERT INTO source_settings (id) VALUES (?1)", [&id])
            .map_err(|e| e.to_string())?;
    }
    Ok(YoutubeSource {
        id,
        name,
        channel_url,
        channel_id: Some(resolved.channel_id),
        category,
        enabled: true,
    })
}

#[tauri::command]
pub async fn remove_youtube_source(db: State<'_, Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM youtube_sources WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    // source_settings no tiene FK real a youtube_sources (mismo patrón que
    // indexers/iptv_sources) — el borrado en cascada se hace a mano acá.
    conn.execute("DELETE FROM source_settings WHERE id = ?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn toggle_youtube_source(db: State<'_, Db>, id: String, enabled: bool) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute("UPDATE youtube_sources SET enabled = ?1 WHERE id = ?2", (enabled as i64, &id))
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn set_youtube_api_key(key: String) -> Result<(), String> {
    keychain::set_secret(API_KEY_ID, &key).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_youtube_api_key_status() -> Result<bool, String> {
    Ok(keychain::get_secret(API_KEY_ID).map_err(|e| e.to_string())?.is_some())
}

#[tauri::command]
pub async fn remove_youtube_api_key() -> Result<(), String> {
    keychain::delete_secret(API_KEY_ID).map_err(|e| e.to_string())
}

async fn fetch_videos_page(
    client: &reqwest::Client,
    api_key: &str,
    uploads_playlist_id: &str,
    page_token: Option<&str>,
) -> anyhow::Result<(Vec<parse::RawVideo>, Option<String>)> {
    let url = format!("{YOUTUBE_API_BASE}/playlistItems");
    let mut query = vec![
        ("part", "snippet"),
        ("playlistId", uploads_playlist_id),
        ("maxResults", PAGE_SIZE),
        ("key", api_key),
    ];
    if let Some(token) = page_token {
        query.push(("pageToken", token));
    }
    let resp = crate::http_retry::send_with_retry(|| client.get(&url).query(&query))
        .await
        .context("no se pudo contactar la YouTube Data API (playlistItems.list)")?;
    if !resp.status().is_success() {
        anyhow::bail!("YouTube Data API devolvió {} al listar videos", resp.status());
    }
    let body = resp.text().await.context("no se pudo leer la respuesta de playlistItems.list")?;
    parse::parse_playlist_items_response(&body)
}

async fn list_videos_for_source(
    client: &reqwest::Client,
    api_key: &str,
    source: &YoutubeSource,
    uploads_playlist_id: &str,
) -> anyhow::Result<Vec<YoutubeVideo>> {
    let mut videos = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let (raw, next) = fetch_videos_page(client, api_key, uploads_playlist_id, page_token.as_deref()).await?;
        for v in raw {
            videos.push(YoutubeVideo {
                video_id: v.video_id,
                title: v.title,
                published_at: v.published_at,
                thumbnail_url: v.thumbnail_url,
                source_id: source.id.clone(),
                category: source.category.clone(),
            });
        }
        if videos.len() >= MAX_VIDEOS_PER_SOURCE {
            videos.truncate(MAX_VIDEOS_PER_SOURCE);
            break;
        }
        page_token = next;
        if page_token.is_none() {
            break;
        }
    }
    Ok(videos)
}

/// Tope de páginas para la búsqueda profunda (ver
/// `search_videos_for_source_deep`) — 10 páginas de 50 = hasta 500 videos
/// escaneados por canal. No persigue exactamente "lo que queda después
/// de los primeros `MAX_VIDEOS_PER_SOURCE`" (reanudar desde ahí exigiría
/// guardar el `pageToken` entre llamadas) — vuelve a paginar desde el
/// principio del uploads playlist, filtrando por título a medida que
/// pagina. No exhaustivo, valor de partida razonable, no un límite
/// técnico duro — se dice explícito en la UI, nunca se promete "busca en
/// todo el canal".
const DEEP_SEARCH_MAX_PAGES: usize = 10;

/// Búsqueda de red real dentro de una fuente ya agregada, más allá de lo
/// que `list_videos_for_source` ya cachea — usada por la lupa global
/// cuando el filtro de texto/IA sobre lo ya cargado no encuentra nada.
/// Filtra por coincidencia de substring en el título (case-insensitive)
/// a medida que pagina, para no devolver videos de más al frontend.
/// Extraído aparte para poder testear el criterio de match sin red
/// (Mandato 12) — el bucle de paginación en sí reusa `fetch_videos_page`,
/// ya cubierto indirectamente por los fixtures de
/// `parse::parse_playlist_items_response` y por `list_videos_for_source`
/// en uso real; no hay mock de red multi-página en este archivo (mismo
/// criterio que ya rige `list_videos_for_source`, sin test dedicado de su
/// propio bucle) — verificado en cambio con `tauri dev` real.
fn title_matches(title: &str, query_lower: &str) -> bool {
    title.to_lowercase().contains(query_lower)
}

async fn search_videos_for_source_deep(
    client: &reqwest::Client,
    api_key: &str,
    source: &YoutubeSource,
    uploads_playlist_id: &str,
    query_lower: &str,
) -> anyhow::Result<Vec<YoutubeVideo>> {
    let mut matches = Vec::new();
    let mut page_token: Option<String> = None;
    for _ in 0..DEEP_SEARCH_MAX_PAGES {
        let (raw, next) = fetch_videos_page(client, api_key, uploads_playlist_id, page_token.as_deref()).await?;
        for v in raw {
            if title_matches(&v.title, query_lower) {
                matches.push(YoutubeVideo {
                    video_id: v.video_id,
                    title: v.title,
                    published_at: v.published_at,
                    thumbnail_url: v.thumbnail_url,
                    source_id: source.id.clone(),
                    category: source.category.clone(),
                });
            }
        }
        page_token = next;
        if page_token.is_none() {
            break;
        }
    }
    Ok(matches)
}

/// Fetch rápido sin curar de todas las fuentes habilitadas — mismo
/// criterio anti-bloqueo que `iptv::list_channels`/`browse_online_library_
/// fast_inner`: la pestaña debe cargar igual de rápido con o sin IA
/// configurada, la curación es un enriquecimiento aparte
/// (`curate_youtube_videos`). Sin fuentes habilitadas, devuelve vacío sin
/// pedir la API key — pedirla solo cuando hace falta de verdad evita un
/// error espurio en una instalación nueva sin canales agregados todavía.
fn enabled_sources_with_playlist(db: &Db) -> Result<Vec<(YoutubeSource, Option<String>)>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, channel_url, channel_id, category, enabled, uploads_playlist_id \
             FROM youtube_sources WHERE enabled = 1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((row_to_source(r)?, r.get::<_, Option<String>>(6)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

#[tauri::command]
pub async fn list_youtube_videos(
    http: State<'_, crate::commands::HttpClient>,
    db: State<'_, Db>,
) -> Result<Vec<YoutubeVideo>, String> {
    let sources_with_playlist = enabled_sources_with_playlist(&db)?;

    if sources_with_playlist.is_empty() {
        return Ok(Vec::new());
    }

    let api_key = keychain::get_secret(API_KEY_ID)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "configura tu YouTube Data API key en Ajustes para ver videos".to_string())?;

    let mut all = Vec::new();
    for (source, uploads_playlist_id) in &sources_with_playlist {
        let Some(uploads_playlist_id) = uploads_playlist_id else {
            eprintln!("[popcorn] fuente YouTube '{}' sin uploads_playlist_id resuelto, saltada", source.name);
            continue;
        };
        match list_videos_for_source(&http.0, &api_key, source, uploads_playlist_id).await {
            Ok(mut videos) => all.append(&mut videos),
            Err(e) => eprintln!("[popcorn] fuente YouTube '{}' falló: {e}", source.name),
        }
    }
    Ok(all)
}

/// Búsqueda global (lupa, ver `SearchModal.tsx`): cuando el filtro de
/// texto/IA sobre lo ya cargado no encuentra nada, se llama a esto para
/// buscar más profundo dentro de canales YA agregados (nunca contra
/// canales nuevos vía `search.list` — decisión explícita, ver
/// `Planes_mejora_popcorn/busqueda_global.txt`).
#[tauri::command]
pub async fn search_youtube_videos_in_added_channels(
    http: State<'_, crate::commands::HttpClient>,
    db: State<'_, Db>,
    query: String,
) -> Result<Vec<YoutubeVideo>, String> {
    let sources_with_playlist = enabled_sources_with_playlist(&db)?;
    if sources_with_playlist.is_empty() || query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let api_key = keychain::get_secret(API_KEY_ID)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "configura tu YouTube Data API key en Ajustes para buscar".to_string())?;
    let query_lower = query.trim().to_lowercase();

    let mut all = Vec::new();
    for (source, uploads_playlist_id) in &sources_with_playlist {
        let Some(uploads_playlist_id) = uploads_playlist_id else {
            continue;
        };
        match search_videos_for_source_deep(&http.0, &api_key, source, uploads_playlist_id, &query_lower).await {
            Ok(mut videos) => all.append(&mut videos),
            Err(e) => eprintln!("[popcorn] búsqueda profunda en fuente YouTube '{}' falló: {e}", source.name),
        }
    }
    Ok(all)
}

fn curation_settings_for(db: &Db, source_id: &str) -> Result<(bool, Option<String>), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT curation_enabled, curation_hint FROM source_settings WHERE id = ?1",
        [source_id],
        |r| Ok((r.get::<_, i64>(0)? != 0, r.get(1)?)),
    )
    .map_err(|e| e.to_string())
}

fn youtube_video_key(v: &YoutubeVideo) -> String {
    format!("{}:{}", v.source_id, v.video_id)
}

/// Re-cura una lista de `YoutubeVideo` ya obtenida (vía `list_youtube_
/// videos`) — se llama después de renderizar el resultado rápido sin
/// curar, mismo criterio que `curate_online_items_inner`/`curate_channels_
/// inner`. Agrupa por `source_id` para aplicar el `curation_hint` correcto
/// de cada canal. Sin ping de disponibilidad (a diferencia de Online/IPTV):
/// un video borrado/privado falla del lado del cliente al cargar el
/// IFrame, no justifica gastar cuota de la Data API en chequeos activos
/// (decisión de diseño, ver plan).
async fn curate_youtube_videos_inner(
    db: &Db,
    provider: Option<&dyn AiProvider>,
    videos: Vec<YoutubeVideo>,
) -> Result<Vec<YoutubeVideo>, String> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<YoutubeVideo>> = std::collections::HashMap::new();
    for v in videos {
        groups.entry(v.source_id.clone()).or_insert_with(|| {
            order.push(v.source_id.clone());
            Vec::new()
        }).push(v);
    }

    let mut result = Vec::new();
    for source_id in order {
        let group = groups.remove(&source_id).unwrap();
        match provider {
            Some(provider) => {
                let (enabled, hint) = curation_settings_for(db, &source_id)?;
                if enabled {
                    let curated = curation::curate_by_hint(
                        db,
                        &source_id,
                        provider,
                        group,
                        |v: &YoutubeVideo| v.title.as_str(),
                        youtube_video_key,
                        hint.as_deref(),
                    )
                    .await?;
                    result.extend(curated);
                } else {
                    result.extend(group);
                }
            }
            None => result.extend(group),
        }
    }
    Ok(result)
}

#[tauri::command]
pub async fn curate_youtube_videos(db: State<'_, Db>, videos: Vec<YoutubeVideo>) -> Result<Vec<YoutubeVideo>, String> {
    let provider = try_build_active_provider(&db)?;
    curate_youtube_videos_inner(&db, provider.as_deref(), videos).await
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

    #[test]
    fn title_matches_is_case_insensitive_substring() {
        // El query ya llega en minúsculas (lo lowercasea el caller una sola
        // vez fuera del loop de paginación, ver search_videos_for_source_deep)
        // — acá se testea que el título con mayúsculas igual matchea.
        assert!(title_matches("Episodio 42: El Regreso", "regreso"));
        assert!(title_matches("Episodio 42: El Regreso", "episodio"));
        assert!(!title_matches("Episodio 42: El Regreso", "temporada 3"));
    }

    #[test]
    fn title_matches_empty_query_matches_everything() {
        assert!(title_matches("cualquier título", ""));
    }

    fn seed_source_settings(db: &Db, id: &str, curation_enabled: bool, hint: Option<&str>) {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO source_settings (id, curation_enabled, curation_hint) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, curation_enabled as i64, hint],
        )
        .unwrap();
    }

    fn video(source_id: &str, video_id: &str) -> YoutubeVideo {
        YoutubeVideo {
            video_id: video_id.to_string(),
            title: format!("título {video_id}"),
            published_at: "2024-01-01T00:00:00Z".to_string(),
            thumbnail_url: None,
            source_id: source_id.to_string(),
            category: "anime".to_string(),
        }
    }

    struct HintRecordingProvider(std::sync::Mutex<Vec<Option<String>>>);
    #[async_trait]
    impl AiProvider for HintRecordingProvider {
        fn name(&self) -> &'static str {
            "fake-hint-recording"
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
            candidates: &[String],
            hint: Option<&str>,
        ) -> anyhow::Result<Vec<crate::ai::ScoredCandidate>> {
            self.0.lock().unwrap().push(hint.map(|h| h.to_string()));
            Ok((0..candidates.len()).map(|i| crate::ai::ScoredCandidate { index: i, score: 50 }).collect())
        }
        async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
        async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
    }

    #[tokio::test]
    async fn curate_applies_the_right_hint_per_source_group() {
        let db = migrated_db();
        seed_source_settings(&db, "src_a", true, Some("hint A"));
        seed_source_settings(&db, "src_b", true, Some("hint B"));
        let videos = vec![video("src_a", "v1"), video("src_b", "v2"), video("src_a", "v3")];
        let provider = HintRecordingProvider(std::sync::Mutex::new(Vec::new()));

        let result = curate_youtube_videos_inner(&db, Some(&provider), videos).await.unwrap();

        assert_eq!(result.len(), 3, "no debe perder videos al agrupar/desagrupar por fuente");
        let mut hints_seen = provider.0.into_inner().unwrap();
        hints_seen.sort();
        assert_eq!(
            hints_seen,
            vec![Some("hint A".to_string()), Some("hint B".to_string())],
            "cada grupo de source_id debe curarse con su propio hint"
        );
    }

    #[tokio::test]
    async fn curate_skips_disabled_sources_without_calling_provider() {
        let db = migrated_db();
        seed_source_settings(&db, "src_off", false, Some("no debería usarse"));
        let videos = vec![video("src_off", "v1")];
        let provider = HintRecordingProvider(std::sync::Mutex::new(Vec::new()));

        let result = curate_youtube_videos_inner(&db, Some(&provider), videos).await.unwrap();

        assert_eq!(result.len(), 1);
        assert!(provider.0.into_inner().unwrap().is_empty(), "curation_enabled=0 no debe llamar al proveedor");
    }

    #[tokio::test]
    async fn curate_returns_videos_unchanged_without_a_provider() {
        let db = migrated_db();
        let videos = vec![video("src_a", "v1")];

        let result = curate_youtube_videos_inner(&db, None, videos.clone()).await.unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].video_id, "v1");
    }

    #[tokio::test]
    #[ignore = "red real, requiere youtube_api_key en el keychain — no disponible en este entorno de desarrollo"]
    async fn resolve_channel_against_real_data_api() {
        let api_key = keychain::get_secret(API_KEY_ID).unwrap().expect("configurar youtube_api_key antes de correr este test");
        let client = reqwest::Client::new();
        let resolved = resolve_channel(&client, &api_key, "@MuseAsia").await.unwrap();
        assert!(!resolved.channel_id.is_empty());
        assert!(!resolved.uploads_playlist_id.is_empty());
    }
}
