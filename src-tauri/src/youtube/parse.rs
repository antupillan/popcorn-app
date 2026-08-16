use anyhow::Context;
use serde::Deserialize;

/// Referencia a un canal extraída de lo que el usuario pegó al agregar la
/// fuente. Solo se soportan las dos formas que la Data API resuelve de
/// forma directa y documentada (`forHandle`/`id`) — a diferencia de
/// `/c/nombre` o `/user/nombre` (URLs legacy sin endpoint de resolución
/// directa confiable), donde inventar una heurística de scraping sería
/// exactamente el tipo de suposición no verificable que el Mandato 4
/// prohíbe. Si el usuario pega una de esas, se lo dice, no se adivina.
#[derive(Debug, PartialEq, Eq)]
pub enum ChannelReference {
    Handle(String),
    Id(String),
}

/// Decodifica porcentaje-encoding si lo hay (una URL de YouTube copiada del
/// navegador trae handles con tildes/ñ como `%C3%B3` etc.) — sin esto, el
/// fragmento crudo viaja como valor de `forHandle` y `reqwest::query()` lo
/// vuelve a codificar encima (el `%` mismo pasa a `%25`), doble-codificando
/// el handle real y haciendo que `channels.list` nunca lo encuentre (bug
/// real, confirmado en vivo con "Radio Emancipación Chile"). No-op si el
/// fragmento no tenía nada codificado.
fn decode_url_fragment(s: &str) -> String {
    urlencoding::decode(s).map(|c| c.into_owned()).unwrap_or_else(|_| s.to_string())
}

pub fn extract_channel_reference(input: &str) -> anyhow::Result<ChannelReference> {
    let s = input.trim();

    if let Some(rest) = s.strip_prefix('@') {
        if !rest.is_empty() {
            return Ok(ChannelReference::Handle(decode_url_fragment(rest)));
        }
    }
    if let Some(idx) = s.find("/@") {
        let rest = &s[idx + 2..];
        let handle = rest.split(['/', '?']).next().unwrap_or(rest);
        if !handle.is_empty() {
            return Ok(ChannelReference::Handle(decode_url_fragment(handle)));
        }
    }
    if let Some(idx) = s.find("/channel/") {
        let rest = &s[idx + "/channel/".len()..];
        let id = rest.split(['/', '?']).next().unwrap_or(rest);
        if !id.is_empty() {
            return Ok(ChannelReference::Id(decode_url_fragment(id)));
        }
    }
    if s.starts_with("UC") && s.len() == 24 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Ok(ChannelReference::Id(s.to_string()));
    }

    anyhow::bail!(
        "no se pudo interpretar '{s}' como canal de YouTube — usa un handle (@nombre o \
         youtube.com/@nombre) o una URL youtube.com/channel/UC... (las URLs /c/ o /user/ \
         legacy no se resuelven de forma confiable, busca el link con /channel/ en su lugar)"
    )
}

#[derive(Deserialize)]
struct ChannelsListResponse {
    items: Vec<ChannelItem>,
}

#[derive(Deserialize)]
struct ChannelItem {
    id: String,
    #[serde(rename = "contentDetails")]
    content_details: ChannelContentDetails,
}

#[derive(Deserialize)]
struct ChannelContentDetails {
    #[serde(rename = "relatedPlaylists")]
    related_playlists: RelatedPlaylists,
}

#[derive(Deserialize)]
struct RelatedPlaylists {
    uploads: String,
}

pub struct ResolvedChannel {
    pub channel_id: String,
    pub uploads_playlist_id: String,
}

/// Parsea la respuesta real de `channels.list?part=contentDetails` (forma
/// documentada en la Data API v3, no inferida) — separado del fetch en sí
/// para poder testear contra fixtures sin red real.
pub fn parse_channels_response(body: &str) -> anyhow::Result<ResolvedChannel> {
    let parsed: ChannelsListResponse =
        serde_json::from_str(body).context("respuesta de channels.list no es el JSON esperado")?;
    let item = parsed
        .items
        .into_iter()
        .next()
        .context("channels.list no devolvió ningún canal — ¿el handle/id existe?")?;
    Ok(ResolvedChannel {
        channel_id: item.id,
        uploads_playlist_id: item.content_details.related_playlists.uploads,
    })
}

#[derive(Deserialize)]
struct PlaylistItemsResponse {
    items: Vec<PlaylistItem>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct PlaylistItem {
    snippet: PlaylistItemSnippet,
}

#[derive(Deserialize)]
struct PlaylistItemSnippet {
    title: String,
    #[serde(rename = "publishedAt")]
    published_at: String,
    #[serde(rename = "resourceId")]
    resource_id: ResourceId,
    thumbnails: Thumbnails,
}

#[derive(Deserialize)]
struct ResourceId {
    #[serde(rename = "videoId")]
    video_id: String,
}

#[derive(Deserialize)]
struct Thumbnails {
    high: Option<Thumbnail>,
    default: Option<Thumbnail>,
}

#[derive(Deserialize)]
struct Thumbnail {
    url: String,
}

pub struct RawVideo {
    pub video_id: String,
    pub title: String,
    pub published_at: String,
    pub thumbnail_url: Option<String>,
}

/// Parsea la respuesta real de `playlistItems.list?part=snippet` — mismo
/// criterio de separación fetch/parse que `parse_channels_response`.
/// `thumbnails.high` no siempre está presente (canales viejos/videos sin
/// reprocesar); cae a `default` antes que a `None`.
pub fn parse_playlist_items_response(body: &str) -> anyhow::Result<(Vec<RawVideo>, Option<String>)> {
    let parsed: PlaylistItemsResponse =
        serde_json::from_str(body).context("respuesta de playlistItems.list no es el JSON esperado")?;
    let videos = parsed
        .items
        .into_iter()
        .map(|item| RawVideo {
            video_id: item.snippet.resource_id.video_id,
            title: item.snippet.title,
            published_at: item.snippet.published_at,
            thumbnail_url: item
                .snippet
                .thumbnails
                .high
                .or(item.snippet.thumbnails.default)
                .map(|t| t.url),
        })
        .collect();
    Ok((videos, parsed.next_page_token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_handle_from_bare_at_prefix() {
        assert_eq!(
            extract_channel_reference("@MuseAsia").unwrap(),
            ChannelReference::Handle("MuseAsia".to_string())
        );
    }

    #[test]
    fn extracts_handle_from_full_url() {
        assert_eq!(
            extract_channel_reference("https://www.youtube.com/@MuseAsia").unwrap(),
            ChannelReference::Handle("MuseAsia".to_string())
        );
    }

    #[test]
    fn extracts_handle_from_url_with_trailing_path() {
        assert_eq!(
            extract_channel_reference("https://www.youtube.com/@MuseAsia/videos").unwrap(),
            ChannelReference::Handle("MuseAsia".to_string())
        );
    }

    #[test]
    fn decodes_percent_encoded_accented_handle_bug_reported_live() {
        // Caso real reportado por el usuario: "Radio Emancipación Chile",
        // el navegador da el handle con la ó codificada. Sin decodificar
        // acá, el fragmento crudo (con "%") viaja a reqwest::query(), que
        // lo codifica una segunda vez y channels.list nunca encuentra el
        // canal real.
        assert_eq!(
            extract_channel_reference("https://www.youtube.com/@RadioEmancipaci%C3%B3nChile").unwrap(),
            ChannelReference::Handle("RadioEmancipaciónChile".to_string())
        );
    }

    #[test]
    fn bare_at_prefix_also_decodes_percent_encoding() {
        assert_eq!(
            extract_channel_reference("@RadioEmancipaci%C3%B3nChile").unwrap(),
            ChannelReference::Handle("RadioEmancipaciónChile".to_string())
        );
    }

    #[test]
    fn extracts_id_from_channel_url() {
        assert_eq!(
            extract_channel_reference("https://www.youtube.com/channel/UCLYtSCJIkm-8hOZoBHfaHCA").unwrap(),
            ChannelReference::Id("UCLYtSCJIkm-8hOZoBHfaHCA".to_string())
        );
    }

    #[test]
    fn extracts_bare_channel_id() {
        assert_eq!(
            extract_channel_reference("UCLYtSCJIkm-8hOZoBHfaHCA").unwrap(),
            ChannelReference::Id("UCLYtSCJIkm-8hOZoBHfaHCA".to_string())
        );
    }

    #[test]
    fn rejects_legacy_custom_url_instead_of_guessing() {
        let result = extract_channel_reference("https://www.youtube.com/c/SomeCustomName");
        assert!(result.is_err(), "no debe adivinar una resolución para /c/, debe pedir un formato soportado");
    }

    #[test]
    fn parse_channels_response_extracts_id_and_uploads_playlist() {
        let body = r#"{
            "items": [{
                "id": "UCLYtSCJIkm-8hOZoBHfaHCA",
                "contentDetails": {
                    "relatedPlaylists": { "uploads": "UULYtSCJIkm-8hOZoBHfaHCA" }
                }
            }]
        }"#;
        let resolved = parse_channels_response(body).unwrap();
        assert_eq!(resolved.channel_id, "UCLYtSCJIkm-8hOZoBHfaHCA");
        assert_eq!(resolved.uploads_playlist_id, "UULYtSCJIkm-8hOZoBHfaHCA");
    }

    #[test]
    fn parse_channels_response_errors_honestly_on_empty_items() {
        let body = r#"{"items": []}"#;
        assert!(parse_channels_response(body).is_err());
    }

    #[test]
    fn parse_playlist_items_response_extracts_videos_and_next_page_token() {
        let body = r#"{
            "items": [
                {
                    "snippet": {
                        "title": "Episodio 1",
                        "publishedAt": "2024-01-01T00:00:00Z",
                        "resourceId": { "videoId": "abc123" },
                        "thumbnails": {
                            "default": { "url": "https://i.ytimg.com/vi/abc123/default.jpg" },
                            "high": { "url": "https://i.ytimg.com/vi/abc123/hqdefault.jpg" }
                        }
                    }
                }
            ],
            "nextPageToken": "CAUQAA"
        }"#;
        let (videos, next) = parse_playlist_items_response(body).unwrap();
        assert_eq!(videos.len(), 1);
        assert_eq!(videos[0].video_id, "abc123");
        assert_eq!(videos[0].title, "Episodio 1");
        assert_eq!(videos[0].thumbnail_url.as_deref(), Some("https://i.ytimg.com/vi/abc123/hqdefault.jpg"));
        assert_eq!(next.as_deref(), Some("CAUQAA"));
    }

    #[test]
    fn parse_playlist_items_response_falls_back_to_default_thumbnail() {
        let body = r#"{
            "items": [{
                "snippet": {
                    "title": "Sin thumbnail alto",
                    "publishedAt": "2024-01-01T00:00:00Z",
                    "resourceId": { "videoId": "xyz789" },
                    "thumbnails": { "default": { "url": "https://i.ytimg.com/vi/xyz789/default.jpg" } }
                }
            }]
        }"#;
        let (videos, _) = parse_playlist_items_response(body).unwrap();
        assert_eq!(videos[0].thumbnail_url.as_deref(), Some("https://i.ytimg.com/vi/xyz789/default.jpg"));
    }

    #[test]
    fn parse_playlist_items_response_handles_no_next_page() {
        let body = r#"{"items": []}"#;
        let (videos, next) = parse_playlist_items_response(body).unwrap();
        assert!(videos.is_empty());
        assert_eq!(next, None);
    }
}
