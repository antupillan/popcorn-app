use regex::Regex;

use super::Channel;

fn attr_re(name: &str) -> Regex {
    Regex::new(&format!(r#"{name}="([^"]*)""#)).unwrap()
}

fn extract_attr(re: &Regex, line: &str) -> Option<String> {
    re.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .filter(|s| !s.is_empty())
}

/// El nombre del canal es todo lo que sigue a la coma que separa los
/// atributos `key="value"` del título — no necesariamente la ÚLTIMA coma
/// de la línea, porque un título real puede traer comas propias (ej.
/// "Canal 24 Horas, Edición Fin de Semana"). Los atributos siempre
/// terminan en una comilla de cierre, así que se busca la primera coma
/// DESPUÉS de la última comilla de la línea — si no hay comillas (EXTINF
/// sin atributos), se usa la primera coma de toda la línea.
fn extract_name(line: &str) -> Option<String> {
    let search_start = line.rfind('"').map(|i| i + 1).unwrap_or(0);
    let comma_idx = line[search_start..].find(',')?;
    let name = line[search_start + comma_idx + 1..].trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Playlist M3U de *canales* (formato de agregador IPTV: `#EXTINF` con
/// atributos `tvg-id`/`tvg-logo`/`group-title` + nombre, seguida de la URL
/// del stream) — no confundir con la *media playlist* HLS de un canal
/// individual (esa la parsea `hls_playlist.rs`, formato distinto con
/// segmentos en vez de canales). A mano con `regex`, mismo criterio que
/// `indexers/parse.rs`: no vale la pena un parser M3U completo para un
/// formato línea-por-línea tan simple.
pub fn channel_list(body: &str) -> Vec<Channel> {
    let group_re = attr_re("group-title");
    let logo_re = attr_re("tvg-logo");
    let id_re = attr_re("tvg-id");

    let mut channels = Vec::new();
    let mut lines = body.lines().peekable();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if !line.starts_with("#EXTINF") {
            continue;
        }
        let Some(name) = extract_name(line) else {
            continue;
        };

        // La URL es la siguiente línea no vacía que no sea otra directiva
        // #EXT* — algunas listas insertan #EXTVLCOPT/#EXTGRP entre el
        // EXTINF y la URL real.
        let mut url = None;
        while let Some(next) = lines.peek() {
            let next_trim = next.trim();
            if next_trim.is_empty() || next_trim.starts_with('#') {
                lines.next();
                continue;
            }
            url = Some(next_trim.to_string());
            lines.next();
            break;
        }

        if let Some(url) = url {
            channels.push(Channel {
                name,
                url,
                group: extract_attr(&group_re, line),
                logo_url: extract_attr(&logo_re, line),
                tvg_id: extract_attr(&id_re, line),
                source_id: String::new(),
            });
        }
    }
    channels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_shaped_iptv_org_public_category_entries() {
        // Extracto real de https://iptv-org.github.io/iptv/categories/public.m3u
        // (verificado en vivo contra la fuente antes de sembrarla como
        // fuente IPTV por defecto — ver migración iptv_sources).
        let body = r#"#EXTM3U
#EXTINF:-1 tvg-id="3CatCameresdeltemps.es@SD" tvg-logo="https://i.imgur.com/zXy2kbe.png" group-title="Public;Weather",3Cat Càmeres del temps (1080p)
https://directes-tv-int.3catdirectes.cat/live-content/beauties-hls/master.m3u8
#EXTINF:-1 tvg-id="24Horas.es@SD" tvg-logo="https://i.ibb.co/21sXZ3GT/24h.png" group-title="News;Public",24 Horas (1080p)
http://185.47.212.25:8080/24h_HD/index.m3u8
"#;
        let channels = channel_list(body);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "3Cat Càmeres del temps (1080p)");
        assert_eq!(channels[0].tvg_id.as_deref(), Some("3CatCameresdeltemps.es@SD"));
        assert_eq!(channels[0].group.as_deref(), Some("Public;Weather"));
        assert_eq!(channels[0].logo_url.as_deref(), Some("https://i.imgur.com/zXy2kbe.png"));
        assert_eq!(
            channels[0].url,
            "https://directes-tv-int.3catdirectes.cat/live-content/beauties-hls/master.m3u8"
        );
        assert_eq!(channels[1].name, "24 Horas (1080p)");
        assert_eq!(channels[1].url, "http://185.47.212.25:8080/24h_HD/index.m3u8");
    }

    #[test]
    fn name_with_embedded_comma_is_not_truncated() {
        let body = r#"#EXTINF:-1 group-title="News",Canal 24 Horas, Edición Fin de Semana
https://example.org/stream.m3u8"#;
        let channels = channel_list(body);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Canal 24 Horas, Edición Fin de Semana");
    }

    #[test]
    fn extinf_without_any_attributes_still_parses() {
        let body = "#EXTINF:-1,Canal Simple\nhttps://example.org/simple.m3u8";
        let channels = channel_list(body);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Canal Simple");
        assert_eq!(channels[0].group, None);
        assert_eq!(channels[0].logo_url, None);
        assert_eq!(channels[0].tvg_id, None);
    }

    #[test]
    fn skips_extra_directives_between_extinf_and_url() {
        let body = "#EXTINF:-1,Canal Con VLC Opts\n#EXTVLCOPT:network-caching=1000\nhttps://example.org/vlc.m3u8";
        let channels = channel_list(body);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].url, "https://example.org/vlc.m3u8");
    }

    #[test]
    fn extinf_without_a_following_url_is_dropped() {
        let body = "#EXTM3U\n#EXTINF:-1,Canal Huérfano";
        assert!(channel_list(body).is_empty());
    }

    #[test]
    fn empty_body_returns_no_channels() {
        assert!(channel_list("").is_empty());
    }
}
