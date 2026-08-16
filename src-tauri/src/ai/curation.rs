use super::{AiProvider, ScoredCandidate, StructuredQuery};
use crate::db::Db;

/// Filtra/reordena `items` según la relevancia que determine `provider`
/// contra `query`, usando `title_of` para extraer el texto comparable de
/// cada ítem — así sirve igual para `ArchiveOrgItem` que para
/// `IndexerResult`, sin lógica distinta por fuente (ver plan).
///
/// Fail-open: cualquier error de curación (sin red, respuesta mal formada,
/// índice inventado) devuelve `items` sin tocar en vez de romper la
/// búsqueda — a diferencia del candado de publicación, esto es un
/// enriquecimiento opcional, nunca un gate (ver plan, decisión "curación
/// por proveedor de búsqueda"). Sin caché (a diferencia de
/// `curate_by_hint` abajo) — esto cura resultados de búsqueda puntuales,
/// no un catálogo estable entre sesiones.
pub(crate) async fn curate<T>(
    provider: &dyn AiProvider,
    query: &StructuredQuery,
    items: Vec<T>,
    title_of: impl Fn(&T) -> &str,
) -> Vec<T> {
    if items.is_empty() {
        return items;
    }
    let candidates: Vec<String> = items.iter().map(|i| title_of(i).to_string()).collect();
    match provider.curate_results(query, &candidates).await {
        Ok(indices) => {
            let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
            indices
                .into_iter()
                .filter_map(|i| slots.get_mut(i).and_then(|slot| slot.take()))
                .collect()
        }
        Err(e) => {
            eprintln!(
                "[popcorn] curación IA falló ({}), devolviendo resultados sin curar: {e}",
                provider.name()
            );
            items
        }
    }
}

/// Ítems por tanda al curar el delta (ver `curate_by_hint`) — cada tanda es
/// su propia llamada al proveedor y su propio upsert inmediato, no una
/// llamada atómica sobre todo el delta. Valor de partida razonable, no un
/// límite técnico duro.
const CURATION_CHUNK_SIZE: usize = 20;

struct CachedEntry {
    included: bool,
    score: Option<i32>,
    hint_used: Option<String>,
}

fn read_cache(db: &Db, source_id: &str) -> Result<std::collections::HashMap<String, CachedEntry>, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT item_key, included, score, hint_used FROM curation_cache WHERE source_id = ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([source_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                CachedEntry {
                    included: r.get::<_, i64>(1)? != 0,
                    score: r.get::<_, Option<i64>>(2)?.map(|s| s as i32),
                    hint_used: r.get::<_, Option<String>>(3)?,
                },
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().collect())
}

/// Upsert de una tanda ya curada con éxito — se llama apenas resuelve cada
/// tanda, no al final de todo el delta (ver comentario en `curate_by_hint`
/// sobre persistencia parcial). No toca `consecutive_ping_failures`/
/// `last_ping_at` (columnas del ping, ver `availability_ping.rs`): al no
/// mencionarlas, `ON CONFLICT` las deja como estaban.
fn write_cache_chunk(
    db: &Db,
    source_id: &str,
    included: &[(String, i32)],
    excluded: &[String],
    hint: Option<&str>,
) -> Result<(), String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    for (item_key, score) in included {
        conn.execute(
            "INSERT INTO curation_cache (source_id, item_key, included, score, hint_used, cached_at)
             VALUES (?1, ?2, 1, ?3, ?4, datetime('now'))
             ON CONFLICT(source_id, item_key) DO UPDATE SET
                included = 1, score = ?3, hint_used = ?4, cached_at = datetime('now')",
            rusqlite::params![source_id, item_key, score, hint],
        )
        .map_err(|e| e.to_string())?;
    }
    for item_key in excluded {
        conn.execute(
            "INSERT INTO curation_cache (source_id, item_key, included, score, hint_used, cached_at)
             VALUES (?1, ?2, 0, NULL, ?3, datetime('now'))
             ON CONFLICT(source_id, item_key) DO UPDATE SET
                included = 0, score = NULL, hint_used = ?3, cached_at = datetime('now')",
            rusqlite::params![source_id, item_key, hint],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Cura `items` contra `hint`, cacheando el resultado en `curation_cache`
/// (SQLite) para no re-curar entre sesiones lo que no cambió. Sirve tanto
/// a canales IPTV como a catálogo Online (Biblioteca unificada) —
/// `source_id` es el mismo id que usa `source_settings`
/// (`curation_enabled`/`curation_hint`), `item_key_of` da la identidad
/// estable del ítem dentro de esa fuente.
///
/// A diferencia de `curate<T>`, el proveedor devuelve un puntaje absoluto
/// por candidato (`ScoredCandidate`, 0-100) en vez de solo un orden
/// relativo — así un ítem cacheado de una sesión vieja y uno recién
/// curado en esta misma pasada son comparables entre sí, y el orden final
/// sale de un sort único por puntaje en vez de necesitar fusionar dos
/// órdenes de llamadas distintas (ver contexto en el plan).
///
/// El delta (ítems sin fila cacheada, o con `hint_used` desactualizado) se
/// parte en tandas de `CURATION_CHUNK_SIZE`: cada tanda es su propia
/// llamada al proveedor y su propio upsert inmediato apenas resuelve. Si
/// una tanda falla a mitad del delta, las tandas ya resueltas quedan
/// persistidas igual — no se pierde el trabajo ya confirmado, ni hace
/// falta re-curar esa parte en el próximo intento (a pedido explícito del
/// usuario: "análisis parciales deben guardarse, aunque sean gotas de
/// información, pasito a pasito").
pub(crate) async fn curate_by_hint<T>(
    db: &Db,
    source_id: &str,
    provider: &dyn AiProvider,
    items: Vec<T>,
    title_of: impl Fn(&T) -> &str,
    item_key_of: impl Fn(&T) -> String,
    hint: Option<&str>,
) -> Result<Vec<T>, String> {
    if items.is_empty() {
        return Ok(items);
    }

    let cache = read_cache(db, source_id)?;

    let mut pool: Vec<(T, Option<i32>)> = Vec::new();
    let mut delta: Vec<T> = Vec::new();
    for item in items {
        let key = item_key_of(&item);
        match cache.get(&key) {
            Some(entry) if entry.hint_used.as_deref() == hint => {
                if entry.included {
                    pool.push((item, entry.score));
                }
                // excluido por una curación previa con el mismo hint: se
                // descarta, no vuelve al resultado.
            }
            _ => delta.push(item),
        }
    }

    let mut delta_iter = delta.into_iter();
    loop {
        let chunk: Vec<T> = delta_iter.by_ref().take(CURATION_CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        let candidates: Vec<String> = chunk.iter().map(|i| title_of(i).to_string()).collect();
        match provider.curate_by_hint(&candidates, hint).await {
            Ok(scored) => {
                let mut slots: Vec<Option<T>> = chunk.into_iter().map(Some).collect();
                let mut included_keys: Vec<(String, i32)> = Vec::new();
                for ScoredCandidate { index, score } in &scored {
                    if let Some(slot) = slots.get_mut(*index) {
                        if let Some(item) = slot.take() {
                            included_keys.push((item_key_of(&item), *score));
                            pool.push((item, Some(*score)));
                        }
                    }
                }
                let excluded_keys: Vec<String> =
                    slots.into_iter().flatten().map(|item| item_key_of(&item)).collect();
                write_cache_chunk(db, source_id, &included_keys, &excluded_keys, hint)?;
            }
            Err(e) => {
                eprintln!(
                    "[popcorn] curación por hint IA falló ({}), tanda sin cachear: {e}",
                    provider.name()
                );
                pool.extend(chunk.into_iter().map(|item| (item, None)));
            }
        }
    }

    // Descendente por score; `Option<i32>` ordena None antes que
    // cualquier Some, así que invertir la comparación deja los sin
    // puntaje (fail-open de esta pasada) al final, no al principio.
    pool.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(pool.into_iter().map(|(item, _)| item).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rusqlite::Connection;
    use std::sync::Mutex;

    fn migrated_db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&conn).unwrap();
        Db(Mutex::new(conn))
    }

    struct OkProvider(Vec<usize>);
    #[async_trait]
    impl AiProvider for OkProvider {
        fn name(&self) -> &'static str {
            "fake-ok"
        }
        async fn parse_query(&self, _text: &str) -> anyhow::Result<StructuredQuery> {
            unreachable!("no lo usa este test")
        }
        async fn curate_results(
            &self,
            _query: &StructuredQuery,
            _candidates: &[String],
        ) -> anyhow::Result<Vec<usize>> {
            Ok(self.0.clone())
        }
        async fn curate_by_hint(
            &self,
            _candidates: &[String],
            _hint: Option<&str>,
        ) -> anyhow::Result<Vec<ScoredCandidate>> {
            unreachable!("no lo usa curate<T>, ver ScoredHintProvider para curate_by_hint")
        }
        async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
        async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
    }

    struct ErrProvider;
    #[async_trait]
    impl AiProvider for ErrProvider {
        fn name(&self) -> &'static str {
            "fake-err"
        }
        async fn parse_query(&self, _text: &str) -> anyhow::Result<StructuredQuery> {
            unreachable!("no lo usa este test")
        }
        async fn curate_results(
            &self,
            _query: &StructuredQuery,
            _candidates: &[String],
        ) -> anyhow::Result<Vec<usize>> {
            Err(anyhow::anyhow!("boom"))
        }
        async fn curate_by_hint(
            &self,
            _candidates: &[String],
            _hint: Option<&str>,
        ) -> anyhow::Result<Vec<ScoredCandidate>> {
            Err(anyhow::anyhow!("boom"))
        }
        async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
        async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
    }

    struct PanicIfCalledProvider;
    #[async_trait]
    impl AiProvider for PanicIfCalledProvider {
        fn name(&self) -> &'static str {
            "fake-panic"
        }
        async fn parse_query(&self, _text: &str) -> anyhow::Result<StructuredQuery> {
            unreachable!("no lo usa este test")
        }
        async fn curate_results(
            &self,
            _query: &StructuredQuery,
            _candidates: &[String],
        ) -> anyhow::Result<Vec<usize>> {
            panic!("curate no debe llamar al proveedor con una lista de candidatos vacía");
        }
        async fn curate_by_hint(
            &self,
            _candidates: &[String],
            _hint: Option<&str>,
        ) -> anyhow::Result<Vec<ScoredCandidate>> {
            panic!("curate_by_hint no debe llamar al proveedor cuando todo está cacheado");
        }
        async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
        async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
    }

    #[tokio::test]
    async fn curate_filters_and_reorders_by_returned_indices() {
        let provider = OkProvider(vec![2, 0]);
        let q = StructuredQuery::default();
        let items = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let result = curate(&provider, &q, items, |s| s.as_str()).await;
        assert_eq!(result, vec!["c".to_string(), "a".to_string()]);
    }

    #[tokio::test]
    async fn curate_ignores_out_of_range_indices() {
        let provider = OkProvider(vec![5, 1]);
        let q = StructuredQuery::default();
        let items = vec!["a".to_string(), "b".to_string()];
        let result = curate(&provider, &q, items, |s| s.as_str()).await;
        assert_eq!(result, vec!["b".to_string()]);
    }

    #[tokio::test]
    async fn curate_fails_open_on_provider_error() {
        let provider = ErrProvider;
        let q = StructuredQuery::default();
        let items = vec!["a".to_string(), "b".to_string()];
        let result = curate(&provider, &q, items.clone(), |s| s.as_str()).await;
        assert_eq!(result, items);
    }

    #[tokio::test]
    async fn curate_skips_provider_call_for_empty_input() {
        let provider = PanicIfCalledProvider;
        let q = StructuredQuery::default();
        let items: Vec<String> = vec![];
        let result = curate(&provider, &q, items, |s| s.as_str()).await;
        assert!(result.is_empty());
    }

    // --- curate_by_hint (caché + tandas) ---

    fn item(key: &str) -> (String, String) {
        // (item_key, title) — item_key_of/title_of separados a propósito en
        // los tests, para que un cambio de título no cambie la identidad
        // cacheada (mismo criterio que Online: item_key es kind:identifier,
        // no el título).
        (key.to_string(), format!("título de {key}"))
    }

    /// Devuelve un `ScoredCandidate` por cada candidato con el score fijo
    /// dado (todos incluidos) — para tests donde no importa el valor
    /// puntual, solo que se cachee algo real.
    struct ScoredHintProvider(i32);
    #[async_trait]
    impl AiProvider for ScoredHintProvider {
        fn name(&self) -> &'static str {
            "fake-scored"
        }
        async fn parse_query(&self, _text: &str) -> anyhow::Result<StructuredQuery> {
            unreachable!("no lo usa este test")
        }
        async fn curate_results(
            &self,
            _query: &StructuredQuery,
            _candidates: &[String],
        ) -> anyhow::Result<Vec<usize>> {
            unreachable!("no lo usa este test")
        }
        async fn curate_by_hint(
            &self,
            candidates: &[String],
            _hint: Option<&str>,
        ) -> anyhow::Result<Vec<ScoredCandidate>> {
            Ok((0..candidates.len()).map(|i| ScoredCandidate { index: i, score: self.0 }).collect())
        }
        async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
        async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
            unreachable!("no lo usa este test")
        }
    }

    #[tokio::test]
    async fn curate_by_hint_skips_provider_when_everything_is_cached() {
        let db = migrated_db();
        let (key, title) = item("a");
        write_cache_chunk(&db, "src", &[(key.clone(), 80)], &[], Some("crit")).unwrap();

        let provider = PanicIfCalledProvider;
        let items = vec![(key, title)];
        let result = curate_by_hint(
            &db,
            "src",
            &provider,
            items,
            |(_, t)| t.as_str(),
            |(k, _)| k.clone(),
            Some("crit"),
        )
        .await
        .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn curate_by_hint_only_sends_uncached_items_and_sorts_by_score() {
        let db = migrated_db();
        let cached = item("cached");
        write_cache_chunk(&db, "src", &[(cached.0.clone(), 50)], &[], Some("crit")).unwrap();

        let fresh_a = item("fresh_a");
        let fresh_b = item("fresh_b");
        let provider = ScoredHintProvider(90); // todos los frescos puntúan 90, por encima del cacheado (50)
        let items = vec![cached.clone(), fresh_a.clone(), fresh_b.clone()];
        let result = curate_by_hint(
            &db,
            "src",
            &provider,
            items,
            |(_, t)| t.as_str(),
            |(k, _)| k.clone(),
            Some("crit"),
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 3, "no debe perder ítems al mezclar cacheados y frescos");
        assert_ne!(result[0].0, cached.0, "los frescos (score 90) deben ir antes que el cacheado (score 50)");

        // Confirma que quedaron cacheados con su score real, no solo que la
        // función devolvió algo razonable.
        let cache = read_cache(&db, "src").unwrap();
        assert_eq!(cache.get(&fresh_a.0).unwrap().score, Some(90));
    }

    #[tokio::test]
    async fn curate_by_hint_stale_hint_forces_recuration() {
        let db = migrated_db();
        let (key, title) = item("a");
        write_cache_chunk(&db, "src", &[(key.clone(), 50)], &[], Some("hint viejo")).unwrap();

        let provider = ScoredHintProvider(70);
        let items = vec![(key.clone(), title)];
        let result = curate_by_hint(
            &db,
            "src",
            &provider,
            items,
            |(_, t)| t.as_str(),
            |(k, _)| k.clone(),
            Some("hint nuevo"),
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 1);
        let cache = read_cache(&db, "src").unwrap();
        assert_eq!(cache.get(&key).unwrap().score, Some(70), "debe re-curarse con el hint nuevo, no quedar con el score viejo");
        assert_eq!(cache.get(&key).unwrap().hint_used.as_deref(), Some("hint nuevo"));
    }

    #[tokio::test]
    async fn curate_by_hint_fails_open_on_provider_error_without_polluting_cache() {
        let db = migrated_db();
        let provider = ErrProvider;
        let (key, title) = item("a");
        let items = vec![(key.clone(), title)];
        let result = curate_by_hint(
            &db,
            "src",
            &provider,
            items,
            |(_, t)| t.as_str(),
            |(k, _)| k.clone(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 1, "fail-open: el ítem se devuelve igual, sin filtrar");
        let cache = read_cache(&db, "src").unwrap();
        assert!(cache.get(&key).is_none(), "un fallo del proveedor no debe dejar una fila espuria en el caché");
    }

    #[tokio::test]
    async fn curate_by_hint_skips_provider_call_for_empty_input() {
        let db = migrated_db();
        let provider = PanicIfCalledProvider;
        let items: Vec<(String, String)> = vec![];
        let result = curate_by_hint(&db, "src", &provider, items, |(_, t)| t.as_str(), |(k, _)| k.clone(), None)
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    /// Segunda tanda falla, la primera ya quedó persistida — verificado
    /// leyendo la tabla directo, no solo el valor de retorno (a pedido
    /// explícito del usuario: los análisis parciales deben guardarse).
    #[tokio::test]
    async fn curate_by_hint_persists_earlier_chunks_when_a_later_chunk_fails() {
        struct FlakyAfterFirstChunk {
            calls: Mutex<usize>,
        }
        #[async_trait]
        impl AiProvider for FlakyAfterFirstChunk {
            fn name(&self) -> &'static str {
                "fake-flaky"
            }
            async fn parse_query(&self, _text: &str) -> anyhow::Result<StructuredQuery> {
                unreachable!("no lo usa este test")
            }
            async fn curate_results(
                &self,
                _query: &StructuredQuery,
                _candidates: &[String],
            ) -> anyhow::Result<Vec<usize>> {
                unreachable!("no lo usa este test")
            }
            async fn curate_by_hint(
                &self,
                candidates: &[String],
                _hint: Option<&str>,
            ) -> anyhow::Result<Vec<ScoredCandidate>> {
                let mut calls = self.calls.lock().unwrap();
                *calls += 1;
                if *calls == 1 {
                    Ok((0..candidates.len()).map(|i| ScoredCandidate { index: i, score: 60 }).collect())
                } else {
                    Err(anyhow::anyhow!("proveedor caído a mitad del delta"))
                }
            }
            async fn translate(&self, _texts: &[String], _target_lang: &str) -> anyhow::Result<Vec<String>> {
                unreachable!("no lo usa este test")
            }
            async fn broaden_query(&self, _query: &str) -> anyhow::Result<Vec<String>> {
                unreachable!("no lo usa este test")
            }
        }

        // CURATION_CHUNK_SIZE es 20 — 25 ítems nuevos arman exactamente 2
        // tandas (20 + 5), la segunda falla.
        let db = migrated_db();
        let provider = FlakyAfterFirstChunk { calls: Mutex::new(0) };
        let items: Vec<(String, String)> = (0..25).map(|i| item(&format!("item{i}"))).collect();
        let expected_first_chunk_keys: Vec<String> = items[..20].iter().map(|(k, _)| k.clone()).collect();

        let result = curate_by_hint(
            &db,
            "src",
            &provider,
            items,
            |(_, t)| t.as_str(),
            |(k, _)| k.clone(),
            None,
        )
        .await
        .unwrap();

        assert_eq!(result.len(), 25, "fail-open de la segunda tanda: no se pierden ítems");
        let cache = read_cache(&db, "src").unwrap();
        for key in &expected_first_chunk_keys {
            assert_eq!(
                cache.get(key).map(|e| e.score),
                Some(Some(60)),
                "la primera tanda debe haber quedado persistida pese a que la segunda falló"
            );
        }
        assert_eq!(cache.len(), 20, "solo la primera tanda (20 ítems) debe estar cacheada, la segunda no");
    }
}
