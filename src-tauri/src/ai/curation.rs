use super::{AiProvider, StructuredQuery};

/// Filtra/reordena `items` según la relevancia que determine `provider`
/// contra `query`, usando `title_of` para extraer el texto comparable de
/// cada ítem — así sirve igual para `ArchiveOrgItem` que para
/// `IndexerResult`, sin lógica distinta por fuente (ver plan).
///
/// Fail-open: cualquier error de curación (sin red, respuesta mal formada,
/// índice inventado) devuelve `items` sin tocar en vez de romper la
/// búsqueda — a diferencia del candado de publicación, esto es un
/// enriquecimiento opcional, nunca un gate (ver plan, decisión "curación
/// por proveedor de búsqueda").
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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

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
}
