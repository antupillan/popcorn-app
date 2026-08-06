use serde::Serialize;
use tauri::State;

use crate::db::Db;

/// Config de curación por fuente de búsqueda (archive.org built-in + cada
/// indexer BYO) — ver migración `source_settings` en `db/mod.rs` para el
/// razonamiento completo de por qué es una tabla aparte de `indexers`.
#[derive(Serialize, Clone)]
pub struct SourceSettings {
    pub id: String,
    pub label: String,
    pub curation_enabled: bool,
    pub mediatype_filter: Option<String>,
}

#[tauri::command]
pub async fn list_source_settings(db: State<'_, Db>) -> Result<Vec<SourceSettings>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "SELECT s.id, \
               COALESCE(i.name, v.name, \
                 CASE s.id \
                   WHEN 'archive_org' THEN 'archive.org' \
                   WHEN 'public_domain_torrents' THEN 'Public Domain Torrents' \
                   WHEN 'blender_foundation' THEN 'Blender Foundation (Open Movies)' \
                 END \
               ) AS label, \
               s.curation_enabled, s.mediatype_filter \
             FROM source_settings s \
             LEFT JOIN indexers i ON i.id = s.id \
             LEFT JOIN iptv_sources v ON v.id = s.id \
             ORDER BY (s.id = 'archive_org') DESC, s.created_at ASC, s.rowid ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(SourceSettings {
                id: r.get(0)?,
                label: r.get(1)?,
                curation_enabled: r.get::<_, i64>(2)? != 0,
                mediatype_filter: r.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_source_curation_enabled(
    db: State<'_, Db>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let changed = conn
        .execute(
            "UPDATE source_settings SET curation_enabled = ?1 WHERE id = ?2",
            (enabled as i64, &id),
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("no existe configuración de fuente para id {id}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    #[test]
    fn list_query_joins_archive_org_indexer_and_iptv_source_labels() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO indexers (id, name, search_url_template, result_format) \
             VALUES ('idx1', 'Mi Nyaa', 'https://nyaa.si/?q={query}', 'rss')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO source_settings (id) VALUES ('idx1')", [])
            .unwrap();

        conn.execute(
            "INSERT INTO iptv_sources (id, name, source_kind, playlist_url) \
             VALUES ('iptv1', 'Mi lista IPTV', 'url', 'https://example.org/list.m3u')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO source_settings (id) VALUES ('iptv1')", [])
            .unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT s.id, \
                   COALESCE(i.name, v.name, \
                     CASE s.id \
                       WHEN 'archive_org' THEN 'archive.org' \
                       WHEN 'public_domain_torrents' THEN 'Public Domain Torrents' \
                       WHEN 'blender_foundation' THEN 'Blender Foundation (Open Movies)' \
                     END \
                   ) \
                 FROM source_settings s \
                 LEFT JOIN indexers i ON i.id = s.id \
                 LEFT JOIN iptv_sources v ON v.id = s.id \
                 ORDER BY (s.id = 'archive_org') DESC, s.created_at ASC, s.rowid ASC",
            )
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        // archive_org siempre primero (prioridad explícita en el ORDER BY);
        // iptv_org_public/public_domain_torrents/blender_foundation son las
        // otras filas semilla (ver migraciones); idx1/iptv1 son BYO
        // agregadas por este test, en orden de inserción (rowid como
        // desempate de created_at, que puede empatar al segundo entre filas
        // creadas en la misma corrida).
        assert_eq!(
            rows,
            vec![
                ("archive_org".to_string(), "archive.org".to_string()),
                ("iptv_org_public".to_string(), "iptv-org: Canales públicos".to_string()),
                ("public_domain_torrents".to_string(), "Public Domain Torrents".to_string()),
                ("blender_foundation".to_string(), "Blender Foundation (Open Movies)".to_string()),
                ("idx1".to_string(), "Mi Nyaa".to_string()),
                ("iptv1".to_string(), "Mi lista IPTV".to_string()),
            ]
        );
    }
}
