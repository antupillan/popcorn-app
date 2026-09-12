// Diagnóstico manual de indexers BYO contra la DB real de un usuario —
// no es un test (sin asserts, depende de red y de datos locales reales
// de quien lo corre). Uso: cargo run --example diagnose_indexer -- <query>
use popcorn_app_lib::indexers::{search_one, Indexer, JsonPaths};

#[tokio::main]
async fn main() {
    let query = std::env::args().nth(1).unwrap_or_else(|| "one piece".to_string());
    let home = std::env::var("HOME").expect("HOME no seteado");
    let db_path = format!("{home}/.local/share/org.popcorn.app/popcorn.sqlite3");
    let conn = rusqlite::Connection::open(&db_path).expect("no se pudo abrir la DB real del usuario");

    let mut stmt = conn
        .prepare(
            "SELECT id, name, search_url_template, result_format, json_paths, enabled \
             FROM indexers WHERE enabled = 1",
        )
        .unwrap();
    let indexers: Vec<Indexer> = stmt
        .query_map([], |row| {
            let json_paths_raw: Option<String> = row.get(4)?;
            Ok(Indexer {
                id: row.get(0)?,
                name: row.get(1)?,
                search_url_template: row.get(2)?,
                result_format: row.get(3)?,
                json_paths: json_paths_raw.and_then(|s| serde_json::from_str::<JsonPaths>(&s).ok()),
                enabled: row.get::<_, i64>(5)? != 0,
            })
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    if indexers.is_empty() {
        println!("no hay indexers habilitados en la DB real");
        return;
    }

    let client = reqwest::Client::new();
    for indexer in &indexers {
        match search_one(&client, indexer, &query).await {
            Ok(results) => println!("[{}] OK: {} resultados", indexer.name, results.len()),
            Err(e) => println!("[{}] ERROR: {e:?}", indexer.name),
        }
    }
}
