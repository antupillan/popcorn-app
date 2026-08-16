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
    r#"
    -- Una sola fila semilla, mismo criterio que `archive_org` en
    -- source_settings (no el de `indexers`, que va a cero filas porque
    -- agrega contenido con copyright arbitrario): la categoría "Public" que
    -- el propio proyecto iptv-org clasifica y mantiene activamente son
    -- streams oficiales de radiodifusores públicos (RTVE, CCMA, Canal Sur,
    -- etc.) que esos mismos canales transmiten abiertamente — no es una
    -- certificación de licencia de redistribución (a diferencia de
    -- archive.org), es una clasificación de género/propiedad, pero coincide
    -- con "TV pública" tal como se pidió. Verificado en vivo antes de
    -- sembrarlo: 39 canales reales, formato M3U válido. El resto de
    -- iptv_sources es BYO igual que `indexers` — el usuario agrega sus
    -- propias listas además de esta. `source_kind='file'` guarda los bytes
    -- subidos en disco (app_data_dir/iptv/playlists/{id}.m3u), no acá;
    -- `playlist_url` queda NULL en ese caso. Ningún canal individual se
    -- persiste — `list_channels` refetchea/reparsea la lista en cada
    -- llamada, igual que `search_indexers` con los indexers.
    CREATE TABLE iptv_sources (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        source_kind TEXT NOT NULL CHECK (source_kind IN ('url', 'file')),
        playlist_url TEXT,
        enabled INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    INSERT INTO iptv_sources (id, name, source_kind, playlist_url) VALUES (
        'iptv_org_public',
        'iptv-org: Canales públicos',
        'url',
        'https://iptv-org.github.io/iptv/categories/public.m3u'
    );
    INSERT INTO source_settings (id) VALUES ('iptv_org_public');
    "#,
    r#"
    -- A diferencia de source_settings/resultados de búsqueda, esto sí
    -- persiste: una grabación tiene que sobrevivir un reinicio de la app
    -- para poder listarse después. `source_id` sin FK real (mismo
    -- precedente que source_settings -> indexers) para que una grabación
    -- de una fuente ya borrada siga listada. `file_name` es relativo a
    -- app_data_dir/iptv/recordings/.
    CREATE TABLE iptv_recordings (
        id TEXT PRIMARY KEY,
        source_id TEXT,
        channel_name TEXT NOT NULL,
        manifest_url TEXT NOT NULL,
        file_name TEXT NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('recording', 'stopped', 'error')),
        error TEXT,
        bytes_written INTEGER NOT NULL DEFAULT 0,
        started_at TEXT NOT NULL DEFAULT (datetime('now')),
        stopped_at TEXT
    );
    "#,
    r#"
    -- `source_settings` ya está commiteada de una sesión anterior — no se
    -- edita esa migración (log, no snapshot), se agrega la columna acá.
    -- Criterio de curación en texto libre, por fuente. Nulo en BYO por
    -- defecto (el usuario lo escribe si quiere, ver curate_by_hint): "el
    -- usuario agregó la lista, la curación sigue sus propios parámetros, no
    -- un juicio de legalidad de la app" (mismo principio que indexers: cero
    -- filas semilla, el contenido BYO es responsabilidad de quien lo agrega).
    -- Único caso con valor precargado: la fuente semilla iptv_org_public,
    -- donde sí es la app la que vouches por el criterio ("radiodifusores
    -- públicos oficiales"), no el usuario.
    ALTER TABLE source_settings ADD COLUMN curation_hint TEXT;
    UPDATE source_settings SET curation_hint = 'radiodifusores públicos oficiales' WHERE id = 'iptv_org_public';
    "#,
    r#"
    -- Config local clave-valor genérica (hoy solo `local_library_folder`) —
    -- evita una migración nueva por cada preferencia suelta que no amerita
    -- su propia tabla tipada.
    CREATE TABLE app_settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
    "#,
    r#"
    -- Seeds de source_settings para las fuentes Online nuevas (Biblioteca
    -- unificada). A diferencia de `indexers` (cero filas semilla, blindaje
    -- legal contra copyright arbitrario), estas dos son catálogo legal
    -- verificado por la app misma, mismo criterio que la fila 'archive_org'
    -- ya sembrada. `blender_foundation` desactiva curación: es una allowlist
    -- ya vetted título por título a mano, someterla a IA no suma nada.
    INSERT INTO source_settings (id, curation_hint) VALUES (
        'public_domain_torrents',
        'películas reales de dominio público del catálogo de publicdomaintorrents.info, no basura ni entradas rotas del feed'
    );
    INSERT INTO source_settings (id, curation_enabled, curation_hint) VALUES (
        'blender_foundation', 0,
        'cortos y películas oficiales de Blender Foundation / Blender Studio'
    );
    UPDATE source_settings SET curation_hint =
      'películas reales, no archivos de prueba, demos técnicos ni vlogs genéricos'
      WHERE id = 'archive_org';
    "#,
    r#"
    -- SQLite no soporta ALTER TABLE ... DROP CHECK: agregar
    -- 'public_domain_torrents' como source_type válido exige reconstruir la
    -- tabla. Nada referencia media_items.id como FK entrante hoy, no hace
    -- falta PRAGMA foreign_keys=OFF/ON.
    CREATE TABLE media_items_new (
        id TEXT PRIMARY KEY,
        source_type TEXT NOT NULL CHECK (source_type IN
            ('archive_org', 'magnet', 'torrent_file', 'public_domain_torrents')),
        source_identifier TEXT NOT NULL,
        title TEXT NOT NULL,
        year INTEGER,
        license TEXT,
        engine_torrent_id TEXT,
        is_private INTEGER NOT NULL DEFAULT 0,
        added_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    INSERT INTO media_items_new SELECT * FROM media_items;
    DROP TABLE media_items;
    ALTER TABLE media_items_new RENAME TO media_items;
    "#,
    r#"
    -- Dos colecciones curadas de archive.org nuevas para Online (misma
    -- confianza que archive_org/public_domain_torrents: catálogo legal
    -- verificado por la app, no BYO). No suman source_type nuevo en
    -- media_items — resuelven por identifier de archive.org igual que
    -- archive_org/blender_foundation (ver online_library.rs). Ambas con
    -- curación activa a diferencia de blender_foundation: son colecciones
    -- grandes de calidad mixta, no una allowlist vetted título por título.
    INSERT INTO source_settings (id, curation_hint) VALUES (
        'prelinger',
        'films reales de la colección Prelinger (educativos, industriales, históricos), no fragmentos técnicos ni duplicados'
    );
    INSERT INTO source_settings (id, curation_hint) VALUES (
        'feature_films',
        'películas reales de las colecciones de cine clásico de archive.org, no rips de baja calidad ni duplicados'
    );
    "#,
    r#"
    -- Caché de curación IA (puntaje absoluto, no orden relativo — ver plan
    -- "Caché de curación IA con puntaje absoluto") + tracking de
    -- disponibilidad (ping), ambos por ítem, compartiendo identidad
    -- (source_id = mismo id que source_settings; item_key = "{kind}:{identifier}"
    -- para Online, url del canal para IPTV). hint_used es la clave de
    -- invalidación: si difiere del curation_hint vigente, el ítem se
    -- re-cura. consecutive_ping_failures es independiente de la curación
    -- IA — se actualiza en cada sesión sin importar si hubo re-curación.
    CREATE TABLE curation_cache (
        source_id TEXT NOT NULL,
        item_key TEXT NOT NULL,
        included INTEGER NOT NULL DEFAULT 1,
        score INTEGER,
        hint_used TEXT,
        consecutive_ping_failures INTEGER NOT NULL DEFAULT 0,
        last_ping_at TEXT,
        cached_at TEXT NOT NULL DEFAULT (datetime('now')),
        PRIMARY KEY (source_id, item_key)
    );
    "#,
    r#"
    -- Fuente YouTube (catálogo temático cine/series/anime, ver plan
    -- fuente_youtube.txt): BYO igual que `indexers`, cero filas semilla en
    -- esta migración. El plan preveía sembrar 1-2 canales oficiales
    -- verificados en vivo (mismo criterio que iptv_org_public), pero esa
    -- verificación requiere una youtube_api_key real contra la Data API y
    -- no hay una disponible en este entorno de desarrollo — sembrar sin
    -- verificar sería alucinar una fuente "confirmada" que no se confirmó
    -- (Mandato 1). Queda como ítem de backlog explícito, no omitido en
    -- silencio. `category` la fija el usuario al agregar el canal, nunca
    -- inferida. `channel_id`/`uploads_playlist_id` se resuelven y cachean
    -- en el alta (ver youtube::resolve_channel) para no volver a pegarle a
    -- channels.list en cada listado de videos. Se guardan ambos en vez de
    -- derivar uploads_playlist_id de channel_id vía el truco no oficial
    -- "reemplazar el prefijo UC por UU" — no está garantizado por la
    -- documentación de la Data API, así que asumirlo sería el tipo de
    -- comportamiento no verificado que el Mandato 4 prohíbe.
    CREATE TABLE youtube_sources (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        channel_url TEXT NOT NULL,
        channel_id TEXT,
        uploads_playlist_id TEXT,
        category TEXT NOT NULL CHECK (category IN ('cine', 'series', 'anime')),
        enabled INTEGER NOT NULL DEFAULT 1,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    "#,
    r#"
    -- Subtítulos (Parte A del plan, ver Planes_mejora_popcorn/subtitulos_ia.txt)
    -- — solo media_items (Mi Colección) y Biblioteca Local, ambos con
    -- duración fija y reproducidos por el <video> propio. `origin` distingue
    -- lo subido/pegado por el usuario de lo traducido por IA o editado a
    -- mano, nunca se pierde esa procedencia. `content` es el SRT completo
    -- (no cues sueltas) — parseo/reserializado vive en el frontend
    -- (src/lib/srt.ts), el backend solo persiste el blob.
    CREATE TABLE subtitles (
        id TEXT PRIMARY KEY,
        media_item_id TEXT NOT NULL,
        language TEXT NOT NULL,
        origin TEXT NOT NULL CHECK (origin IN ('original', 'ai_translated', 'human_edited')),
        content TEXT NOT NULL,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    "#,
    r#"
    -- Bug real reportado en vivo: el reproductor asumía file_idx=0 como "el
    -- video" (ver resolve_media_item_stream_url) — pero un torrent real de
    -- archive.org trae, además del/los video(s), transcripts/subtítulos/
    -- miniaturas/metadata como archivos propios, y en orden alfabético el
    -- índice 0 puede caer en cualquiera de esos (visto en vivo: un .asr.js
    -- de 135KB, no un video). NULL para filas existentes — se resuelve una
    -- vez, en la próxima alta/sanación de cada ítem (ver
    -- sources::archive_org::resolve_primary_file_idx), no requiere backfill
    -- inmediato ni bloquea nada mientras tanto.
    ALTER TABLE media_items ADD COLUMN primary_file_idx INTEGER;
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
        assert_eq!(count, 6, "archive_org + iptv_org_public + public_domain_torrents + blender_foundation + prelinger + feature_films son las seis fuentes built-in; el resto se crea al agregar una fuente BYO");
    }

    #[test]
    fn source_settings_seeds_prelinger_and_feature_films_with_curation_enabled() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        for id in ["prelinger", "feature_films"] {
            let (curation_enabled, hint): (i64, Option<String>) = conn
                .query_row(
                    "SELECT curation_enabled, curation_hint FROM source_settings WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(curation_enabled, 1, "{id} es una colección grande de calidad mixta, no una allowlist vetted a mano");
            assert!(hint.is_some(), "{id} debe traer curation_hint seteado por la migración");
        }
    }

    #[test]
    fn curation_hint_defaults_null_except_for_seeded_sources() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let archive_org_hint: Option<String> = conn
            .query_row(
                "SELECT curation_hint FROM source_settings WHERE id = 'archive_org'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            archive_org_hint.as_deref(),
            Some("películas reales, no archivos de prueba, demos técnicos ni vlogs genéricos"),
            "seteado por la migración de Biblioteca unificada — la app vouches por el criterio de su propio catálogo por defecto"
        );

        let iptv_public_hint: Option<String> = conn
            .query_row(
                "SELECT curation_hint FROM source_settings WHERE id = 'iptv_org_public'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            iptv_public_hint.as_deref(),
            Some("radiodifusores públicos oficiales"),
            "única fuente donde la app fija el criterio, no el usuario"
        );
    }

    #[test]
    fn iptv_sources_seeds_exactly_the_iptv_org_public_row() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM iptv_sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "una sola fila semilla (iptv_org_public) — el resto es BYO, igual que indexers");

        let (source_kind, playlist_url, enabled): (String, Option<String>, i64) = conn
            .query_row(
                "SELECT source_kind, playlist_url, enabled FROM iptv_sources WHERE id = 'iptv_org_public'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(source_kind, "url");
        assert_eq!(playlist_url.as_deref(), Some("https://iptv-org.github.io/iptv/categories/public.m3u"));
        assert_eq!(enabled, 1);

        // Debe haber registrado su fila espejo en source_settings, mismo
        // contrato de doble-insert que add_indexer.
        let curation_enabled: i64 = conn
            .query_row(
                "SELECT curation_enabled FROM source_settings WHERE id = 'iptv_org_public'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(curation_enabled, 1, "default de source_settings, sin override especial para esta fuente");
    }

    #[test]
    fn iptv_sources_rejects_unknown_source_kind() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let result = conn.execute(
            "INSERT INTO iptv_sources (id, name, source_kind) VALUES ('x', 'Test', 'ftp')",
            [],
        );
        assert!(result.is_err(), "el CHECK debe rechazar un source_kind fuera de ('url','file')");
    }

    #[test]
    fn youtube_sources_table_has_no_seed_rows_yet() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM youtube_sources", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "sin youtube_api_key real para verificar en vivo, la migración no siembra canales (Mandato 1) — ver plan fuente_youtube.txt"
        );
    }

    #[test]
    fn youtube_sources_rejects_unknown_category() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let result = conn.execute(
            "INSERT INTO youtube_sources (id, name, channel_url, category) \
             VALUES ('x', 'Test', 'https://youtube.com/@test', 'documentales')",
            [],
        );
        assert!(result.is_err(), "el CHECK debe rechazar una category fuera de ('cine','series','anime')");
    }

    #[test]
    fn youtube_sources_accepts_each_valid_category() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        for (id, category) in [("a", "cine"), ("b", "series"), ("c", "anime")] {
            conn.execute(
                "INSERT INTO youtube_sources (id, name, channel_url, category) VALUES (?1, 'Test', 'https://youtube.com/@test', ?2)",
                (id, category),
            )
            .unwrap();
        }
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM youtube_sources", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 3);
    }

    #[test]
    fn iptv_recordings_table_has_no_seed_rows_and_enforces_status_check() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM iptv_recordings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "las grabaciones las crea el usuario, no vienen precargadas");

        conn.execute(
            "INSERT INTO iptv_recordings (id, channel_name, manifest_url, file_name, status) \
             VALUES ('r1', 'Canal Test', 'https://example.org/live.m3u8', 'r1.ts', 'recording')",
            [],
        )
        .unwrap();
        let bytes_written: i64 = conn
            .query_row("SELECT bytes_written FROM iptv_recordings WHERE id = 'r1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(bytes_written, 0, "bytes_written debe tener default 0");

        let bad_status = conn.execute(
            "INSERT INTO iptv_recordings (id, channel_name, manifest_url, file_name, status) \
             VALUES ('r2', 'Canal Test', 'https://example.org/live.m3u8', 'r2.ts', 'paused')",
            [],
        );
        assert!(bad_status.is_err(), "el CHECK debe rechazar un status fuera de ('recording','stopped','error')");
    }

    #[test]
    fn app_settings_table_starts_empty() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM app_settings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "config clave-valor genérica, sin defaults precargados");

        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES ('local_library_folder', '/home/user/Videos')",
            [],
        )
        .unwrap();
        let value: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'local_library_folder'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "/home/user/Videos");
    }

    #[test]
    fn source_settings_seeds_public_domain_torrents_and_blender_foundation_with_expected_curation_defaults() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        let (curation_enabled, curation_hint): (i64, Option<String>) = conn
            .query_row(
                "SELECT curation_enabled, curation_hint FROM source_settings WHERE id = 'public_domain_torrents'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(curation_enabled, 1, "sin allowlist propia, la curación por IA sí aporta acá");
        assert_eq!(
            curation_hint.as_deref(),
            Some("películas reales de dominio público del catálogo de publicdomaintorrents.info, no basura ni entradas rotas del feed")
        );

        let (curation_enabled, curation_hint): (i64, Option<String>) = conn
            .query_row(
                "SELECT curation_enabled, curation_hint FROM source_settings WHERE id = 'blender_foundation'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(curation_enabled, 0, "allowlist ya vetted a mano, título por título — someterla a IA no suma nada");
        assert_eq!(
            curation_hint.as_deref(),
            Some("cortos y películas oficiales de Blender Foundation / Blender Studio")
        );
    }

    #[test]
    fn media_items_check_accepts_public_domain_torrents_source_type() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();

        conn.execute(
            "INSERT INTO media_items (id, source_type, source_identifier, title) \
             VALUES ('pdt1', 'public_domain_torrents', 'nosferatu', 'Nosferatu')",
            [],
        )
        .unwrap();

        let title: String = conn
            .query_row("SELECT title FROM media_items WHERE id = 'pdt1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(title, "Nosferatu");

        let bad = conn.execute(
            "INSERT INTO media_items (id, source_type, source_identifier, title) \
             VALUES ('bad1', 'not_a_real_source_type', 'x', 'x')",
            [],
        );
        assert!(bad.is_err(), "el CHECK reconstruido debe seguir rechazando source_type desconocidos");
    }
}
