use anyhow::Context;
use m3u8_rs::{parse_playlist_res, KeyMethod, Playlist};

#[derive(Debug)]
pub struct Segment {
    pub uri: String,
    pub duration: f32,
}

#[derive(Debug)]
pub struct MediaPlaylist {
    pub target_duration: u64,
    pub end_list: bool,
    pub segments: Vec<Segment>,
}

/// Parsea la *media playlist* HLS de un canal (segmentos de video — no
/// confundir con la lista de *canales* que parsea `parse.rs`, formato
/// distinto). Usa `m3u8-rs` (ver Cargo.toml) en vez de un parser a mano:
/// acá un bug sutil corrompe en silencio una grabación del usuario, más
/// grave que perder un resultado de búsqueda.
///
/// Rechaza explícitamente, nunca adivina (Mandato 4 de CLAUDE.md), los tres
/// casos que el grabador no soporta en v1: playlist maestra (multi-bitrate
/// — no hay forma honesta de elegir una variante sin que el usuario lo
/// pida), segmentos cifrados (`EXT-X-KEY` con método distinto de `NONE` —
/// descargar bytes cifrados sin descifrarlos produce un archivo inútil
/// disfrazado de grabación completa), y segmentos con `EXT-X-BYTERANGE`
/// (requieren leer un rango específico del recurso del origen, no alcanza
/// con concatenar bytes secuenciales).
pub fn parse_media_playlist(body: &[u8]) -> anyhow::Result<MediaPlaylist> {
    let playlist = parse_playlist_res(body)
        .map_err(|e| anyhow::anyhow!("no se pudo parsear la media playlist HLS: {e}"))?;

    let media = match playlist {
        Playlist::MediaPlaylist(m) => m,
        Playlist::MasterPlaylist(_) => anyhow::bail!(
            "manifest multi-bitrate (playlist maestra) no soportado en v1 para grabación — \
             elegí la URL de una variante específica, no la maestra"
        ),
    };

    for seg in &media.segments {
        if let Some(key) = &seg.key {
            if !matches!(key.method, KeyMethod::None) {
                anyhow::bail!(
                    "segmento cifrado (EXT-X-KEY) no soportado en v1 para grabación"
                );
            }
        }
        if seg.byte_range.is_some() {
            anyhow::bail!("segmento con EXT-X-BYTERANGE no soportado en v1 para grabación");
        }
    }

    Ok(MediaPlaylist {
        target_duration: media.target_duration,
        end_list: media.end_list,
        segments: media
            .segments
            .into_iter()
            .map(|s| Segment { uri: s.uri, duration: s.duration })
            .collect(),
    })
}

/// Si `body` es una playlist maestra (multi-bitrate), resuelve la URL de
/// la variante de mayor `bandwidth` (mejor calidad disponible — grabar
/// prioriza calidad sobre el streaming adaptativo que sí quiere el
/// reproductor en vivo) contra `base_url`. `None` si no es maestra —
/// nada que resolver, `parse_media_playlist` ya la procesa directo.
pub fn resolve_master_variant(body: &[u8], base_url: &reqwest::Url) -> anyhow::Result<Option<String>> {
    let playlist = parse_playlist_res(body)
        .map_err(|e| anyhow::anyhow!("no se pudo parsear el manifest HLS: {e}"))?;
    let Playlist::MasterPlaylist(master) = playlist else {
        return Ok(None);
    };
    let best = master
        .variants
        .iter()
        .filter(|v| !v.is_i_frame)
        .max_by_key(|v| v.bandwidth)
        .context("playlist maestra sin variantes de video reproducibles")?;
    let resolved = base_url
        .join(&best.uri)
        .with_context(|| format!("URI de variante inválida '{}'", best.uri))?;
    Ok(Some(resolved.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fixtures: ejemplos de referencia textuales de RFC 8216 (HLS), no
    // inventados — mismo criterio que usar la API real de un proveedor
    // para las fixtures de ai/gemini.rs.

    const LIVE_PLAYLIST: &[u8] = b"#EXTM3U
#EXT-X-TARGETDURATION:10
#EXT-X-VERSION:3
#EXT-X-MEDIA-SEQUENCE:2680

#EXTINF:9.009,
https://priv.example.com/fileSequence2680.ts
#EXTINF:9.009,
https://priv.example.com/fileSequence2681.ts
#EXTINF:9.009,
https://priv.example.com/fileSequence2682.ts
";

    const VOD_PLAYLIST: &[u8] = b"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-PLAYLIST-TYPE:VOD
#EXTINF:9.009,
http://media.example.com/first.ts
#EXTINF:9.009,
http://media.example.com/second.ts
#EXTINF:3.003,
http://media.example.com/third.ts
#EXT-X-ENDLIST
";

    const MASTER_PLAYLIST: &[u8] = b"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1280000,AVERAGE-BANDWIDTH=1000000
low/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2560000,AVERAGE-BANDWIDTH=2000000
mid/index.m3u8
";

    const ENCRYPTED_PLAYLIST: &[u8] = b"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-MEDIA-SEQUENCE:7794
#EXT-X-TARGETDURATION:15
#EXT-X-KEY:METHOD=AES-128,URI=\"https://priv.example.com/key.bin\"
#EXTINF:2.833,
http://media.example.com/fileSequence52.ts
#EXTINF:15.0,
http://media.example.com/fileSequence53.ts
";

    const BYTERANGE_PLAYLIST: &[u8] = b"#EXTM3U
#EXT-X-VERSION:4
#EXT-X-TARGETDURATION:10
#EXT-X-PLAYLIST-TYPE:VOD
#EXT-X-BYTERANGE:76242@0
#EXTINF:10.0,
video.ts
#EXT-X-BYTERANGE:82112@76242
#EXTINF:10.0,
video.ts
#EXT-X-ENDLIST
";

    #[test]
    fn parses_live_playlist_without_endlist() {
        let playlist = parse_media_playlist(LIVE_PLAYLIST).unwrap();
        assert_eq!(playlist.target_duration, 10);
        assert!(!playlist.end_list, "playlist en vivo no trae EXT-X-ENDLIST");
        assert_eq!(playlist.segments.len(), 3);
        assert_eq!(playlist.segments[0].uri, "https://priv.example.com/fileSequence2680.ts");
        assert!((playlist.segments[0].duration - 9.009).abs() < 0.001);
    }

    #[test]
    fn parses_vod_playlist_with_endlist() {
        let playlist = parse_media_playlist(VOD_PLAYLIST).unwrap();
        assert!(playlist.end_list, "playlist VOD sí trae EXT-X-ENDLIST");
        assert_eq!(playlist.segments.len(), 3);
    }

    #[test]
    fn rejects_master_playlist_explicitly() {
        let err = parse_media_playlist(MASTER_PLAYLIST).unwrap_err();
        assert!(
            err.to_string().contains("maestra"),
            "el error debe explicar que es una playlist maestra, no adivinar una variante: {err}"
        );
    }

    #[test]
    fn rejects_encrypted_segments_explicitly() {
        let err = parse_media_playlist(ENCRYPTED_PLAYLIST).unwrap_err();
        assert!(
            err.to_string().contains("cifrado"),
            "el error debe explicar que hay cifrado, no descargar bytes inútiles en silencio: {err}"
        );
    }

    #[test]
    fn rejects_byte_range_segments_explicitly() {
        let err = parse_media_playlist(BYTERANGE_PLAYLIST).unwrap_err();
        assert!(
            err.to_string().contains("BYTERANGE"),
            "el error debe explicar el motivo real: {err}"
        );
    }

    #[test]
    fn resolve_master_variant_picks_highest_bandwidth_and_resolves_relative_uri() {
        let base = reqwest::Url::parse("http://example.org/live/master.m3u8").unwrap();
        let resolved = resolve_master_variant(MASTER_PLAYLIST, &base).unwrap();
        assert_eq!(
            resolved.as_deref(),
            Some("http://example.org/live/mid/index.m3u8"),
            "debe elegir la variante de mayor bandwidth (2560000 > 1280000), resuelta contra la URL base"
        );
    }

    #[test]
    fn resolve_master_variant_returns_none_for_media_playlist() {
        let base = reqwest::Url::parse("http://example.org/live/index.m3u8").unwrap();
        assert!(resolve_master_variant(VOD_PLAYLIST, &base).unwrap().is_none());
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(parse_media_playlist(b"esto no es un m3u8 valido").is_err());
    }

    #[tokio::test]
    #[ignore = "red real, no apto para CI por defecto — correr manualmente contra un canal público real"]
    async fn rejects_real_live_master_playlist_from_public_broadcaster() {
        // Manifest real de un canal de la fuente semilla iptv_org_public
        // (RTVE "24 Horas" HD, ver categories/public.m3u) — confirmado en
        // vivo que es una playlist maestra (EXT-X-STREAM-INF con variantes
        // de audio/video), el caso real que motiva este rechazo explícito.
        let body = reqwest::get("http://185.47.212.25:8080/24h_HD/index.m3u8")
            .await
            .expect("el canal real debe responder")
            .bytes()
            .await
            .expect("debe poder leerse el body");
        let err = parse_media_playlist(&body).unwrap_err();
        assert!(err.to_string().contains("maestra"), "error real: {err}");
    }

    #[tokio::test]
    #[ignore = "red real, no apto para CI por defecto — correr manualmente contra un canal público real"]
    async fn resolves_real_master_playlist_to_a_playable_media_variant() {
        let url = "http://185.47.212.25:8080/24h_HD/index.m3u8";
        let resp = reqwest::get(url).await.expect("el canal real debe responder");
        let base_url = resp.url().clone();
        let body = resp.bytes().await.expect("debe poder leerse el body");

        let variant_url = resolve_master_variant(&body, &base_url)
            .unwrap()
            .expect("debe reconocer la maestra real y resolver una variante");

        let variant_body = reqwest::get(&variant_url)
            .await
            .expect("la variante resuelta debe responder")
            .bytes()
            .await
            .expect("debe poder leerse el body de la variante");
        parse_media_playlist(&variant_body)
            .expect("la variante resuelta debe ser una media playlist real, grabable");
    }
}
