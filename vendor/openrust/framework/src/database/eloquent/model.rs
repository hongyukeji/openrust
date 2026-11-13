use std::{borrow::Cow, future::Future};

use crate::{
    database::{Connection, DB, DatabaseError, DatabaseResult, Value},
    http::{HttpError, HttpFuture},
    routing::BindingContext,
};

use super::{
    Attributes, Builder, Collection, EagerLoads, ModelMetadata, ModelState, Scope, Touches,
    VisibilityCondition,
    casts::{self, Attribute, AttributeCast, CastableEnum, CastableObject},
    concerns, persistence,
    relations::Relation,
    route_binding, serialization, soft_deletes,
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

    /// Declares the owners whose timestamps a dirty save should update.
    fn touches() -> Touches<Self> {
        Touches::default()
    }

    /// Maps a lifecycle event to a typed application event using the current model state.
    fn dispatchesEvents(
        &self,
        _event: concerns::ModelEvent,
    ) -> Option<std::sync::Arc<dyn crate::contracts::events::Event>> {
        None
    }

    fn attribute(_name: &str) -> Option<Attribute> {
        None
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[]
    }

    fn getHidden(&self) -> Vec<&str> {
        serialization::names(
            self.state().serialization().hidden.as_deref(),
            Self::metadata().hidden,
        )
        .collect()
    }

    fn setHidden(&mut self, attributes: &[&str]) -> &mut Self {
        self.state_mut().serialization_mut().hidden =
            Some(attributes.iter().map(|name| (*name).into()).collect());
        self
    }

    fn mergeHidden(&mut self, attributes: &[&str]) -> &mut Self {
        if !attributes.is_empty() {
            serialization::merge(
                serialization::local_names(
                    &mut self.state_mut().serialization_mut().hidden,
                    Self::metadata().hidden,
                ),
                attributes,
            );
        }
        self
    }

    fn getVisible(&self) -> Vec<&str> {
        serialization::names(
            self.state().serialization().visible.as_deref(),
            Self::metadata().visible,
        )
        .collect()
    }

    fn setVisible(&mut self, attributes: &[&str]) -> &mut Self {
        self.state_mut().serialization_mut().visible =
            Some(attributes.iter().map(|name| (*name).into()).collect());
        self
    }

    fn mergeVisible(&mut self, attributes: &[&str]) -> &mut Self {
        if !attributes.is_empty() {
            serialization::merge(
                serialization::local_names(
                    &mut self.state_mut().serialization_mut().visible,
                    Self::metadata().visible,
                ),
                attributes,
            );
        }
        self
    }

    fn getAppends(&self) -> Vec<&str> {
        serialization::names(
            self.state().serialization().appends.as_deref(),
            Self::metadata().appends,
        )
        .collect()
    }

    fn setAppends(&mut self, attributes: &[&str]) -> &mut Self {
        self.state_mut().serialization_mut().appends =
            Some(attributes.iter().map(|name| (*name).into()).collect());
        self
    }

    fn append(&mut self, attributes: &[&str]) -> &mut Self {
        if !attributes.is_empty() {
            serialization::merge(
                serialization::local_names(
                    &mut self.state_mut().serialization_mut().appends,
                    Self::metadata().appends,
                ),
                attributes,
            );
        }
        self
    }

    fn makeVisible(&mut self, attributes: &[&str]) -> &mut Self {
        let metadata = Self::metadata();
        let options = self.state_mut().serialization_mut();
        serialization::local_names(&mut options.hidden, metadata.hidden)
            .retain(|name| !attributes.contains(&name.as_str()));
        let visible = serialization::local_names(&mut options.visible, metadata.visible);
        if !visible.is_empty() {
            serialization::merge(visible, attributes);
        }
        self
    }

    fn makeHidden(&mut self, attributes: &[&str]) -> &mut Self {
        self.mergeHidden(attributes)
    }

    fn makeVisibleIf(
        &mut self,
        condition: impl VisibilityCondition<Self>,
        attributes: &[&str],
    ) -> &mut Self {
        if condition.evaluate(self) {
            self.makeVisible(attributes);
        }
        self
    }

    fn makeHiddenIf(
        &mut self,
        condition: impl VisibilityCondition<Self>,
        attributes: &[&str],
    ) -> &mut Self {
        if condition.evaluate(self) {
            self.makeHidden(attributes);
        }
        self
    }

    fn hasAppended(&self, name: &str) -> bool {
        serialization::names(
            self.state().serialization().appends.as_deref(),
            Self::metadata().appends,
        )
        .any(|attribute| attribute == name)
    }

    fn withoutAppends(&mut self) -> &mut Self {
        self.setAppends(&[])
    }

    /// Formats a date for JSON output without changing its database representation.
    fn serializeDate(&self, date: chrono::DateTime<chrono::Utc>) -> DatabaseResult<String> {
        Ok(date.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
    }

    fn attributesToArray(&self) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        serialization::attributes(self)
    }

    fn relationsToArray(&self) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        let state = self.state();
        state
            .relations
            .project(|name| state.serialization().includes(Self::metadata(), name))
    }

    fn toArray(&self) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        serialization::model(self)
    }

    fn toJson(&self) -> DatabaseResult<String> {
        serde_json::to_string(&self.toArray()?)
            .map_err(|_| DatabaseError::InvalidQuery("model JSON"))
    }

    /// Declares the inverse relationship names available on this custom pivot model.
    fn pivotInverseRelations() -> &'static [&'static str] {
        &[]
    }

    /// Defines the relationship descriptors available to named eager loading.
    fn namedRelations() -> super::NamedRelations<Self> {
        super::NamedRelations::default()
    }

    /// Declares relationships to load whenever this model is retrieved.
    fn withDefaultRelations(relations: EagerLoads<Self>) -> EagerLoads<Self> {
        relations
    }

    /// Adds the relationship counts selected by default when reading this model.
    fn withDefaultCounts(query: Builder<'static, Self>) -> DatabaseResult<Builder<'static, Self>> {
        Ok(query)
    }

    /// Enables first-access relationship batching for collections of this model.
    fn automaticallyEagerLoadRelationships() -> bool {
        false
    }

    /// Prevents implicit single-model relationship queries during access.
    fn preventsLazyLoading(&self) -> bool {
        false
    }

    /// Handles a prohibited lazy access. Returning success permits the query.
    fn handleLazyLoadingViolationUsing(&self, violation: DatabaseError) -> DatabaseResult<()> {
        if !self.state().exists() || self.state().wasRecentlyCreated() {
            return Ok(());
        }
        Err(violation)
    }

    fn query() -> DatabaseResult<Builder<'static, Self>> {
        Ok(Self::queryOn(&DB::connection(Self::metadata().connection)?))
    }

    fn queryOn(connection: &Connection) -> Builder<'static, Self> {
        Builder::new(connection.clone())
            .withOnly(Self::withDefaultRelations)
            .enable_default_counts()
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

    fn uniqueIds() -> &'static [&'static str] {
        let metadata = Self::metadata();
        metadata.unique_ids.unwrap_or_else(|| {
            if metadata.unique_id.is_some() {
                std::slice::from_ref(&metadata.primary_key)
            } else {
                &[]
            }
        })
    }

    fn usesUniqueIds() -> bool {
        !Self::uniqueIds().is_empty()
    }

    fn newUniqueId(&self) -> DatabaseResult<Option<String>> {
        Self::metadata()
            .unique_id
            .map(super::UniqueId::generate)
            .transpose()
    }

    fn isValidUniqueId(value: &str) -> bool {
        Self::metadata()
            .unique_id
            .is_none_or(|kind| kind.valid(value))
    }

    fn setUniqueIds(&mut self) -> impl Future<Output = DatabaseResult<()>> + Send {
        async {
            for column in Self::uniqueIds() {
                if super::unique_ids::is_empty(self.getAttributeValue(column)?) {
                    let id = self.newUniqueId()?.map_or(Value::Null, Value::Text);
                    self.setAttributeValueAsync(column, id).await?;
                }
            }
            Ok(())
        }
    }

    fn getIncrementing() -> bool {
        let metadata = Self::metadata();
        metadata.incrementing && !Self::uniqueIds().contains(&metadata.primary_key)
    }

    /// Creates an unsaved copy, excluding identity, timestamps, and the given attributes.
    fn replicate<'a>(
        &'a self,
        except: &'a [&str],
    ) -> impl Future<Output = DatabaseResult<Self>> + Send + 'a {
        async move {
            let metadata = Self::metadata();
            let source = self.state();
            let attributes = source
                .attributes
                .iter()
                .filter(|(column, _)| {
                    let column = column.as_str();
                    source
                        .pivot_identity
                        .is_some_and(|identity| identity.columns().any(|key| key == column))
                        || (column != metadata.primary_key
                            && Some(column) != metadata.created_at
                            && Some(column) != metadata.updated_at
                            && column != "laravel_through_key"
                            && !Self::uniqueIds().contains(&column)
                            && !metadata.refreshes.contains(&column)
                            && !except.contains(&column))
                })
                .map(|(column, value)| (column.clone(), value.clone()))
                .collect();
            let mut replica = Self::from_state(source.replica(attributes));
            let connection = match source.connection.clone() {
                Some(connection) => Some(connection),
                None if Connection::model_container(None).is_some() => {
                    Some(DB::connection(metadata.connection)?)
                }
                None => None,
            };
            if let Some(connection) = connection {
                concerns::has_events::fire(
                    &mut replica,
                    &connection,
                    concerns::ModelEvent::Replicating,
                    false,
                )
                .await?;
            }
            Ok(replica)
        }
    }

    /// Creates an unsaved copy with model lifecycle events suppressed.
    fn replicateQuietly<'a>(
        &'a self,
        except: &'a [&str],
    ) -> impl Future<Output = DatabaseResult<Self>> + Send + 'a {
        Self::withoutEvents(self.replicate(except))
    }

    /// Overrides casts for this model instance. Later entries win for the same attribute.
    fn mergeCasts(
        &mut self,
        casts: impl IntoIterator<Item = (impl Into<String>, AttributeCast)>,
    ) -> &mut Self {
        let state = self.state_mut();
        state.casts.merge(casts);
        state.invalidate_value_objects();
        self
    }

    fn fill(&mut self, attributes: Attributes) -> DatabaseResult<&mut Self> {
        let incoming = concerns::guards_attributes::fill(attributes, Self::metadata())?;
        if incoming.is_empty() {
            return Ok(self);
        }
        let state = self.state();
        let mut attributes = state.attributes.clone();
        let connection = state.connection.as_ref();
        for (name, value) in incoming {
            casts::write::<Self>(&mut attributes, name, value, connection, &state.casts)?;
        }
        self.state_mut().replace_attributes(attributes);
        Ok(self)
    }

    /// Mass-assigns attributes, including plaintext values for hashed casts.
    fn fillAsync(
        &mut self,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<&mut Self>> + Send {
        async move {
            if !attributes
                .keys()
                .any(|name| casts::needs_async::<Self>(name, &self.state().casts))
            {
                return self.fill(attributes);
            }
            let incoming = concerns::guards_attributes::fill(attributes, Self::metadata())?;
            if incoming.is_empty() {
                return Ok(self);
            }
            let state = self.state();
            let mut attributes = state.attributes.clone();
            let connection = state.connection.as_ref();
            for (name, value) in incoming {
                casts::write_async::<Self>(&mut attributes, name, value, connection, &state.casts)
                    .await?;
            }
            self.state_mut().replace_attributes(attributes);
            Ok(self)
        }
    }

    fn forceFill(&mut self, attributes: Attributes) -> &mut Self {
        self.state_mut().apply_attributes(attributes);
        self
    }

    fn getAttribute(&self, column: &str) -> Option<&Value> {
        self.state().getAttribute(column)
    }

    /// Reads through the model accessor or cast.
    fn getAttributeValue(&self, column: &str) -> DatabaseResult<Option<Value>> {
        casts::read::<Self>(self.state(), column)
    }

    /// Reads the Rust case of an attribute declared with an enum cast.
    fn getEnum<E: CastableEnum>(&self, column: &str) -> DatabaseResult<Option<E>> {
        let state = self.state();
        casts::read_enum::<Self, E>(&state.attributes, column, &state.casts)
    }

    /// Assigns the Rust case of an attribute declared with an enum cast.
    fn setEnum<E: CastableEnum>(&mut self, column: &str, value: E) -> DatabaseResult<&mut Self> {
        casts::check_enum::<Self, E>(column, &self.state().casts)?;
        self.setAttributeValue(column, value.intoValue())
    }

    fn getEnumCollection<E: CastableEnum>(
        &self,
        column: &str,
    ) -> DatabaseResult<Option<crate::support::Collection<E>>> {
        casts::read_enum_collection(self, column)
    }

    fn setEnumCollection<E: CastableEnum>(
        &mut self,
        column: &str,
        values: crate::support::Collection<E>,
    ) -> DatabaseResult<&mut Self> {
        casts::write_enum_collection(self, column, values)?;
        Ok(self)
    }

    /// Applies an edit to a typed enum collection before writing its backing values.
    fn editEnumCollection<E: CastableEnum>(
        &mut self,
        column: &str,
        edit: impl FnOnce(&mut crate::support::Collection<E>) -> DatabaseResult<()>,
    ) -> DatabaseResult<&mut Self> {
        let mut values = self
            .getEnumCollection::<E>(column)?
            .ok_or(DatabaseError::InvalidQuery("missing enum collection"))?;
        edit(&mut values)?;
        self.setEnumCollection(column, values)
    }

    /// Borrows the cached object or constructs an owned value when caching is disabled.
    fn getValueObject<T: CastableObject>(
        &mut self,
        column: &str,
    ) -> DatabaseResult<Option<Cow<'_, T>>> {
        let state = self.state();
        casts::check_object::<Self, T>(state, column)?;
        if T::WITHOUT_OBJECT_CACHING {
            return T::fromAttributes(column, &state.attributes).map(|value| value.map(Cow::Owned));
        }
        if state.value_objects().get::<T>(column).is_none() {
            let Some(value) = T::fromAttributes(column, &state.attributes)? else {
                return Ok(None);
            };
            self.state_mut().value_objects_mut().insert(column, value);
        }
        Ok(self.state().value_objects().get(column).map(Cow::Borrowed))
    }

    /// Writes the object's column patch and retains the typed value.
    fn setValueObject<T: CastableObject>(
        &mut self,
        column: &str,
        value: Option<T>,
    ) -> DatabaseResult<&mut Self> {
        let state = self.state();
        casts::check_object::<Self, T>(state, column)?;
        let patch = T::toAttributes(value.as_ref(), column, &state.attributes)?;
        let state = self.state_mut();
        state.invalidate_value_objects();
        patch.apply(&mut state.attributes, column.into());
        if let Some(value) = value
            && !T::WITHOUT_OBJECT_CACHING
        {
            state.value_objects_mut().insert(column, value);
        }
        Ok(self)
    }

    /// Applies an object edit and its column patch only after both operations succeed.
    fn editValueObject<T: CastableObject>(
        &mut self,
        column: &str,
        edit: impl FnOnce(&mut T) -> DatabaseResult<()>,
    ) -> DatabaseResult<&mut Self> {
        let mut value = self
            .getValueObject::<T>(column)?
            .map(Cow::into_owned)
            .ok_or(DatabaseError::InvalidQuery("missing value object"))?;
        edit(&mut value)?;
        self.setValueObject(column, Some(value))
    }

    /// Reads a fresh collection from an encrypted collection attribute.
    fn getEncryptedCollection(
        &self,
        column: &str,
    ) -> DatabaseResult<Option<crate::support::Collection>> {
        casts::check_encrypted_collection::<Self>(column, &self.state().casts)?;
        match self.getAttributeValue(column)? {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Json(value)) => serde_json::from_value(value)
                .map(Some)
                .map_err(|_| DatabaseError::InvalidQuery("encrypted collection attribute")),
            Some(_) => Err(DatabaseError::InvalidQuery(
                "encrypted collection attribute",
            )),
        }
    }

    /// Reads an array object or collection cast while preserving its keys.
    fn getJsonCollection(
        &self,
        column: &str,
    ) -> DatabaseResult<Option<crate::support::Collection>> {
        casts::read_json_collection(self, column)
    }

    /// Deserializes each projected item into an application type, retaining its key.
    fn getJsonCollectionAs<T: serde::de::DeserializeOwned>(
        &self,
        column: &str,
    ) -> DatabaseResult<Option<crate::support::Collection<T>>> {
        self.getJsonCollection(column)?
            .map(|values| {
                values.tryMap(|value, _| {
                    T::deserialize(value)
                        .map_err(|_| DatabaseError::InvalidQuery("JSON collection item"))
                })
            })
            .transpose()
    }

    /// Writes a collection's ordered values to a JSON collection attribute.
    fn setJsonCollection<T: serde::Serialize>(
        &mut self,
        column: &str,
        values: crate::support::Collection<T>,
    ) -> DatabaseResult<&mut Self> {
        casts::write_json_collection(self, column, values)?;
        Ok(self)
    }

    /// Applies an edit to a decoded JSON collection before assigning it.
    fn editJsonCollection(
        &mut self,
        column: &str,
        edit: impl FnOnce(&mut crate::support::Collection) -> DatabaseResult<()>,
    ) -> DatabaseResult<&mut Self> {
        let mut values = self
            .getJsonCollection(column)?
            .ok_or(DatabaseError::InvalidQuery("missing JSON collection"))?;
        edit(&mut values)?;
        self.setJsonCollection(column, values)
    }

    /// Updates a typed encrypted collection after its edit succeeds.
    fn editEncryptedCollection(
        &mut self,
        column: &str,
        edit: impl FnOnce(&mut crate::support::Collection) -> DatabaseResult<()>,
    ) -> DatabaseResult<&mut Self> {
        let mut values = self
            .getEncryptedCollection(column)?
            .ok_or(DatabaseError::InvalidQuery("missing encrypted collection"))?;
        edit(&mut values)?;
        let value = values
            .toArray()
            .map_err(|_| DatabaseError::InvalidQuery("encrypted collection attribute"))?;
        self.setAttributeValue(column, Value::Json(value))
    }

    /// Replaces a nested JSON value and writes the updated document to its model column.
    fn fillJsonAttribute(
        &mut self,
        path: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<&mut Self> {
        let state = self.state_mut();
        casts::write_json_path::<Self>(
            &mut state.attributes,
            path,
            value.into(),
            state.connection.as_ref(),
            &state.casts,
        )?;
        state.invalidate_value_objects();
        Ok(self)
    }

    /// Writes through the model mutator or storage cast.
    fn setAttributeValue(
        &mut self,
        column: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<&mut Self> {
        let state = self.state_mut();
        casts::write::<Self>(
            &mut state.attributes,
            column.into(),
            value.into(),
            state.connection.as_ref(),
            &state.casts,
        )?;
        state.invalidate_value_objects();
        Ok(self)
    }

    /// Assigns an attribute without blocking the async executor for a password hash.
    fn setAttributeValueAsync<'a>(
        &'a mut self,
        column: &'a str,
        value: impl Into<Value>,
    ) -> impl Future<Output = DatabaseResult<&'a mut Self>> + Send + 'a {
        let value = value.into();
        async move {
            if !casts::needs_async::<Self>(column, &self.state().casts) {
                return self.setAttributeValue(column, value);
            }
            let state = self.state_mut();
            casts::write_async::<Self>(
                &mut state.attributes,
                column.into(),
                value,
                state.connection.as_ref(),
                &state.casts,
            )
            .await?;
            state.invalidate_value_objects();
            Ok(self)
        }
    }

    fn getRawOriginal(&self, column: &str) -> Option<&Value> {
        self.state().original.get(column)
    }

    /// Compares a current attribute with its original using its declared cast.
    fn originalIsEquivalent(&self, column: &str) -> DatabaseResult<bool> {
        let state = self.state();
        match (state.attributes.get(column), state.original.get(column)) {
            (Some(current), Some(original)) => casts::equivalent::<Self>(
                column,
                current,
                original,
                state.connection.as_ref(),
                &state.casts,
            ),
            _ => Ok(false),
        }
    }

    fn getDirty(&self) -> DatabaseResult<Attributes> {
        let mut dirty = Attributes::new();
        for (name, value) in &self.state().attributes {
            if !self.originalIsEquivalent(name)? {
                dirty.insert(name.clone(), value.clone());
            }
        }
        Ok(dirty)
    }

    fn isDirty(&self) -> DatabaseResult<bool> {
        Ok(!self.getDirty()?.is_empty())
    }

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

    fn getRouteKeyName() -> &'static str {
        Self::metadata().primary_key
    }

    fn getRouteKey(&self) -> DatabaseResult<Option<Value>> {
        self.getAttributeValue(Self::getRouteKeyName())
    }

    fn routeBindingValue(field: &str, value: &str) -> Option<Value> {
        if Self::uniqueIds().contains(&field) && !Self::isValidUniqueId(value) {
            return None;
        }
        if field == Self::metadata().primary_key && Self::getIncrementing() {
            return value
                .parse::<i64>()
                .map(Value::Integer)
                .or_else(|_| value.parse::<u64>().map(Value::Unsigned))
                .ok();
        }
        Some(Value::from(value))
    }

    fn resolveRouteBinding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        Box::pin(async move {
            let query = Self::query().map_err(route_binding::http_error)?;
            route_binding::lookup(query, binding)
                .await
                .map_err(route_binding::http_error)
        })
    }

    fn resolveChildRouteBinding(
        _binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        Box::pin(async {
            Err(route_binding::http_error(DatabaseError::InvalidQuery(
                "scoped route binding relation",
            )))
        })
    }

    fn getTable(&self) -> &'static str {
        self.state()
            .pivot_identity
            .map_or(Self::metadata().table, |identity| identity.table)
    }

    fn getConnectionName(&self) -> Option<&str> {
        self.state()
            .connection
            .as_ref()
            .map(Connection::getName)
            .or(Self::metadata().connection)
    }

    /// Compares the primary key, table, and connection name of two models.
    fn is(&self, other: &impl Model) -> bool {
        self.getKey().unwrap_or(&Value::Null) == other.getKey().unwrap_or(&Value::Null)
            && self.getTable() == other.getTable()
            && self.getConnectionName() == other.getConnectionName()
    }

    fn isNot(&self, other: &impl Model) -> bool {
        !self.is(other)
    }

    /// Registers or replaces a named relation resolver in this application's container.
    fn resolveRelationUsing<R: Relation<Parent = Self>>(
        app: &crate::foundation::Application,
        name: impl Into<String>,
        resolver: impl Fn(&Self) -> R + Send + Sync + 'static,
    ) -> DatabaseResult<()> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        let name = name.into();
        if name.is_empty() || name.contains('.') {
            return Err(DatabaseError::InvalidQuery("dynamic relationship name"));
        }
        app.container()
            .make::<super::RelationResolvers>()
            .map_err(DatabaseError::Unavailable)?
            .register::<Self, R>(name, resolver);
        Ok(())
    }

    /// Falls back to another model's named resolver when this model has no local resolver.
    ///
    /// The closures construct the base model for resolution and adapt its typed relation.
    fn inheritRelationUsing<B, S, R>(
        app: &crate::foundation::Application,
        name: impl Into<String>,
        base: impl Fn(&Self) -> B + Send + Sync + 'static,
        adapt: impl Fn(S) -> R + Send + Sync + 'static,
    ) -> DatabaseResult<()>
    where
        B: Model,
        S: Relation<Parent = B>,
        R: Relation<Parent = Self>,
    {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        let name = name.into();
        if name.is_empty() || name.contains('.') {
            return Err(DatabaseError::InvalidQuery("dynamic relationship name"));
        }
        app.container()
            .make::<super::RelationResolvers>()
            .map_err(DatabaseError::Unavailable)?
            .inherit::<Self, B, S, R>(name, base, adapt);
        Ok(())
    }

    /// Resolves a named relationship using the model's application.
    fn relationResolver<R: Relation<Parent = Self>>(
        &self,
        name: &str,
    ) -> DatabaseResult<Option<R>> {
        super::RelationResolvers::resolve(self, name)
    }

    /// Loads a named dynamic relationship on first access.
    fn getDynamicRelationOrLoad<R: Relation<Parent = Self>>(
        &mut self,
        name: &str,
    ) -> impl Future<Output = DatabaseResult<&R::Output>> + Send {
        async move {
            let relation = super::DynamicRelation::<R>::new(name);
            super::detached_autoload::load_on_model_access(self, name, &relation).await?;
            self.getRelation(name)
                .ok_or(DatabaseError::InvalidQuery("relationship output type"))
        }
    }

    fn relationLoaded(&self, name: &str) -> bool {
        self.state().relations.contains(name)
    }

    fn getRelation<T: Send + Sync + 'static>(&self, name: &str) -> Option<&T> {
        self.state().relations.get(name)
    }

    /// Reads a relationship, querying once if it has not been loaded.
    fn getRelationOrLoad<'a, R: Relation<Parent = Self>>(
        &'a mut self,
        name: &'a str,
        relation: &'a R,
    ) -> impl Future<Output = DatabaseResult<&'a R::Output>> + Send + 'a {
        async move {
            super::detached_autoload::load_on_model_access(self, name, relation).await?;
            self.getRelation(name)
                .ok_or(DatabaseError::InvalidQuery("relationship output type"))
        }
    }

    fn unsetRelation(&mut self, name: &str) {
        let state = self.state_mut();
        state.relations.remove(name);
        if let Some(link) = &mut state.detached_autoload {
            link.invalidate(name);
        }
    }

    /// Eager loads registered paths onto this model.
    fn loadPaths(&mut self, paths: &[&str]) -> impl Future<Output = DatabaseResult<()>> + Send {
        async move {
            let loads = super::NamedRelations::<Self>::prepare(paths)?;
            concerns::has_relationships::load_eager(loads, std::slice::from_mut(self)).await
        }
    }

    /// Loads missing relationships along the supplied registered paths.
    fn loadMissingPaths<'a>(
        &'a mut self,
        paths: &'a [&'a str],
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        super::NamedRelations::<Self>::load_missing(std::slice::from_mut(self), paths)
    }

    fn load<'a, R: Relation<Parent = Self>>(
        &'a mut self,
        name: &'a str,
        relation: &'a R,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        concerns::has_relationships::load(std::slice::from_mut(self), name, relation, false)
    }

    /// Registers a listener on this application's model event registry.
    fn registerModelEvent(
        app: &crate::foundation::Application,
        event: super::concerns::ModelEvent,
        listener: impl super::concerns::ModelEventListener<Self> + 'static,
    ) -> DatabaseResult<()> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        app.container()
            .make::<super::ModelEvents>()
            .map_err(DatabaseError::Unavailable)?
            .listen::<Self>(event, listener);
        Ok(())
    }

    /// Registers a typed observer's lifecycle methods for this application.
    fn observe<O: super::Observer<Self>>(
        app: &crate::foundation::Application,
        observer: O,
    ) -> DatabaseResult<()> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        app.container()
            .make::<super::ModelEvents>()
            .map_err(DatabaseError::Unavailable)?
            .listen_to::<Self>(O::EVENTS, super::observer::ObserverListener(observer));
        Ok(())
    }

    /// Registers an observer whose non-vetoing callbacks wait for the transaction to commit.
    ///
    /// A deferred callback receives a clone of the model taken when the event fires.
    fn observeAfterCommit<O>(
        app: &crate::foundation::Application,
        observer: O,
    ) -> DatabaseResult<()>
    where
        Self: Clone,
        O: super::Observer<Self> + crate::contracts::events::ShouldHandleEventsAfterCommit,
    {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        app.container()
            .make::<super::ModelEvents>()
            .map_err(DatabaseError::Unavailable)?
            .listen_to::<Self>(
                O::EVENTS,
                super::observer::AfterCommitObserver::new(observer),
            );
        Ok(())
    }

    /// Suppresses model events in the current async call chain.
    fn withoutEvents<F: Future + Send>(work: F) -> impl Future<Output = F::Output> + Send {
        concerns::has_events::without_events(work)
    }

    fn saveQuietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::withoutEvents(self.save())
    }

    fn deleteQuietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::withoutEvents(self.delete())
    }

    fn forceDeleteQuietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::withoutEvents(self.forceDelete())
    }

    fn restoreQuietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::withoutEvents(self.restore())
    }

    fn save(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::save(self, None)
    }

    fn update(
        &mut self,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::update(self, attributes, None)
    }

    fn delete(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::delete(self, false, None)
    }

    fn forceDelete(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::delete(self, true, None)
    }

    fn restore(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        persistence::restore(self, None)
    }

    fn saveInTransaction<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        persistence::save(self, Some(transaction))
    }

    fn updateInTransaction<'a>(
        &'a mut self,
        attributes: Attributes,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        persistence::update(self, attributes, Some(transaction))
    }

    fn deleteInTransaction<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        persistence::delete(self, false, Some(transaction))
    }

    fn forceDeleteInTransaction<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        persistence::delete(self, true, Some(transaction))
    }

    fn restoreInTransaction<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        persistence::restore(self, Some(transaction))
    }

    fn trashed(&self) -> bool {
        Self::metadata()
            .deleted_at
            .is_some_and(|column| soft_deletes::is_trashed(self.getAttribute(column)))
    }
}
