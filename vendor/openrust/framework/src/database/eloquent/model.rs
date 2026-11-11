use std::future::Future;

use crate::database::{Connection, DB, DatabaseResult, Value};

use super::{
    Attributes, Builder, Collection, ModelMetadata, ModelState, Scope, concerns, persistence,
    soft_deletes,
};

/// A model type implemented by the application.
#[allow(non_snake_case, reason = "Matches Laravel's Eloquent Model API")]
pub trait Model: Sized + Send + Sync + 'static {
    fn metadata() -> &'static ModelMetadata;
    fn from_state(state: ModelState) -> Self;
    fn state(&self) -> &ModelState;
    fn state_mut(&mut self) -> &mut ModelState;

    fn globalScopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[]
    }

    fn query() -> DatabaseResult<Builder<'static, Self>> {
        Ok(Self::queryOn(&DB::connection(Self::metadata().connection)?))
    }

    fn queryOn(connection: &Connection) -> Builder<'static, Self> {
        Builder::new(connection.clone())
    }

    fn all() -> impl Future<Output = DatabaseResult<Collection<Self>>> + Send {
        async { Self::query()?.get().await }
    }

    fn find(key: impl Into<Value>) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send {
        let key = key.into();
        async move { Self::query()?.find(key).await }
    }

    fn findOrFail(key: impl Into<Value>) -> impl Future<Output = DatabaseResult<Self>> + Send {
        let key = key.into();
        async move { Self::query()?.findOrFail(key).await }
    }

    fn create(attributes: Attributes) -> impl Future<Output = DatabaseResult<Self>> + Send {
        async { Self::query()?.create(attributes).await }
    }

    fn newInstance(attributes: Attributes) -> DatabaseResult<Self> {
        let mut model = Self::from_state(ModelState::default());
        model.fill(attributes)?;
        Ok(model)
    }

    fn newInstanceOn(connection: &Connection, attributes: Attributes) -> DatabaseResult<Self> {
        let mut model = Self::newInstance(Attributes::new())?;
        model.state_mut().connection = Some(connection.clone());
        model.fill(attributes)?;
        Ok(model)
    }

    fn fill(&mut self, attributes: Attributes) -> DatabaseResult<&mut Self> {
        let attributes = concerns::guards_attributes::fill(attributes, Self::metadata())?;
        self.state_mut().apply_attributes(attributes);
        Ok(self)
    }

    fn forceFill(&mut self, attributes: Attributes) -> &mut Self {
        self.state_mut().apply_attributes(attributes);
        self
    }

    fn getAttribute(&self, column: &str) -> Option<&Value> {
        self.state().getAttribute(column)
    }

    /// Reads through the model accessor or cast.
    fn setAttribute(&mut self, column: &str, value: impl Into<Value>) -> &mut Self {
        self.state_mut().setAttribute(column, value);
        self
    }

    fn getKey(&self) -> Option<&Value> {
        self.getAttribute(Self::metadata().primary_key)
    }

    fn getKeyName(&self) -> &'static str {
        Self::metadata().primary_key
    }

    fn getTable(&self) -> &'static str {
        Self::metadata().table
    }

    fn getConnectionName(&self) -> Option<&str> {
        self.state()
            .connection
            .as_ref()
            .map(Connection::getName)
            .or(Self::metadata().connection)
    }

    fn save(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::save(self)
    }

    fn update(
        &mut self,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::update(self, attributes)
    }

    fn delete(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::delete(self, false)
    }

    fn forceDelete(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::delete(self, true)
    }

    fn restore(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::restore(self)
    }

    fn trashed(&self) -> bool {
        Self::metadata()
            .deleted_at
            .is_some_and(|column| soft_deletes::is_trashed(self.getAttribute(column)))
    }
}
