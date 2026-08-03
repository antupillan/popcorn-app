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

fn migrate(conn: &Connection) -> Result<()> {
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
}
