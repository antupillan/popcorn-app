use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

pub struct Db(pub Mutex<Connection>);

// Each entry runs once, in order, tracked via PRAGMA user_version. Append new
// migrations to this slice instead of editing old ones — the schema is a log,
// not a snapshot, so past installs upgrade deterministically.
const MIGRATIONS: &[&str] = &[
    r#"
    CREATE TABLE media_items (
        id TEXT PRIMARY KEY,
        source_type TEXT NOT NULL CHECK (source_type IN ('archive_org', 'magnet', 'torrent_file')),
        source_identifier TEXT NOT NULL,
        title TEXT NOT NULL,
        year INTEGER,
        license TEXT,
        engine_torrent_id TEXT,
        is_private INTEGER NOT NULL DEFAULT 0,
        added_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    "#,
    r#"
    -- Sin filas semilla: la app nunca lista ni recomienda indexers de
    -- contenido con copyright (ver plan, blindaje legal). El usuario agrega
    -- los suyos manualmente, uno por uno.
    CREATE TABLE indexers (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        search_url_template TEXT NOT NULL,
        result_format TEXT NOT NULL CHECK (result_format IN ('magnet_list', 'rss', 'json')),
        json_paths TEXT,
        enabled INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    "#,
    r#"
    -- La API key nunca vive acá (Mandato de Configurabilidad Soberana) —
    -- se guarda por separado en el keychain del SO, indexada por este `id`.
    CREATE TABLE ai_providers (
        id TEXT PRIMARY KEY,
        kind TEXT NOT NULL CHECK (kind IN ('gemini', 'openai_compatible')),
        label TEXT NOT NULL,
        model TEXT NOT NULL,
        base_url TEXT,
        active INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    "#,
    r#"
    -- Una fila por fuente de búsqueda (archive.org built-in + cada indexer
    -- BYO), no por resultado — así la curación (IA u otro filtro) se
    -- configura igual sin importar el origen. `id` es 'archive_org' para la
    -- fuente built-in o el `indexers.id` de una fuente BYO (sin FK real
    -- porque 'archive_org' no tiene fila en `indexers`; el borrado en
    -- cascada lo hace remove_indexer a mano). `mediatype_filter` hoy solo lo
    -- usa archive_org (equivalente al parámetro `mediatype` de su API);
    -- queda nullable para el resto porque no tiene sentido genérico todavía.
    CREATE TABLE source_settings (
        id TEXT PRIMARY KEY,
        curation_enabled INTEGER NOT NULL DEFAULT 1,
        mediatype_filter TEXT,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    INSERT INTO source_settings (id, mediatype_filter) VALUES ('archive_org', 'movies');
    "#,
];

fn db_path(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .context("no se pudo resolver el directorio de datos de la app")?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("no se pudo crear {}", dir.display()))?;
    Ok(dir.join("popcorn.sqlite3"))
}

/// pub(crate) para que otros módulos puedan levantar una conexión in-memory
/// ya migrada en sus propios tests (ver `sources::settings::tests`).
pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let current = usize::try_from(current).unwrap_or(0);
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        conn.execute_batch(sql)
            .with_context(|| format!("migración {} falló", i))?;
        conn.pragma_update(None, "user_version", (i + 1) as i64)?;
    }
    Ok(())
}

pub fn open(app: &AppHandle) -> Result<Connection> {
    let path = db_path(app)?;
    let conn = Connection::open(&path)
        .with_context(|| format!("no se pudo abrir {}", path.display()))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    migrate(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_creates_media_items_and_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        // Running again on an already-migrated connection must be a no-op,
        // not re-run CREATE TABLE and fail on "table already exists".
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO media_items (id, source_type, source_identifier, title) \
             VALUES ('1', 'archive_org', 'sita_sings_the_blues', 'Sita Sings the Blues')",
            [],
        )
        .unwrap();

        let title: String = conn
            .query_row("SELECT title FROM media_items WHERE id = '1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(title, "Sita Sings the Blues");

        let is_private: i64 = conn
            .query_row("SELECT is_private FROM media_items WHERE id = '1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(is_private, 0, "is_private debe tener default 0");

        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
    }

    #[test]
    fn indexers_table_has_no_seed_rows() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM indexers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "indexers no debe traer filas precargadas — blindaje legal");

        conn.execute(
            "INSERT INTO indexers (id, name, search_url_template, result_format) \
             VALUES ('1', 'Mi indexer', 'https://example.org/search?q={query}', 'magnet_list')",
            [],
        )
        .unwrap();
        let enabled: i64 = conn
            .query_row("SELECT enabled FROM indexers WHERE id = '1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(enabled, 1, "enabled debe tener default 1");
    }

    #[test]
    fn ai_providers_table_has_no_seed_rows_and_defaults_inactive() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ai_providers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "sin proveedor por defecto — el usuario elige y configura el suyo");

        conn.execute(
            "INSERT INTO ai_providers (id, kind, label, model) \
             VALUES ('1', 'gemini', 'Mi Gemini', 'gemini-2.5-flash')",
            [],
        )
        .unwrap();
        let active: i64 = conn
            .query_row("SELECT active FROM ai_providers WHERE id = '1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(active, 0, "active debe tener default 0");
    }

    #[test]
    fn source_settings_seeds_archive_org_row_with_movies_filter() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let (curation_enabled, mediatype_filter): (i64, Option<String>) = conn
            .query_row(
                "SELECT curation_enabled, mediatype_filter FROM source_settings WHERE id = 'archive_org'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(curation_enabled, 1);
        assert_eq!(mediatype_filter.as_deref(), Some("movies"));

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM source_settings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "archive_org es la única fuente built-in, el resto se crea al agregar un indexer BYO");
    }
}
