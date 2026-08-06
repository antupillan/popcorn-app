use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::State;

use crate::commands::EngineState;
use crate::db::Db;

const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "avi", "webm", "mov", "m4v"];

#[derive(Serialize, Clone)]
pub struct LocalFile {
    pub path: String,
    pub name: String,
}

/// Recorre `dir` recursivamente a mano (std::fs, sin crate nuevo — ver
/// Mandato de sincronización de empaquetado) filtrando por extensión de
/// video. Un subdirectorio o entrada ilegible (permisos, symlink roto) se
/// saltea sin abortar el resto del escaneo — mismo criterio que
/// `iptv::list_channels` con una fuente que falla.
fn scan_dir_recursive(dir: &Path, out: &mut Vec<LocalFile>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[popcorn] no se pudo leer {}: {e}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if VIDEO_EXTENSIONS.contains(&ext.to_lowercase().as_str()) {
                out.push(LocalFile {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    path: path.to_string_lossy().to_string(),
                });
            }
        }
    }
}

fn read_local_library_folder(db: &Db) -> Result<Option<String>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = 'local_library_folder'",
        [],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_local_library_folder(db: State<'_, Db>) -> Result<Option<String>, String> {
    read_local_library_folder(&db)
}

/// El selector de carpeta nativo lo invoca el frontend directamente
/// (`@tauri-apps/plugin-dialog`) — este comando solo persiste la elección.
#[tauri::command]
pub async fn set_local_library_folder(db: State<'_, Db>, folder: String) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('local_library_folder', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [&folder],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Escaneo fresco en cada llamada, sin persistir ningún archivo individual
/// — mismo criterio que `list_channels`/`search_indexers` con sus fuentes.
#[tauri::command]
pub async fn list_local_files(db: State<'_, Db>) -> Result<Vec<LocalFile>, String> {
    let Some(folder) = read_local_library_folder(&db)? else {
        return Ok(vec![]);
    };
    let mut out = Vec::new();
    scan_dir_recursive(Path::new(&folder), &mut out);
    Ok(out)
}

#[tauri::command]
pub async fn get_local_stream_url(engine: State<'_, EngineState>, path: String) -> Result<String, String> {
    engine
        .0
        .local_stream_url(PathBuf::from(path))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn migrated_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(std::sync::Mutex::new(conn))
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("popcorn-local-lib-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_dir_recursive_finds_video_files_case_insensitively_and_skips_others() {
        let dir = temp_dir();
        std::fs::write(dir.join("movie.mp4"), b"x").unwrap();
        std::fs::write(dir.join("movie.MKV"), b"x").unwrap();
        std::fs::write(dir.join("readme.txt"), b"x").unwrap();
        let sub = dir.join("subfolder");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("nested.avi"), b"x").unwrap();

        let mut out = Vec::new();
        scan_dir_recursive(&dir, &mut out);

        let names: std::collections::HashSet<String> = out.iter().map(|f| f.name.clone()).collect();
        assert_eq!(names.len(), 3, "debe encontrar los 3 archivos de video, ignorando readme.txt: {names:?}");
        assert!(names.contains("movie.mp4"));
        assert!(names.contains("movie.MKV"));
        assert!(names.contains("nested.avi"), "debe bajar a subcarpetas recursivamente");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_dir_recursive_does_not_abort_on_unreadable_subdirectory() {
        // Directorio inexistente en vez de uno con permisos revocados —
        // más portable entre entornos de test, mismo camino de error
        // (std::fs::read_dir falla) que un subdirectorio sin permisos.
        let dir = temp_dir();
        let mut out = Vec::new();
        scan_dir_recursive(&dir.join("no-existe"), &mut out);
        assert!(out.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn local_library_folder_round_trips_through_app_settings() {
        let db = migrated_db();
        assert_eq!(read_local_library_folder(&db).unwrap(), None);

        {
            let conn = db.0.lock().unwrap();
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES ('local_library_folder', ?1) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                ["/home/user/Videos"],
            )
            .unwrap();
        }
        assert_eq!(read_local_library_folder(&db).unwrap().as_deref(), Some("/home/user/Videos"));

        // Un segundo set (upsert) debe reemplazar, no fallar por PK duplicada.
        {
            let conn = db.0.lock().unwrap();
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES ('local_library_folder', ?1) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                ["/home/user/Movies"],
            )
            .unwrap();
        }
        assert_eq!(read_local_library_folder(&db).unwrap().as_deref(), Some("/home/user/Movies"));
    }
}
