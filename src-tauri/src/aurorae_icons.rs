// Fallback adicional para los glifos de ventana en Linux/KDE cuando la
// decoración activa es un tema Aurorae (basado en archivos SVG en disco)
// en vez de un plugin compilado (Breeze/Lightly/Klassy — esos no tienen
// ningún archivo que leer, por diseño; ver native_icons.rs y el registro
// fechado en Planes_mejora_popcorn/set_iconos_lucide.txt).
//
// Confirmado leyendo 3 temas Aurorae reales instalados en esta máquina
// (Layan en /usr/share/aurorae/themes, Dracula y WhiteSur en
// ~/.local/share/aurorae/themes): close/minimize/maximize/restore.svg no
// son íconos planos — son un único documento SVG con un grupo por estado
// (active-center/inactive-center/deactivated-center/hover-center/
// pressed-center), misma convención de ID en los 3 temas. Se usa
// "active-center" (ventana enfocada, sin hover/press) como el glifo de
// reposo — el hover/press ya lo da el propio botón de TitleBar.tsx vía
// clases de Tailwind, no hace falta el estado interactivo del tema.
//
// Detección NO verificada en vivo contra un kwinrc real con Aurorae
// activo (Mandato 1): la máquina de desarrollo usa Breeze (compilado), no
// Aurorae. El formato esperado (`library=org.kde.kwin.aurorae` +
// `theme=__aurorae__svg__<Nombre>`) es la convención documentada de KDE,
// no algo confirmado en esta sesión. Si el formato real difiere, estas
// funciones devuelven `None` y el llamador cae al tier siguiente (tema de
// íconos GTK / Lucide) — nunca rompe ni panickea.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const AURORAE_LIBRARY: &str = "org.kde.kwin.aurorae";
const AURORAE_THEME_PREFIX: &str = "__aurorae__svg__";
const STATE_GROUP_ID: &str = "active-center";

/// Devuelve los 4 glifos (mismas claves que native_icons.rs: close/
/// minimize/maximize/restore) como data URI si la decoración activa es un
/// tema Aurorae real y se pudo extraer al menos uno — `None` si no aplica
/// (decoración compilada, o cualquier paso de la detección falla), para
/// que el llamador siga con el tema de íconos GTK sin ninguna señal de
/// error visible al usuario.
pub fn resolve_all() -> Option<HashMap<String, String>> {
    let theme_dir = active_aurorae_theme_dir()?;
    const FILES: [(&str, &str); 4] = [
        ("close", "close.svg"),
        ("minimize", "minimize.svg"),
        ("maximize", "maximize.svg"),
        ("restore", "restore.svg"),
    ];
    let mut out = HashMap::new();
    for (key, filename) in FILES {
        if let Some(data_uri) = extract_active_center_data_uri(&theme_dir.join(filename)) {
            out.insert(key.to_string(), data_uri);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn active_aurorae_theme_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let kwinrc = Path::new(&home).join(".config/kwinrc");
    let contents = std::fs::read_to_string(kwinrc).ok()?;
    let section = ini_section(&contents, "org.kde.kdecoration2")?;
    if section.get("library").map(String::as_str) != Some(AURORAE_LIBRARY) {
        return None;
    }
    let theme_key = section.get("theme")?;
    let theme_name = theme_key.strip_prefix(AURORAE_THEME_PREFIX)?;
    find_theme_dir(&home, theme_name)
}

// Parser INI ad-hoc mínimo: alcanza con extraer una sola sección plana
// clave=valor (lo único que necesitamos de kwinrc), no pretende cubrir el
// formato completo de KConfig (grupos anidados, listas, $i18n, etc.).
fn ini_section(contents: &str, section_name: &str) -> Option<HashMap<String, String>> {
    let mut in_section = false;
    let mut map = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if in_section {
                break; // ya recorrimos toda la sección que buscábamos
            }
            in_section = name == section_name;
            continue;
        }
        if in_section {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

fn find_theme_dir(home: &str, theme_name: &str) -> Option<PathBuf> {
    [
        PathBuf::from(home).join(".local/share/aurorae/themes").join(theme_name),
        PathBuf::from("/usr/share/aurorae/themes").join(theme_name),
    ]
    .into_iter()
    .find(|p| p.is_dir())
}

fn extract_active_center_data_uri(svg_path: &Path) -> Option<String> {
    let svg = extract_active_center_svg(svg_path)?;
    Some(format!("data:image/svg+xml;base64,{}", STANDARD.encode(svg)))
}

/// Extrae el grupo `id="active-center"` de un SVG de Aurorae y lo envuelve
/// en un documento SVG standalone válido, reusando `width`/`height`/
/// `viewBox` del documento original — el `transform` interno del grupo ya
/// lo posiciona bien dentro de ese lienzo, no hace falta normalizarlo.
fn extract_active_center_svg(svg_path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(svg_path).ok()?;
    let doc = roxmltree::Document::parse(&contents).ok()?;
    let root = doc.root_element();
    let group = root
        .descendants()
        .find(|n| n.is_element() && n.attribute("id") == Some(STATE_GROUP_ID))?;

    let width = root.attribute("width").unwrap_or("22");
    let height = root.attribute("height").unwrap_or("22");
    let view_box = root
        .attribute("viewBox")
        .map(|v| format!(r#" viewBox="{v}""#))
        .unwrap_or_default();

    Some(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"{view_box}>{}</svg>"#,
        node_to_string(group)
    ))
}

// roxmltree es solo de lectura (no re-serializa nodos) — se reconstruye
// el `<g>` a mano recorriendo atributos e hijos. Alcanza para las formas
// simples (g/rect/circle/ellipse/path) que usan estos temas reales; un
// SVG con features más raras (use/symbol externos) perdería esos
// elementos, degradando a un glifo incompleto en vez de romper — mismo
// espíritu "nunca rompe" que el resto de este módulo.
fn node_to_string(node: roxmltree::Node) -> String {
    if node.is_text() {
        return node.text().unwrap_or_default().to_string();
    }
    if !node.is_element() {
        return String::new();
    }
    let tag = node.tag_name().name();
    let attrs: String = node
        .attributes()
        .map(|a| format!(r#" {}="{}""#, a.name(), a.value()))
        .collect();
    let children: String = node.children().map(node_to_string).collect();
    if children.is_empty() {
        format!("<{tag}{attrs}/>")
    } else {
        format!("<{tag}{attrs}>{children}</{tag}>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_section_extracts_only_the_requested_section() {
        let contents = "[General]\nfoo=bar\n[org.kde.kdecoration2]\nlibrary=org.kde.kwin.aurorae\ntheme=__aurorae__svg__Layan\n[Other]\nbaz=qux\n";
        let section = ini_section(contents, "org.kde.kdecoration2").unwrap();
        assert_eq!(section.get("library").map(String::as_str), Some("org.kde.kwin.aurorae"));
        assert_eq!(section.get("theme").map(String::as_str), Some("__aurorae__svg__Layan"));
        assert_eq!(section.get("foo"), None, "no debe filtrar claves de otras secciones");
        assert_eq!(section.get("baz"), None, "no debe filtrar claves de otras secciones");
    }

    #[test]
    fn ini_section_returns_none_for_missing_section() {
        assert!(ini_section("[General]\nfoo=bar\n", "org.kde.kdecoration2").is_none());
    }

    #[test]
    fn ini_section_reports_a_compiled_decoration_library_as_is() {
        // Regresión del caso real de esta sesión (kwinrc real del
        // usuario): decoración compilada (Breeze), no Aurorae — la
        // sección se parsea igual, es active_aurorae_theme_dir quien debe
        // descartarla comparando contra AURORAE_LIBRARY.
        let contents = "[org.kde.kdecoration2]\nlibrary=org.kde.breeze\n";
        let section = ini_section(contents, "org.kde.kdecoration2").unwrap();
        assert_ne!(section.get("library").map(String::as_str), Some(AURORAE_LIBRARY));
    }

    #[test]
    fn extract_active_center_svg_pulls_only_that_group_and_keeps_canvas_size() {
        let dir = std::env::temp_dir().join(format!("popcorn-aurorae-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg_path = dir.join("close.svg");
        std::fs::write(
            &svg_path,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22" viewBox="0 0 22 22">
                <g id="active-center" transform="translate(1,1)"><rect width="10" height="10" fill="#fff"/></g>
                <g id="pressed-center" transform="translate(1,50)"><rect width="10" height="10" fill="#000"/></g>
            </svg>"##,
        )
        .unwrap();

        let svg = extract_active_center_svg(&svg_path).expect("debe encontrar el grupo active-center");

        assert!(svg.contains(r#"width="22""#));
        assert!(svg.contains(r#"height="22""#));
        assert!(svg.contains(r#"viewBox="0 0 22 22""#));
        assert!(svg.contains(r#"id="active-center""#));
        assert!(svg.contains(r##"fill="#fff""##), "debe conservar el contenido del grupo active-center");
        assert!(!svg.contains(r##"fill="#000""##), "no debe filtrar contenido de otros estados (pressed-center)");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn extract_active_center_svg_returns_none_when_group_is_absent() {
        let dir = std::env::temp_dir().join(format!("popcorn-aurorae-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let svg_path = dir.join("close.svg");
        std::fs::write(
            &svg_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="22" height="22"><rect width="10" height="10"/></svg>"#,
        )
        .unwrap();

        assert!(extract_active_center_svg(&svg_path).is_none(), "tema que no sigue la convención debe degradar a None, nunca romper");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Prueba de sistema real (Mandato 12): corre contra los temas Aurorae
    /// de verdad instalados en esta máquina (no fixtures) para confirmar
    /// que la convención de IDs realmente se cumple, no solo en el SVG
    /// sintético de arriba. `#[ignore]` porque depende de rutas de disco
    /// que no existen en un entorno de CI limpio.
    #[test]
    #[ignore = "depende de temas Aurorae reales instalados en esta máquina (Layan/Dracula/WhiteSur) — no existen en un CI limpio"]
    fn extracts_active_center_from_real_installed_aurorae_themes() {
        let real_themes = [
            "/usr/share/aurorae/themes/Layan/close.svg",
            "/home/entropia/.local/share/aurorae/themes/Dracula/close.svg",
            "/home/entropia/.local/share/aurorae/themes/WhiteSur/close.svg",
        ];
        for path in real_themes {
            let svg = extract_active_center_svg(Path::new(path));
            assert!(svg.is_some(), "{path} debería tener un grupo active-center real");
        }
    }
}
