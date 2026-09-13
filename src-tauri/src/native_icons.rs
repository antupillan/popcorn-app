// Íconos reales de window-close/minimize/maximize/restore, con dos tiers
// en Linux: (1) decoración de ventana Aurorae real si está activa (ver
// aurorae_icons.rs — esto es lo que el usuario realmente ve dibujado como
// botones de su ventana, cuando la decoración es de ese tipo), (2) tema
// de íconos activo del usuario (Breeze, Adwaita, etc.) vía los nombres
// estándar del freedesktop icon-naming-spec — un ícono de acciones
// genérico, no necesariamente igual a la decoración real si esta es un
// plugin compilado (Breeze/Lightly/Klassy: sin ningún archivo que leer,
// ver registro fechado en Planes_mejora_popcorn/set_iconos_lucide.txt).
// Solo Linux: Windows/macOS no tienen nada de esto consultable así
// (Fluent/mac usan glifos del sistema, no archivos en disco) — HashMap
// vacío en cualquier otro caso, el frontend cae a los glifos dibujados a
// mano (TitleBar.tsx), nunca se rompe por esto.
//
// El nombre del tema se lee del portal freedesktop (org.gnome.desktop.
// interface, icon-theme) — mismo mecanismo que os_accent.rs, NO de
// gtk::IconTheme::default(). Verificado en vivo que difieren: GTK puede
// quedar con un valor cacheado/desincronizado del archivo de config
// (settings.ini) mientras el portal ya refleja el cambio real — el
// portal es la fuente de verdad que consultan también los paneles de
// Ajustes del sistema.
#[tauri::command]
pub async fn get_native_window_icons(app: tauri::AppHandle) -> std::collections::HashMap<String, String> {
    #[cfg(target_os = "linux")]
    {
        linux_icons(&app).await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        std::collections::HashMap::new()
    }
}

#[cfg(target_os = "linux")]
async fn linux_icons(app: &tauri::AppHandle) -> std::collections::HashMap<String, String> {
    const NAMES: [(&str, &str); 4] = [
        ("close", "window-close-symbolic"),
        ("minimize", "window-minimize-symbolic"),
        ("maximize", "window-maximize-symbolic"),
        ("restore", "window-restore-symbolic"),
    ];
    // Tier 1: decoración de ventana Aurorae real (KWin), si la decoración
    // activa es de ese tipo — ver aurorae_icons.rs. `out` arranca con lo
    // que este tier resuelva; el tema de íconos GTK de abajo solo llena
    // las claves que falten (nunca pisa un ícono de decoración real con
    // uno de tema de íconos genérico).
    let mut out = crate::aurorae_icons::resolve_all().unwrap_or_default();

    let theme_name = current_icon_theme_name().await;
    for (key, icon_name) in NAMES {
        if out.contains_key(key) {
            continue;
        }
        if let Some(data_uri) = lookup_icon_data_uri(app, theme_name.as_deref(), icon_name) {
            out.insert(key.to_string(), data_uri);
        }
    }
    out
}

#[cfg(target_os = "linux")]
async fn current_icon_theme_name() -> Option<String> {
    let settings = ashpd::desktop::settings::Settings::new().await.ok()?;
    settings.read::<String>("org.gnome.desktop.interface", "icon-theme").await.ok()
}

// Escucha cambios de tema de íconos en vivo (portal, mismo namespace que
// current_icon_theme_name) y emite un evento al frontend para que vuelva
// a pedir los íconos — así la titlebar sigue al tema global sin
// necesitar relanzar la app. Se llama una vez desde lib.rs::setup().
#[cfg(target_os = "linux")]
pub fn watch_icon_theme_changes(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use futures::StreamExt;
        use tauri::Emitter;

        let Ok(settings) = ashpd::desktop::settings::Settings::new().await else { return };
        let Ok(mut stream) = settings.receive_setting_changed().await else { return };
        while let Some(setting) = stream.next().await {
            if setting.namespace() == "org.gnome.desktop.interface" && setting.key() == "icon-theme" {
                let _ = app.emit("native-window-icons-changed", ());
            }
        }
    });
}

#[cfg(not(target_os = "linux"))]
pub fn watch_icon_theme_changes(_app: tauri::AppHandle) {}

// gtk-rs exige IconTheme en el hilo principal (assert_initialized_main_thread!
// interno) — un comando de Tauri no corre ahí necesariamente, así que se
// despacha con run_on_main_thread y se recibe el resultado por canal.
// `theme_name`: si el portal dio un nombre, se fuerza ese tema explícito
// (set_custom_theme) en vez de confiar en gtk::IconTheme::default(), que
// puede estar desincronizado (ver comentario de arriba).
#[cfg(target_os = "linux")]
fn lookup_icon_data_uri(app: &tauri::AppHandle, theme_name: Option<&str>, icon_name: &str) -> Option<String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};

    let (tx, rx) = std::sync::mpsc::channel();
    let name = icon_name.to_string();
    let theme_name = theme_name.map(str::to_string);
    app.run_on_main_thread(move || {
        let flags = gtk::IconLookupFlags::FORCE_SYMBOLIC | gtk::IconLookupFlags::FORCE_SVG;
        let theme = match &theme_name {
            Some(name) => {
                let t = gtk::IconTheme::new();
                gtk::prelude::IconThemeExt::set_custom_theme(&t, Some(name.as_str()));
                Some(t)
            }
            None => gtk::IconTheme::default(),
        };
        let path = theme
            .and_then(|theme| gtk::prelude::IconThemeExt::lookup_icon(&theme, &name, 16, flags))
            .and_then(|info| info.filename());
        let _ = tx.send(path);
    })
    .ok()?;

    let path = rx.recv().ok().flatten()?;
    let bytes = std::fs::read(&path).ok()?;
    let mime = if path.extension().and_then(|e| e.to_str()) == Some("svg") {
        "image/svg+xml"
    } else {
        "image/png"
    };
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}
