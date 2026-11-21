use crate::{
    EngineManager, Error, Result, Searchable,
    engines::{Engine, SearchResults},
};
use openrust::{
    database::{Value, eloquent::Builder as EloquentBuilder},
    pagination::{LengthAwarePaginator, Paginator, PaginatorOptions},
};
use std::sync::Arc;
pub type QueryCallback<M> =
    dyn Fn(EloquentBuilder<'static, M>) -> Result<EloquentBuilder<'static, M>> + Send + Sync;
pub type RawCallback = dyn Fn(SearchResults) -> Result<SearchResults> + Send + Sync;
#[derive(Clone)]
pub struct Where {
    pub field: String,
    pub operator: String,
    pub value: Value,
}
pub struct Builder<M: Searchable> {
    pub model: M,
    pub query: String,
    pub index: Option<String>,
    pub wheres: Vec<Where>,
    pub whereIns: Vec<(String, Vec<Value>)>,
    pub whereNotIns: Vec<(String, Vec<Value>)>,
    pub orders: Vec<(String, String)>,
    pub limit: Option<usize>,
    pub options: serde_json::Value,
    pub semanticSearch: bool,
    pub minimumSimilarity: Option<f64>,
    pub hybridSearch: Option<(f64, f64)>,
    pub callback: Option<Arc<QueryCallback<M>>>,
    pub queryCallback: Option<Arc<QueryCallback<M>>>,
    pub afterRawSearchCallback: Option<Arc<RawCallback>>,
    pub(crate) manager: Arc<EngineManager>,
}
impl<M: Searchable> Clone for Builder<M> {
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
            query: self.query.clone(),
            index: self.index.clone(),
            wheres: self.wheres.clone(),
            whereIns: self.whereIns.clone(),
            whereNotIns: self.whereNotIns.clone(),
            orders: self.orders.clone(),
            limit: self.limit,
            options: self.options.clone(),
            semanticSearch: self.semanticSearch,
            minimumSimilarity: self.minimumSimilarity,
            hybridSearch: self.hybridSearch,
            callback: self.callback.clone(),
            queryCallback: self.queryCallback.clone(),
            afterRawSearchCallback: self.afterRawSearchCallback.clone(),
            manager: self.manager.clone(),
        }
    }
}
impl<M: Searchable> Builder<M> {
    pub fn new(model: M, query: impl Into<String>, manager: Arc<EngineManager>) -> Self {
        let mut wheres = Vec::new();
        if manager.boolean("soft_delete") && model.getDeletedAtColumn().is_some() {
            wheres.push(Where {
                field: "__soft_deleted".into(),
                operator: "=".into(),
                value: 0_i64.into(),
            });
        }
        Self {
            model,
            query: query.into(),
            manager,
            index: None,
            wheres,
            whereIns: Vec::new(),
            whereNotIns: Vec::new(),
            orders: Vec::new(),
            limit: None,
            options: serde_json::json!({}),
            semanticSearch: false,
            minimumSimilarity: None,
            hybridSearch: None,
            callback: None,
            queryCallback: None,
            afterRawSearchCallback: None,
        }
    }
    pub fn within(mut self, index: impl Into<String>) -> Self {
        self.index = Some(index.into());
        self
    }
    pub fn r#where(
        mut self,
        field: impl Into<String>,
        operator: impl Into<String>,
        value: impl Into<Value>,
    ) -> Self {
        self.wheres.push(Where {
            field: field.into(),
            operator: operator.into(),
            value: value.into(),
        });
        self
    }
    pub fn whereIn(mut self, field: impl Into<String>, values: Vec<Value>) -> Self {
        let field = field.into();
        self.whereIns.retain(|(key, _)| *key != field);
        self.whereIns.push((field, values));
        self
    }
    pub fn whereNotIn(mut self, field: impl Into<String>, values: Vec<Value>) -> Self {
        let field = field.into();
        self.whereNotIns.retain(|(key, _)| *key != field);
        self.whereNotIns.push((field, values));
        self
    }
    pub fn withTrashed(mut self) -> Self {
        self.wheres.retain(|w| w.field != "__soft_deleted");
        self
    }
    pub fn onlyTrashed(self) -> Self {
        self.withTrashed().r#where("__soft_deleted", "=", 1_i64)
    }
    pub fn take(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
    pub fn orderBy(mut self, column: impl Into<String>, direction: &str) -> Self {
        self.orders.push((
            column.into(),
            if direction.eq_ignore_ascii_case("asc") {
                "asc"
            } else {
                "desc"
            }
            .into(),
        ));
        self
    }
    pub fn orderByDesc(self, column: impl Into<String>) -> Self {
        self.orderBy(column, "desc")
    }
    pub fn latest(self, column: Option<&str>) -> Self {
        let column = column
            .unwrap_or_else(|| self.model.getCreatedAtColumn().unwrap_or("created_at"))
            .to_owned();
        self.orderBy(column, "desc")
    }
    pub fn oldest(self, column: Option<&str>) -> Self {
        let column = column
            .unwrap_or_else(|| self.model.getCreatedAtColumn().unwrap_or("created_at"))
            .to_owned();
        self.orderBy(column, "asc")
    }
    pub fn options(mut self, options: serde_json::Value) -> Self {
        self.options = options;
        self
    }
    pub fn query(
        mut self,
        callback: impl Fn(EloquentBuilder<'static, M>) -> Result<EloquentBuilder<'static, M>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.queryCallback = Some(Arc::new(callback));
        self
    }
    pub fn callback(
        mut self,
        callback: impl Fn(EloquentBuilder<'static, M>) -> Result<EloquentBuilder<'static, M>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.callback = Some(Arc::new(callback));
        self
    }
    pub fn withRawResults(
        mut self,
        callback: impl Fn(SearchResults) -> Result<SearchResults> + Send + Sync + 'static,
    ) -> Self {
        self.afterRawSearchCallback = Some(Arc::new(callback));
        self
    }
    pub fn semantic(mut self, minSimilarity: Option<f64>) -> Result<Self> {
        if self.query.trim().is_empty() {
            return Err(Error::InvalidConfiguration(
                "Semantic searches require a non-empty query.".into(),
            ));
        }
        self.semanticSearch = true;
        self.hybridSearch = None;
        self.minimumSimilarity = minSimilarity;
        Ok(self)
    }
    pub fn hybrid(
        mut self,
        textWeight: f64,
        semanticWeight: f64,
        minSimilarity: Option<f64>,
    ) -> Result<Self> {
        if self.query.trim().is_empty()
            || !textWeight.is_finite()
            || !semanticWeight.is_finite()
            || textWeight <= 0.0
            || semanticWeight <= 0.0
        {
            return Err(Error::InvalidConfiguration(
                "Hybrid searches require a non-empty query and positive finite weights.".into(),
            ));
        }
        self.semanticSearch = false;
        self.hybridSearch = Some((textWeight, semanticWeight));
        self.minimumSimilarity = minSimilarity;
        Ok(self)
    }
    pub fn engine(&self) -> Result<Arc<dyn Engine<M>>> {
        let engine = self
            .manager
            .engine::<M>(self.model.scoutDriver().as_deref())?;
        if (self.semanticSearch || self.hybridSearch.is_some()) && !engine.supportsSemanticSearch()
        {
            return Err(Error::NotSupported(
                "The configured Scout engine does not support semantic search.".into(),
            ));
        }
        Ok(engine)
    }
    pub async fn raw(&self) -> Result<SearchResults> {
        self.engine()?.search(self).await
    }
    pub async fn keys(&self) -> Result<Vec<Value>> {
        self.engine()?.keys(self).await
    }
    pub fn applyAfterRawSearchCallback(&self, results: SearchResults) -> Result<SearchResults> {
        match &self.afterRawSearchCallback {
            Some(callback) => callback(results),
            None => Ok(results),
        }
    }
    pub async fn get(&self) -> Result<openrust::database::eloquent::Collection<M>> {
        self.engine()?.get(self).await
    }
    pub async fn first(&self) -> Result<Option<M>> {
        Ok(self.get().await?.into_iter().next())
    }
    pub async fn paginate(&self, perPage: u64, page: u64) -> Result<LengthAwarePaginator<M>> {
        if perPage == 0 {
            return Err(Error::InvalidConfiguration(
                "Scout page size must be positive".into(),
            ));
        }
        let engine = self.engine()?;
        // Validate native sizes/offsets before executing an engine query.
        LengthAwarePaginator::<M>::new(
            vec![],
            0,
            perPage,
            Some(page),
            PaginatorOptions::default(),
        )?;
        let results =
            self.applyAfterRawSearchCallback(engine.paginate(self, perPage, page).await?)?;
        let models = engine.map(self, &results).await?;
        let total = if self.queryCallback.is_some() {
            let mut all = self.clone();
            all.limit = None;
            let raw = engine.search(&all).await?;
            engine.map(self, &raw).await?.len() as u64
        } else {
            engine.getTotalCount(&results)
        };
        let mut paginator = LengthAwarePaginator::new(
            models,
            total,
            perPage,
            Some(page),
            PaginatorOptions::default(),
        )?;
        paginator.appends(&[("query".into(), self.query.clone())]);
        Ok(paginator)
    }
    pub async fn simplePaginate(&self, perPage: u64, page: u64) -> Result<Paginator<M>> {
        if perPage == 0 {
            return Err(Error::InvalidConfiguration(
                "Scout page size must be positive".into(),
            ));
        }
        let engine = self.engine()?;
        Paginator::<M>::new(vec![], perPage, Some(page), PaginatorOptions::default())?;
        let results =
            self.applyAfterRawSearchCallback(engine.paginate(self, perPage, page).await?)?;
        let mut paginator = Paginator::new(
            engine.map(self, &results).await?,
            perPage,
            Some(page),
            PaginatorOptions::default(),
        )?;
        paginator.hasMorePagesWhen(perPage.saturating_mul(page) < engine.getTotalCount(&results));
        paginator.appends(&[("query".into(), self.query.clone())]);
        Ok(paginator)
    }
    pub fn cursor(&self) -> futures_util::stream::BoxStream<'static, Result<M>> {
        use futures_util::StreamExt as _;
        let builder = self.clone();
        futures_util::stream::once(async move {
            let engine = builder.engine()?;
            engine.cursor(&builder).await
        })
        .map(|result| match result {
            Ok(stream) => stream,
            Err(error) => futures_util::stream::once(async { Err(error) }).boxed(),
        })
        .flatten()
        .boxed()
    }
    pub fn modelConnectionType(&self) -> Result<String> {
        Ok(self.model.getConnection()?.getDriverName().into())
    }
    pub(crate) fn collectionQuery(&self) -> Result<EloquentBuilder<'static, M>> {
        let mut query = M::queryOn(&self.model.getConnection()?);
        if let Some(callback) = &self.callback {
            query = callback(query)?;
        } else {
            for clause in &self.wheres {
                if clause.field != "__soft_deleted" {
                    query.r#where(&clause.field, &clause.operator, clause.value.clone());
                }
            }
            for (field, values) in &self.whereIns {
                query.whereIn(field, values.clone());
            }
            for (field, values) in &self.whereNotIns {
                query.whereNested(|conditions| conditions.whereNotIn(field, values.clone()));
            }
        }
        for (column, direction) in &self.orders {
            query = query.orderBy(column, direction);
        }
        if self.orders.is_empty() {
            query = query.orderBy(&self.model.getScoutKeyName(), "desc");
        }
        let soft = self.wheres.iter().find(|w| w.field == "__soft_deleted");
        if self.model.getDeletedAtColumn().is_some() {
            query = match soft.map(|w| &w.value) {
                Some(Value::Integer(1)) => query.onlyTrashed(),
                Some(Value::Integer(0)) => query.withoutTrashed(),
                _ if self.manager.boolean("soft_delete") => query.withTrashed(),
                _ => query,
            };
        }
        Ok(query)
    }
}
