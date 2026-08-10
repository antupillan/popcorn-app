// Acento de color real del escritorio, vía el portal freedesktop estándar
// (org.freedesktop.portal.Settings, D-Bus) — solo Linux, KDE/GNOME modernos.
// None en cualquier otro caso (Windows/macOS, o Linux sin xdg-desktop-portal)
// para que el frontend caiga al acento sky fijo, nunca un valor inventado.

#[tauri::command]
pub async fn get_os_accent_color() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        linux_accent().await
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
async fn linux_accent() -> Option<String> {
    let settings = ashpd::desktop::settings::Settings::new().await.ok()?;
    let color = settings.accent_color().await.ok()?;
    Some(to_hex(color.red(), color.green(), color.blue()))
}

fn to_hex(r: f64, g: f64, b: f64) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_hex_known_values() {
        assert_eq!(to_hex(0.0, 0.0, 0.0), "#000000");
        assert_eq!(to_hex(1.0, 1.0, 1.0), "#ffffff");
        // sky-600 real (#0284c7) usado hoy como fallback fijo en el frontend.
        assert_eq!(to_hex(2.0 / 255.0, 132.0 / 255.0, 199.0 / 255.0), "#0284c7");
    }

    #[test]
    fn to_hex_clamps_out_of_range() {
        assert_eq!(to_hex(-1.0, 2.0, 0.5), "#00ff80");
    }

    // Verifica contra el portal real de escritorio (D-Bus) — corre en
    // #[ignore] porque un CI headless sin xdg-desktop-portal activo (KDE/
    // GNOME) no puede completarlo. Correr a mano en un entorno con sesión
    // de escritorio real.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "requiere sesión D-Bus real con xdg-desktop-portal (KDE/GNOME); un CI headless no la tiene"]
    async fn reads_real_accent_color_from_portal() {
        let color = linux_accent().await;
        assert!(color.is_some(), "el portal debería devolver un color en un entorno de escritorio real");
        assert!(color.unwrap().starts_with('#'));
    }
}
