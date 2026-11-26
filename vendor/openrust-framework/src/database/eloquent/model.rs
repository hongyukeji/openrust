use std::{borrow::Cow, future::Future};

use crate::{
    database::{Connection, DatabaseError, DatabaseResult, Db, Value, events::with_query_caller},
    http::{
        HttpError, HttpFuture,
        resources::json::{JsonResource, UseResource},
    },
    routing::BindingContext,
    support::Str,
};

use super::{
    Attributes, Builder, Collection, EagerLoads, ModelMetadata, ModelState, RelationAggregate,
    RelationAggregateQuery, Scope, Touches, VisibilityCondition,
    casts::{self, Attribute, AttributeCast, CastableEnum, CastableObject},
    concerns, persistence,
    relations::{ExistenceRelation, Relation},
    route_binding, serialization, soft_deletes,
};

/// The database key type used when referencing an Eloquent model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKeyType {
    Integer,
    String,
    Uuid,
    Ulid,
}

/// A model type implemented by the application.
pub trait Model: Sized + Send + Sync + 'static {
    fn metadata() -> &'static ModelMetadata;
    fn from_state(state: ModelState) -> Self;
    fn state(&self) -> &ModelState;
    fn state_mut(&mut self) -> &mut ModelState;

    /// Builds the resource declared for this model.
    fn to_resource(self) -> <Self as UseResource>::Resource
    where
        Self: UseResource,
    {
        self.to_resource_with()
    }

    /// Selects a resource explicitly for this model.
    fn to_resource_with<R: JsonResource + From<Self>>(self) -> R {
        R::make(self)
    }

    /// 定制模型查询的默认行为；通过 fromQuery 构造后沿用公共查询入口。
    fn new_eloquent_builder(
        query: crate::database::query::Builder<'static>,
    ) -> Builder<'static, Self> {
        Builder::from_query(query)
    }

    fn global_scopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[]
    }

    /// Declares the owners whose timestamps a dirty save should update.
    fn touches() -> Touches<Self> {
        Touches::default()
    }

    /// Maps a lifecycle event to a typed application event using the current model state.
    fn dispatches_events(
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

    fn get_hidden(&self) -> Vec<&str> {
        serialization::names(
            self.state().serialization().hidden.as_deref(),
            Self::metadata().hidden,
        )
        .collect()
    }

    fn set_hidden(&mut self, attributes: &[&str]) -> &mut Self {
        self.state_mut().serialization_mut().hidden =
            Some(attributes.iter().map(|name| (*name).into()).collect());
        self
    }

    fn merge_hidden(&mut self, attributes: &[&str]) -> &mut Self {
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

    fn get_visible(&self) -> Vec<&str> {
        serialization::names(
            self.state().serialization().visible.as_deref(),
            Self::metadata().visible,
        )
        .collect()
    }

    fn set_visible(&mut self, attributes: &[&str]) -> &mut Self {
        self.state_mut().serialization_mut().visible =
            Some(attributes.iter().map(|name| (*name).into()).collect());
        self
    }

    fn merge_visible(&mut self, attributes: &[&str]) -> &mut Self {
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

    fn get_appends(&self) -> Vec<&str> {
        serialization::names(
            self.state().serialization().appends.as_deref(),
            Self::metadata().appends,
        )
        .collect()
    }

    fn set_appends(&mut self, attributes: &[&str]) -> &mut Self {
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

    fn make_visible(&mut self, attributes: &[&str]) -> &mut Self {
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

    fn make_hidden(&mut self, attributes: &[&str]) -> &mut Self {
        self.merge_hidden(attributes)
    }

    fn make_visible_if(
        &mut self,
        condition: impl VisibilityCondition<Self>,
        attributes: &[&str],
    ) -> &mut Self {
        if condition.evaluate(self) {
            self.make_visible(attributes);
        }
        self
    }

    fn make_hidden_if(
        &mut self,
        condition: impl VisibilityCondition<Self>,
        attributes: &[&str],
    ) -> &mut Self {
        if condition.evaluate(self) {
            self.make_hidden(attributes);
        }
        self
    }

    fn has_appended(&self, name: &str) -> bool {
        serialization::names(
            self.state().serialization().appends.as_deref(),
            Self::metadata().appends,
        )
        .any(|attribute| attribute == name)
    }

    fn without_appends(&mut self) -> &mut Self {
        self.set_appends(&[])
    }

    /// Formats a date for JSON output without changing its database representation.
    fn serialize_date(&self, date: jiff::Timestamp) -> DatabaseResult<String> {
        Ok(date.strftime("%Y-%m-%dT%H:%M:%S%.6fZ").to_string())
    }

    fn attributes_to_array(&self) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        serialization::attributes(self)
    }

    /// Evaluates an appended accessor against this instance's current state.
    fn mutate_attribute_for_array(
        &self,
        column: &str,
        value: Option<&Value>,
    ) -> DatabaseResult<Value> {
        let getter = Self::attribute(column)
            .and_then(|attribute| attribute.get)
            .ok_or(DatabaseError::InvalidQuery("appended model accessor"))?;
        getter(value, self.state().get_attributes())
    }

    fn relations_to_array(&self) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        let state = self.state();
        let include = |name: &str| state.serialization().includes(Self::metadata(), name);
        match &state.detached_autoload {
            Some(link) => link.project(state, include),
            None => state.relations.project(include),
        }
    }

    fn to_array(&self) -> DatabaseResult<serde_json::Map<String, serde_json::Value>> {
        serialization::model(self)
    }

    fn to_json(&self) -> DatabaseResult<String> {
        serde_json::to_string(&self.to_array()?)
            .map_err(|_| DatabaseError::InvalidQuery("model JSON"))
    }

    /// Declares the relationship definitions allowed to restore this custom pivot from a job.
    fn queueable_pivots() -> Vec<super::QueueablePivot> {
        Vec::new()
    }

    /// Declares the inverse relationship names available on this custom pivot model.
    fn pivot_inverse_relations() -> &'static [&'static str] {
        &[]
    }

    /// Resolves the discriminator from the model's application morph map.
    fn get_morph_class(&self) -> DatabaseResult<&'static str> {
        super::relations::MorphMap::class_for::<Self>(self.state().connection.as_ref())
    }

    /// Constructs the Eloquent collection returned by model queries.
    fn new_collection(models: Vec<Self>) -> Collection<Self> {
        Collection::new(models)
    }

    /// Defines a through relationship by reusing an existing local relationship.
    fn through<L: super::relations::ThroughRelation<Parent = Self>>(local: L) -> L::Pending {
        local.pending()
    }

    /// Defines the relationship descriptors available to named eager loading.
    fn named_relations() -> super::NamedRelations<Self> {
        super::NamedRelations::default()
    }

    /// Declares relationships to load whenever this model is retrieved.
    fn with_default_relations(relations: EagerLoads<Self>) -> EagerLoads<Self> {
        relations
    }

    /// Adds the relationship counts selected by default when reading this model.
    fn with_default_counts(
        query: Builder<'static, Self>,
    ) -> DatabaseResult<Builder<'static, Self>> {
        Ok(query)
    }

    /// Enables first-access relationship batching for collections of this model.
    fn automatically_eager_load_relationships() -> bool {
        false
    }

    /// Prevents implicit single-model relationship queries during access.
    fn prevents_lazy_loading(&self) -> bool {
        self.state().prevent_lazy_loading
    }

    /// 配置当前应用中本模型类型的严格策略，适合在应用启动时设置。
    fn should_be_strict(
        app: &crate::foundation::Application,
        should_be_strict: bool,
    ) -> DatabaseResult<()> {
        super::strictness::configure::<Self>(app, |policy| {
            policy.lazy_loading = should_be_strict;
            policy.discarded_attributes = should_be_strict;
            policy.missing_attributes = should_be_strict;
        })
    }

    fn prevent_lazy_loading(
        app: &crate::foundation::Application,
        value: bool,
    ) -> DatabaseResult<()> {
        super::strictness::configure::<Self>(app, |policy| policy.lazy_loading = value)
    }

    fn prevent_silently_discarding_attributes(
        app: &crate::foundation::Application,
        value: bool,
    ) -> DatabaseResult<()> {
        super::strictness::configure::<Self>(app, |policy| policy.discarded_attributes = value)
    }

    fn prevent_accessing_missing_attributes(
        app: &crate::foundation::Application,
        value: bool,
    ) -> DatabaseResult<()> {
        super::strictness::configure::<Self>(app, |policy| policy.missing_attributes = value)
    }

    fn prevents_silently_discarding_attributes(&self) -> bool {
        super::strictness::policy(self).discarded_attributes
    }

    fn prevents_accessing_missing_attributes(&self) -> bool {
        super::strictness::policy(self).missing_attributes
    }

    /// 可覆盖此钩子记录被丢弃的字段；返回成功会允许本次丢弃。
    fn handle_discarded_attribute_violation_using(
        &mut self,
        _keys: &[String],
        violation: DatabaseError,
    ) -> DatabaseResult<()> {
        Err(violation)
    }

    /// 可覆盖此钩子为缺失属性提供值或记录诊断。
    fn handle_missing_attribute_violation_using(
        &self,
        _key: &str,
        violation: DatabaseError,
    ) -> DatabaseResult<Option<Value>> {
        Err(violation)
    }

    fn without_touching<F: Future>(work: F) -> impl Future<Output = F::Output> {
        super::touches::without_touching(&[std::any::TypeId::of::<Self>()], work)
    }

    fn without_touching_on<F: Future>(
        models: &[std::any::TypeId],
        work: F,
    ) -> impl Future<Output = F::Output> + use<F, Self> {
        super::touches::without_touching(models, work)
    }

    fn is_ignoring_touch() -> bool {
        Self::metadata().updated_at.is_none() || super::touches::is_ignoring::<Self>()
    }

    /// Handles a prohibited lazy access. Returning success permits the query.
    fn handle_lazy_loading_violation_using(&self, violation: DatabaseError) -> DatabaseResult<()> {
        if !self.state().exists() || self.state().was_recently_created() {
            return Ok(());
        }
        Err(violation)
    }

    fn query() -> DatabaseResult<Builder<'static, Self>> {
        Ok(Self::query_on(
            &Self::from_state(ModelState::default()).get_connection()?,
        ))
    }

    fn query_on(connection: &Connection) -> Builder<'static, Self> {
        Builder::new(connection.clone())
            .with_only(Self::with_default_relations)
            .enable_default_counts()
    }

    fn all() -> impl Future<Output = DatabaseResult<Collection<Self>>> + Send {
        async { Self::query()?.get().await }
    }

    fn find(key: impl Into<Value>) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send {
        let key = key.into();
        async move { Self::query()?.find(key).await }
    }

    fn find_or_fail(key: impl Into<Value>) -> impl Future<Output = DatabaseResult<Self>> + Send {
        let key = key.into();
        async move { Self::query()?.find_or_fail(key).await }
    }

    /// Reads a new instance from the write connection using the original model key.
    fn fresh(&self) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send {
        self.fresh_with_paths(&[])
    }

    /// Reads a new instance and preloads the requested registered paths.
    fn fresh_with_paths<'a>(
        &'a self,
        paths: &'a [&'a str],
    ) -> impl Future<Output = DatabaseResult<Option<Self>>> + Send + 'a {
        async move {
            if !self.state().exists {
                return Ok(None);
            }
            persistence::reload_query(self)?
                .with_paths(paths)?
                .first()
                .await
        }
    }

    /// Reloads this instance and its loaded registered relationships.
    fn refresh(&mut self) -> impl Future<Output = DatabaseResult<&mut Self>> + Send {
        persistence::refresh(self, None)
    }

    /// Reloads this model under a write lock held by the caller's transaction.
    fn refresh_for_update<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<&'a mut Self>> + Send + 'a {
        persistence::refresh(self, Some(transaction))
    }

    #[track_caller]
    fn create(attributes: Attributes) -> impl Future<Output = DatabaseResult<Self>> + Send {
        with_query_caller(std::panic::Location::caller(), async move {
            Self::query()?.create(attributes).await
        })
    }

    fn new_instance(attributes: Attributes) -> DatabaseResult<Self> {
        let mut model = Self::from_state(ModelState::default());
        model.fill(attributes)?;
        Ok(model)
    }

    fn new_instance_on(connection: &Connection, attributes: Attributes) -> DatabaseResult<Self> {
        let mut model = Self::new_instance(Attributes::new())?;
        model.state_mut().connection = Some(connection.clone());
        model.fill(attributes)?;
        Ok(model)
    }

    fn unique_ids() -> &'static [&'static str] {
        let metadata = Self::metadata();
        metadata.unique_ids.unwrap_or_else(|| {
            if metadata.unique_id.is_some() {
                std::slice::from_ref(&metadata.primary_key)
            } else {
                &[]
            }
        })
    }

    /// Instance hooks preserve options initialized when a model is constructed.
    fn unique_ids_for_instance(&self) -> &'static [&'static str] {
        Self::unique_ids()
    }

    fn get_incrementing_for_instance(&self) -> bool {
        Self::get_incrementing()
    }

    fn uses_unique_ids_for_instance(&self) -> bool {
        Self::uses_unique_ids()
    }

    fn uses_unique_ids() -> bool {
        !Self::unique_ids().is_empty()
    }

    fn new_unique_id(&self) -> DatabaseResult<Option<String>> {
        Self::metadata()
            .unique_id
            .map(super::UniqueId::generate)
            .transpose()
    }

    fn is_valid_unique_id(value: &str) -> bool {
        Self::metadata()
            .unique_id
            .is_none_or(|kind| kind.valid(value))
    }

    fn set_unique_ids(&mut self) -> impl Future<Output = DatabaseResult<()>> + Send {
        async {
            for column in self.unique_ids_for_instance() {
                if super::unique_ids::is_empty(self.get_attribute_value(column)?) {
                    let id = self.new_unique_id()?.map_or(Value::Null, Value::Text);
                    self.set_attribute_value_async(column, id).await?;
                }
            }
            Ok(())
        }
    }

    fn get_incrementing() -> bool {
        let metadata = Self::metadata();
        metadata.incrementing && !Self::unique_ids().contains(&metadata.primary_key)
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
                            && !self.unique_ids_for_instance().contains(&column)
                            && !metadata.refreshes.contains(&column)
                            && !except.contains(&column))
                })
                .map(|(column, value)| (column.clone(), value.clone()))
                .collect();
            let mut replica = Self::from_state(source.replica(attributes));
            let connection = match source.connection.clone() {
                Some(connection) => Some(connection),
                None if Connection::model_container(None).is_some() => {
                    Some(Db::connection(metadata.connection)?)
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
    fn replicate_quietly<'a>(
        &'a self,
        except: &'a [&str],
    ) -> impl Future<Output = DatabaseResult<Self>> + Send + 'a {
        Self::without_events(self.replicate(except))
    }

    /// Overrides casts for this model instance. Later entries win for the same attribute.
    fn merge_casts(
        &mut self,
        casts: impl IntoIterator<Item = (impl Into<String>, AttributeCast)>,
    ) -> &mut Self {
        let state = self.state_mut();
        state.casts.merge(casts);
        state.invalidate_value_objects();
        self
    }

    /// Returns the model's declared mass-assignable attribute names.
    fn get_fillable(&self) -> &'static [&'static str] {
        Self::metadata().fillable
    }

    /// Returns the model's declared guarded attribute names.
    fn get_guarded(&self) -> &'static [&'static str] {
        Self::metadata().guarded
    }

    /// Whether the current task or synchronous force-fill scope bypasses guards.
    fn is_unguarded() -> bool {
        concerns::guards_attributes::is_unguarded()
    }

    /// Bypasses mass-assignment guards while polling this future, restoring on exit.
    fn unguarded<F: Future>(work: F) -> impl Future<Output = F::Output> {
        concerns::guards_attributes::unguarded(work)
    }

    /// Returns the configured soft-delete column, or `None` when soft deletes are disabled.
    fn get_deleted_at_column(&self) -> Option<&'static str> {
        Self::metadata().deleted_at
    }

    fn fill(&mut self, attributes: Attributes) -> DatabaseResult<&mut Self> {
        let mut discarded = Vec::new();
        for (name, value) in attributes {
            if concerns::guards_attributes::is_fillable(&name, Self::metadata()) {
                self.set_attribute_value(&name, value)?;
            } else if Self::metadata().fillable.is_empty() {
                if concerns::guards_attributes::totally_guarded(Self::metadata())
                    || self.prevents_silently_discarding_attributes()
                {
                    self.handle_discarded_attribute_violation_using(
                        std::slice::from_ref(&name),
                        DatabaseError::MassAssignment(name.clone()),
                    )?;
                }
            } else {
                discarded.push(name);
            }
        }
        if self.prevents_silently_discarding_attributes() && !discarded.is_empty() {
            self.handle_discarded_attribute_violation_using(
                &discarded,
                DatabaseError::MassAssignment(discarded.join(", ")),
            )?;
        }
        Ok(self)
    }

    /// 批量赋值保留异步转换器，并按字段赋值顺序处理保护策略。
    fn fill_async(
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
            let mut discarded = Vec::new();
            for (name, value) in attributes {
                if concerns::guards_attributes::is_fillable(&name, Self::metadata()) {
                    self.set_attribute_value_async(&name, value).await?;
                } else if Self::metadata().fillable.is_empty() {
                    if concerns::guards_attributes::totally_guarded(Self::metadata())
                        || self.prevents_silently_discarding_attributes()
                    {
                        self.handle_discarded_attribute_violation_using(
                            std::slice::from_ref(&name),
                            DatabaseError::MassAssignment(name.clone()),
                        )?;
                    }
                } else {
                    discarded.push(name);
                }
            }
            if self.prevents_silently_discarding_attributes() && !discarded.is_empty() {
                self.handle_discarded_attribute_violation_using(
                    &discarded,
                    DatabaseError::MassAssignment(discarded.join(", ")),
                )?;
            }
            Ok(self)
        }
    }

    /// Bypasses mass-assignment guards while retaining attribute mutators and casts.
    fn force_fill(&mut self, attributes: Attributes) -> DatabaseResult<&mut Self> {
        concerns::guards_attributes::unguarded_sync(|| self.fill(attributes))
    }

    /// Retains asynchronous setters, including password hashing, without bypassing casts.
    fn force_fill_async(
        &mut self,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<&mut Self>> + Send {
        concerns::guards_attributes::unguarded(self.fill_async(attributes))
    }

    fn get_attribute(&self, column: &str) -> Option<&Value> {
        self.state().get_attribute(column)
    }

    /// Reads through the model accessor or cast.
    fn get_attribute_value(&self, column: &str) -> DatabaseResult<Option<Value>> {
        let state = self.state();
        if state.exists
            && !state.was_recently_created
            && self.prevents_accessing_missing_attributes()
            && !state.attributes.contains_key(column)
            && casts::guards_missing::<Self>(state, column)
        {
            let fallback = self.handle_missing_attribute_violation_using(
                column,
                DatabaseError::MissingAttribute {
                    model: std::any::type_name::<Self>(),
                    attribute: column.into(),
                },
            )?;
            if casts::cast_for::<Self>(column, &state.casts).is_none() {
                return Ok(fallback);
            }
        }
        casts::read::<Self>(state, column)
    }

    /// Reads the Rust case of an attribute declared with an enum cast.
    fn get_enum<E: CastableEnum>(&self, column: &str) -> DatabaseResult<Option<E>> {
        let state = self.state();
        casts::check_enum::<Self, E>(column, &state.casts)?;
        if state.exists
            && !state.was_recently_created
            && self.prevents_accessing_missing_attributes()
            && !state.attributes.contains_key(column)
        {
            self.handle_missing_attribute_violation_using(
                column,
                DatabaseError::MissingAttribute {
                    model: std::any::type_name::<Self>(),
                    attribute: column.into(),
                },
            )?;
        }
        casts::read_enum::<Self, E>(&state.attributes, column, &state.casts)
    }

    /// Assigns the Rust case of an attribute declared with an enum cast.
    fn set_enum<E: CastableEnum>(&mut self, column: &str, value: E) -> DatabaseResult<&mut Self> {
        casts::check_enum::<Self, E>(column, &self.state().casts)?;
        self.set_attribute_value(column, value.into_value())
    }

    fn get_enum_collection<E: CastableEnum>(
        &self,
        column: &str,
    ) -> DatabaseResult<Option<crate::support::Collection<E>>> {
        casts::read_enum_collection(self, column)
    }

    fn set_enum_collection<E: CastableEnum>(
        &mut self,
        column: &str,
        values: crate::support::Collection<E>,
    ) -> DatabaseResult<&mut Self> {
        casts::write_enum_collection(self, column, values)?;
        Ok(self)
    }

    /// Applies an edit to a typed enum collection before writing its backing values.
    fn edit_enum_collection<E: CastableEnum>(
        &mut self,
        column: &str,
        edit: impl FnOnce(&mut crate::support::Collection<E>) -> DatabaseResult<()>,
    ) -> DatabaseResult<&mut Self> {
        let mut values = self
            .get_enum_collection::<E>(column)?
            .ok_or(DatabaseError::InvalidQuery("missing enum collection"))?;
        edit(&mut values)?;
        self.set_enum_collection(column, values)
    }

    /// Borrows the cached object or constructs an owned value when caching is disabled.
    fn get_value_object<T: CastableObject>(
        &mut self,
        column: &str,
    ) -> DatabaseResult<Option<Cow<'_, T>>> {
        let state = self.state();
        casts::check_object::<Self, T>(state, column)?;
        if T::WITHOUT_OBJECT_CACHING {
            return casts::read_object::<T>(column, &state.attributes, state.connection.as_ref())
                .map(|value| value.map(Cow::Owned));
        }
        if state.value_objects().get::<T>(column).is_none() {
            let Some(value) =
                casts::read_object::<T>(column, &state.attributes, state.connection.as_ref())?
            else {
                return Ok(None);
            };
            self.state_mut().value_objects_mut().insert(column, value);
        }
        Ok(self.state().value_objects().get(column).map(Cow::Borrowed))
    }

    /// Writes the object's column patch and retains the typed value.
    fn set_value_object<T: CastableObject>(
        &mut self,
        column: &str,
        value: Option<T>,
    ) -> DatabaseResult<&mut Self> {
        let state = self.state();
        casts::check_object::<Self, T>(state, column)?;
        let patch = casts::write_object(
            value.as_ref(),
            column,
            &state.attributes,
            state.connection.as_ref(),
        )?;
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
    fn edit_value_object<T: CastableObject>(
        &mut self,
        column: &str,
        edit: impl FnOnce(&mut T) -> DatabaseResult<()>,
    ) -> DatabaseResult<&mut Self> {
        let mut value = self
            .get_value_object::<T>(column)?
            .map(Cow::into_owned)
            .ok_or(DatabaseError::InvalidQuery("missing value object"))?;
        edit(&mut value)?;
        self.set_value_object(column, Some(value))
    }

    /// Reads a fresh collection from an encrypted collection attribute.
    fn get_encrypted_collection(
        &self,
        column: &str,
    ) -> DatabaseResult<Option<crate::support::Collection>> {
        casts::check_encrypted_collection::<Self>(column, &self.state().casts)?;
        match self.get_attribute_value(column)? {
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
    fn get_json_collection(
        &self,
        column: &str,
    ) -> DatabaseResult<Option<crate::support::Collection>> {
        casts::read_json_collection(self, column)
    }

    /// Deserializes each projected item into an application type, retaining its key.
    fn get_json_collection_as<T: serde::de::DeserializeOwned>(
        &self,
        column: &str,
    ) -> DatabaseResult<Option<crate::support::Collection<T>>> {
        self.get_json_collection(column)?
            .map(|values| {
                values.try_map(|value, _| {
                    T::deserialize(value)
                        .map_err(|_| DatabaseError::InvalidQuery("JSON collection item"))
                })
            })
            .transpose()
    }

    /// Writes a collection's ordered values to a JSON collection attribute.
    fn set_json_collection<T: serde::Serialize>(
        &mut self,
        column: &str,
        values: crate::support::Collection<T>,
    ) -> DatabaseResult<&mut Self> {
        casts::write_json_collection(self, column, values)?;
        Ok(self)
    }

    /// Applies an edit to a decoded JSON collection before assigning it.
    fn edit_json_collection(
        &mut self,
        column: &str,
        edit: impl FnOnce(&mut crate::support::Collection) -> DatabaseResult<()>,
    ) -> DatabaseResult<&mut Self> {
        let mut values = self
            .get_json_collection(column)?
            .ok_or(DatabaseError::InvalidQuery("missing JSON collection"))?;
        edit(&mut values)?;
        self.set_json_collection(column, values)
    }

    /// Updates a typed encrypted collection after its edit succeeds.
    fn edit_encrypted_collection(
        &mut self,
        column: &str,
        edit: impl FnOnce(&mut crate::support::Collection) -> DatabaseResult<()>,
    ) -> DatabaseResult<&mut Self> {
        let mut values = self
            .get_encrypted_collection(column)?
            .ok_or(DatabaseError::InvalidQuery("missing encrypted collection"))?;
        edit(&mut values)?;
        let value = values
            .to_array()
            .map_err(|_| DatabaseError::InvalidQuery("encrypted collection attribute"))?;
        self.set_attribute_value(column, Value::Json(value))
    }

    /// Replaces a nested JSON value and writes the updated document to its model column.
    fn fill_json_attribute(
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
    fn set_attribute_value(
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
    fn set_attribute_value_async<'a>(
        &'a mut self,
        column: &'a str,
        value: impl Into<Value>,
    ) -> impl Future<Output = DatabaseResult<&'a mut Self>> + Send + 'a {
        let value = value.into();
        async move {
            if !casts::needs_async::<Self>(column, &self.state().casts) {
                return self.set_attribute_value(column, value);
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

    fn get_raw_original(&self, column: &str) -> Option<&Value> {
        self.state().original.get(column)
    }

    /// Compares a current attribute with its original using its declared cast.
    fn original_is_equivalent(&self, column: &str) -> DatabaseResult<bool> {
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

    fn get_dirty(&self) -> DatabaseResult<Attributes> {
        let mut dirty = Attributes::new();
        for (name, value) in &self.state().attributes {
            if !self.original_is_equivalent(name)? {
                dirty.insert(name.clone(), value.clone());
            }
        }
        Ok(dirty)
    }

    fn is_dirty(&self) -> DatabaseResult<bool> {
        Ok(!self.get_dirty()?.is_empty())
    }

    fn set_attribute(&mut self, column: &str, value: impl Into<Value>) -> &mut Self {
        self.state_mut().set_attribute(column, value);
        self
    }

    fn get_key(&self) -> Option<&Value> {
        self.get_attribute(Self::metadata().primary_key)
    }

    /// The storage format declared by this model's metadata.
    fn get_date_format(&self) -> &'static str {
        Self::metadata().date_format
    }

    fn get_key_name(&self) -> &'static str {
        Self::metadata().primary_key
    }

    /// The default name for foreign key columns pointing to this model.
    fn get_foreign_key(&self) -> String {
        let name = std::any::type_name::<Self>()
            .split('<')
            .next()
            .unwrap_or_default()
            .rsplit("::")
            .next()
            .unwrap_or_default();
        format!("{}_{}", Str::snake(name, "_"), self.get_key_name())
    }

    /// The key type used by schema helpers. Override for manually assigned string keys.
    fn get_key_type(&self) -> ModelKeyType {
        if self
            .unique_ids_for_instance()
            .contains(&self.get_key_name())
        {
            match Self::metadata().unique_id {
                Some(super::UniqueId::Uuid) => ModelKeyType::Uuid,
                Some(super::UniqueId::Ulid) => ModelKeyType::Ulid,
                None => ModelKeyType::Integer,
            }
        } else {
            ModelKeyType::Integer
        }
    }

    fn get_route_key_name() -> &'static str {
        Self::metadata().primary_key
    }

    fn get_route_key(&self) -> DatabaseResult<Option<Value>> {
        self.get_attribute_value(Self::get_route_key_name())
    }

    fn route_binding_value(field: &str, value: &str) -> Option<Value> {
        if Self::unique_ids().contains(&field) && !Self::is_valid_unique_id(value) {
            return None;
        }
        if field == Self::metadata().primary_key && Self::get_incrementing() {
            return value
                .parse::<i64>()
                .map(Value::Integer)
                .or_else(|_| value.parse::<u64>().map(Value::Unsigned))
                .ok();
        }
        Some(Value::from(value))
    }

    fn resolve_route_binding(
        binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        Box::pin(async move {
            let query = Self::query()?;
            route_binding::lookup(query, binding)
                .await
                .map_err(Into::into)
        })
    }

    fn resolve_child_route_binding(
        _binding: BindingContext<'_>,
    ) -> HttpFuture<'_, Result<Option<Self>, HttpError>> {
        Box::pin(async {
            Err(HttpError::from(DatabaseError::InvalidQuery(
                "scoped route binding relation",
            )))
        })
    }

    /// The configured creation timestamp column, or None when disabled.
    fn get_created_at_column(&self) -> Option<&str> {
        Self::metadata().created_at
    }

    fn get_table(&self) -> &'static str {
        self.state()
            .pivot_identity
            .map_or(Self::metadata().table, |identity| identity.table)
    }

    fn get_connection_name(&self) -> Option<&str> {
        self.state()
            .connection
            .as_ref()
            .map(Connection::get_name)
            .or(Self::metadata().connection)
    }

    /// Resolves a dynamic connection name without borrowing application configuration.
    fn resolve_connection_name(&self) -> DatabaseResult<Option<Cow<'_, str>>> {
        Ok(self.get_connection_name().map(Cow::Borrowed))
    }

    /// Retains the actual connection of a loaded or explicitly bound model.
    fn get_connection(&self) -> DatabaseResult<Connection> {
        match &self.state().connection {
            Some(connection) => Ok(connection.clone()),
            None => Db::connection(self.resolve_connection_name()?.as_deref()),
        }
    }

    /// Returns the primary key used by queue payloads.
    fn get_queueable_id(&self) -> Option<&Value> {
        self.get_key()
    }

    /// Returns loaded relation names and nested paths without running queries or accessors.
    fn get_queueable_relations(&self) -> Vec<String> {
        super::queueable::model_relations(self.state())
    }

    fn get_queueable_connection(&self) -> Option<&str> {
        self.get_connection_name()
    }

    /// Compares the primary key, table, and connection name of two models.
    fn is(&self, other: &impl Model) -> bool {
        self.get_key().unwrap_or(&Value::Null) == other.get_key().unwrap_or(&Value::Null)
            && self.get_table() == other.get_table()
            && self.get_connection_name() == other.get_connection_name()
    }

    fn is_not(&self, other: &impl Model) -> bool {
        !self.is(other)
    }

    /// Registers or replaces a named relation resolver in this application's container.
    fn resolve_relation_using<R: Relation<Parent = Self>>(
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
            .make::<super::RelationResolvers>()?
            .register::<Self, R>(name, resolver);
        Ok(())
    }

    /// Falls back to another model's named resolver when this model has no local resolver.
    ///
    /// The closures construct the base model for resolution and adapt its typed relation.
    fn inherit_relation_using<B, S, R>(
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
            .make::<super::RelationResolvers>()?
            .inherit::<Self, B, S, R>(name, base, adapt);
        Ok(())
    }

    /// Resolves a named relationship using the model's application.
    fn relation_resolver<R: Relation<Parent = Self>>(
        &self,
        name: &str,
    ) -> DatabaseResult<Option<R>> {
        super::RelationResolvers::resolve(self, name)
    }

    /// Loads a named dynamic relationship on first access.
    #[track_caller]
    fn get_dynamic_relation_or_load<R: Relation<Parent = Self>>(
        &mut self,
        name: &str,
    ) -> impl Future<Output = DatabaseResult<&R::Output>> + Send {
        with_query_caller(std::panic::Location::caller(), async move {
            let relation = super::DynamicRelation::<R>::new(name);
            super::detached_autoload::load_on_model_access(self, name, &relation).await?;
            self.get_relation(name)
                .ok_or(DatabaseError::InvalidQuery("relationship output type"))
        })
    }

    fn relation_loaded(&self, name: &str) -> bool {
        let state = self.state();
        state.relations.contains(name)
            || state
                .detached_autoload
                .as_ref()
                .is_some_and(|link| link.value(state, name).is_some())
    }

    fn get_relation<T: Send + Sync + 'static>(&self, name: &str) -> Option<&T> {
        let state = self.state();
        if let Some(shared) = state
            .detached_autoload
            .as_ref()
            .and_then(|link| link.value(state, name))
        {
            return shared.get(name);
        }
        state.relations.get(name)
    }

    /// Reads a relationship, querying once if it has not been loaded.
    #[track_caller]
    fn get_relation_or_load<'a, R: Relation<Parent = Self>>(
        &'a mut self,
        name: &'a str,
        relation: &'a R,
    ) -> impl Future<Output = DatabaseResult<&'a R::Output>> + Send + 'a {
        with_query_caller(std::panic::Location::caller(), async move {
            super::detached_autoload::load_on_model_access(self, name, relation).await?;
            self.get_relation(name)
                .ok_or(DatabaseError::InvalidQuery("relationship output type"))
        })
    }

    fn unset_relation(&mut self, name: &str) {
        let state = self.state_mut();
        state.relations.remove(name);
        if let Some(link) = &mut state.detached_autoload {
            link.invalidate(name);
        }
    }

    /// Eager loads registered paths onto this model.
    #[track_caller]
    fn load_paths(&mut self, paths: &[&str]) -> impl Future<Output = DatabaseResult<()>> + Send {
        with_query_caller(std::panic::Location::caller(), async move {
            let loads = super::NamedRelations::<Self>::prepare(paths)?;
            concerns::has_relationships::load_eager(loads, std::slice::from_mut(self)).await
        })
    }

    /// Loads registered relationship paths using the caller's explicit transaction.
    #[track_caller]
    fn load_paths_in_transaction<'a>(
        &'a mut self,
        paths: &'a [&'a str],
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(std::panic::Location::caller(), async move {
            let loads = super::NamedRelations::<Self>::prepare(paths)?;
            concerns::has_relationships::load_eager_with_transaction(
                loads,
                std::slice::from_mut(self),
                Some(transaction),
            )
            .await
        })
    }

    /// Loads missing relationships along the supplied registered paths.
    #[track_caller]
    fn load_missing_paths<'a>(
        &'a mut self,
        paths: &'a [&'a str],
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            super::NamedRelations::<Self>::load_missing(std::slice::from_mut(self), paths),
        )
    }

    /// Applies a grouped predicate when loading a missing terminal relationship.
    #[track_caller]
    fn load_missing_path_with(
        &mut self,
        path: &str,
        constraint: impl Fn(
            crate::database::query::Conditions,
            &str,
        ) -> crate::database::query::Conditions
        + Send
        + Sync
        + 'static,
    ) -> impl Future<Output = DatabaseResult<()>> + Send {
        self.load_missing_paths_with([super::MissingRelation::new(path).constrain(constraint)])
    }

    /// Loads several registered paths, each with its optional terminal predicate.
    #[track_caller]
    fn load_missing_paths_with<'a>(
        &mut self,
        paths: impl IntoIterator<Item = super::MissingRelation<'a>> + Send,
    ) -> impl Future<Output = DatabaseResult<()>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            super::NamedRelations::<Self>::load_missing_with(std::slice::from_mut(self), paths),
        )
    }

    #[track_caller]
    fn load<'a, R: Relation<Parent = Self>>(
        &'a mut self,
        name: &'a str,
        relation: &'a R,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            concerns::has_relationships::load(std::slice::from_mut(self), name, relation, false),
        )
    }

    /// Reloads a relationship on this model's cached polymorphic target.
    #[track_caller]
    fn load_morph<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        related_name: &'a str,
        relation: &'a impl Relation<Parent = T>,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            super::collection::load_morph_relation(
                std::slice::from_mut(self),
                name,
                morph,
                alias,
                related_name,
                relation,
            ),
        )
    }

    /// Reloads registered relationship paths on this model's cached polymorphic target.
    #[track_caller]
    fn load_morph_paths<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        paths: &'a [&'a str],
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            super::collection::load_morph_paths::<Self, T>(
                std::slice::from_mut(self),
                name,
                morph,
                alias,
                paths,
            ),
        )
    }

    /// Loads relation aggregates without replacing other model attributes.
    #[track_caller]
    fn load_aggregates<'a>(
        &'a mut self,
        aggregates: &'a [RelationAggregateQuery<'a, Self>],
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            super::collection::load_aggregates(std::slice::from_mut(self), aggregates),
        )
    }

    #[track_caller]
    fn load_aggregate<'a>(
        &'a mut self,
        name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = Self>,
        aggregate: RelationAggregate<'a>,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(std::panic::Location::caller(), async move {
            self.load_aggregates(&[RelationAggregateQuery::new(name, relation, aggregate)])
                .await
        })
    }

    #[track_caller]
    fn load_count<'a>(
        &'a mut self,
        name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = Self>,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_aggregate(name, relation, RelationAggregate::Count)
    }

    #[track_caller]
    fn load_exists<'a>(
        &'a mut self,
        name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = Self>,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_aggregate(name, relation, RelationAggregate::Exists)
    }

    #[track_caller]
    fn load_sum<'a>(
        &'a mut self,
        name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = Self>,
        column: &'a str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_aggregate(name, relation, RelationAggregate::Sum(column))
    }

    #[track_caller]
    fn load_avg<'a>(
        &'a mut self,
        name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = Self>,
        column: &'a str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_aggregate(name, relation, RelationAggregate::Avg(column))
    }

    #[track_caller]
    fn load_min<'a>(
        &'a mut self,
        name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = Self>,
        column: &'a str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_aggregate(name, relation, RelationAggregate::Min(column))
    }

    #[track_caller]
    fn load_max<'a>(
        &'a mut self,
        name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = Self>,
        column: &'a str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_aggregate(name, relation, RelationAggregate::Max(column))
    }

    /// Loads a count onto the cached polymorphic target of the selected type.
    #[track_caller]
    fn load_morph_count<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        related_name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = T>,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_morph_aggregate(
            name,
            morph,
            alias,
            RelationAggregateQuery::new(related_name, relation, RelationAggregate::Count),
        )
    }

    /// 为指定多态目标加载关系最大值，复用既有聚合查询与缓存回写。
    #[track_caller]
    fn load_morph_max<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        related_name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = T>,
        column: &'a str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_morph_aggregate(
            name,
            morph,
            alias,
            RelationAggregateQuery::new(related_name, relation, RelationAggregate::Max(column)),
        )
    }

    /// 为指定多态目标加载关系最小值，复用既有聚合查询与缓存回写。
    #[track_caller]
    fn load_morph_min<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        related_name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = T>,
        column: &'a str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_morph_aggregate(
            name,
            morph,
            alias,
            RelationAggregateQuery::new(related_name, relation, RelationAggregate::Min(column)),
        )
    }

    /// 为指定多态目标加载关系合计，复用既有聚合查询与缓存回写。
    #[track_caller]
    fn load_morph_sum<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        related_name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = T>,
        column: &'a str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_morph_aggregate(
            name,
            morph,
            alias,
            RelationAggregateQuery::new(related_name, relation, RelationAggregate::Sum(column)),
        )
    }

    /// 为指定多态目标加载关系平均值，复用既有聚合查询与缓存回写。
    #[track_caller]
    fn load_morph_avg<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        related_name: &'a str,
        relation: &'a impl ExistenceRelation<Parent = T>,
        column: &'a str,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        self.load_morph_aggregate(
            name,
            morph,
            alias,
            RelationAggregateQuery::new(related_name, relation, RelationAggregate::Avg(column)),
        )
    }

    /// Loads an aggregate onto this model's cached polymorphic target.
    #[track_caller]
    fn load_morph_aggregate<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        aggregate: super::RelationAggregateQuery<'a, T>,
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(std::panic::Location::caller(), async move {
            self.load_morph_aggregates(name, morph, alias, &[aggregate])
                .await
        })
    }

    /// Loads aggregates onto this model's cached polymorphic target of the selected type.
    #[track_caller]
    fn load_morph_aggregates<'a, T: Model>(
        &'a mut self,
        name: &'a str,
        morph: &'a super::relations::MorphTo<Self>,
        alias: &'a str,
        aggregates: &'a [super::RelationAggregateQuery<'a, T>],
    ) -> impl Future<Output = DatabaseResult<()>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            super::collection::load_morph_aggregates(
                std::slice::from_mut(self),
                name,
                morph,
                alias,
                aggregates,
            ),
        )
    }

    /// Registers a listener on this application's model event registry.
    fn register_model_event(
        app: &crate::foundation::Application,
        event: super::concerns::ModelEvent,
        listener: impl super::concerns::ModelEventListener<Self> + 'static,
    ) -> DatabaseResult<()> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        app.container()
            .make::<super::ModelEvents>()?
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
            .make::<super::ModelEvents>()?
            .listen_to::<Self>(O::EVENTS, super::observer::ObserverListener(observer));
        Ok(())
    }

    /// Registers an observer whose non-vetoing callbacks wait for the transaction to commit.
    ///
    /// A deferred callback receives a clone of the model taken when the event fires.
    fn observe_after_commit<O>(
        app: &crate::foundation::Application,
        observer: O,
    ) -> DatabaseResult<()>
    where
        Self: Clone,
        O: super::Observer<Self> + crate::contracts::events::ShouldHandleEventsAfterCommit,
    {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        app.container()
            .make::<super::ModelEvents>()?
            .listen_to::<Self>(
                O::EVENTS,
                super::observer::AfterCommitObserver::new(observer),
            );
        Ok(())
    }

    /// Suppresses model events in the current async call chain.
    fn without_events<F: Future + Send>(work: F) -> impl Future<Output = F::Output> + Send {
        concerns::has_events::without_events(work)
    }

    /// Saves this model and its already loaded relationship graph.
    #[track_caller]
    fn push(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(std::panic::Location::caller(), async move {
            super::relations::PushContext::default().push(self).await
        })
    }

    #[track_caller]
    fn push_quietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::without_events(self.push())
    }

    /// Replaces a cached relation with a typed output without querying it.
    fn set_relation<R: Relation<Parent = Self>>(&mut self, name: &str, output: R::Output) {
        concerns::has_relationships::update_association_cache::<R>(self, Some(name), || {
            Some(output)
        });
    }

    #[track_caller]
    fn save_quietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::without_events(self.save())
    }

    #[track_caller]
    fn delete_quietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::without_events(self.delete())
    }

    #[track_caller]
    fn force_delete_quietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::without_events(self.force_delete())
    }

    #[track_caller]
    fn restore_quietly(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        Self::without_events(self.restore())
    }

    #[track_caller]
    fn increment(
        &mut self,
        column: &str,
        amount: impl Into<Value>,
        extra: Attributes,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send {
        self.increment_each(Attributes::from([(column.into(), amount.into())]), extra)
    }

    #[track_caller]
    fn increment_each(
        &mut self,
        columns: Attributes,
        extra: Attributes,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::change_counters(self, columns, extra, false),
        )
    }

    #[track_caller]
    fn decrement(
        &mut self,
        column: &str,
        amount: impl Into<Value>,
        extra: Attributes,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send {
        self.decrement_each(Attributes::from([(column.into(), amount.into())]), extra)
    }

    #[track_caller]
    fn decrement_each(
        &mut self,
        columns: Attributes,
        extra: Attributes,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::change_counters(self, columns, extra, true),
        )
    }

    #[track_caller]
    fn increment_quietly(
        &mut self,
        column: &str,
        amount: impl Into<Value>,
        extra: Attributes,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send {
        Self::without_events(self.increment(column, amount, extra))
    }

    #[track_caller]
    fn increment_each_quietly(
        &mut self,
        columns: Attributes,
        extra: Attributes,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send {
        Self::without_events(self.increment_each(columns, extra))
    }

    #[track_caller]
    fn decrement_quietly(
        &mut self,
        column: &str,
        amount: impl Into<Value>,
        extra: Attributes,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send {
        Self::without_events(self.decrement(column, amount, extra))
    }

    #[track_caller]
    fn decrement_each_quietly(
        &mut self,
        columns: Attributes,
        extra: Attributes,
    ) -> impl Future<Output = DatabaseResult<u64>> + Send {
        Self::without_events(self.decrement_each(columns, extra))
    }

    #[track_caller]
    fn save(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::save(self, None),
        )
    }

    #[track_caller]
    fn update(
        &mut self,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::update(self, attributes, None),
        )
    }

    /// 在事务内保存；事件取消返回 false，错误或取消时回滚数据库写入。
    #[track_caller]
    fn save_or_fail(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::save_or_fail(self),
        )
    }

    #[track_caller]
    fn update_or_fail(
        &mut self,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(std::panic::Location::caller(), async move {
            if !self.state().exists {
                return Ok(false);
            }
            self.fill_async(attributes).await?;
            self.save_or_fail().await
        })
    }

    #[track_caller]
    fn update_quietly(
        &mut self,
        attributes: Attributes,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(std::panic::Location::caller(), async move {
            if !self.state().exists {
                return Ok(false);
            }
            self.fill_async(attributes).await?;
            self.save_quietly().await
        })
    }

    #[track_caller]
    fn delete_or_fail(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::delete_or_fail(self),
        )
    }

    /// 插入新模型并忽略指定唯一约束冲突，支持范围沿用驱动的 RETURNING 能力。
    #[track_caller]
    fn save_or_ignore<'a>(
        &'a mut self,
        unique_by: Option<&'a [&'a str]>,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::save_or_ignore(self, unique_by),
        )
    }

    #[track_caller]
    fn delete(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::delete(self, false, None),
        )
    }

    #[track_caller]
    fn force_delete(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::delete(self, true, None),
        )
    }

    #[track_caller]
    fn restore(&mut self) -> impl Future<Output = DatabaseResult<bool>> + Send {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::restore(self, None),
        )
    }

    #[track_caller]
    fn save_in_transaction<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::save(self, Some(transaction)),
        )
    }

    #[track_caller]
    fn update_in_transaction<'a>(
        &'a mut self,
        attributes: Attributes,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::update(self, attributes, Some(transaction)),
        )
    }

    #[track_caller]
    fn delete_in_transaction<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::delete(self, false, Some(transaction)),
        )
    }

    #[track_caller]
    fn force_delete_in_transaction<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::delete(self, true, Some(transaction)),
        )
    }

    #[track_caller]
    fn restore_in_transaction<'a>(
        &'a mut self,
        transaction: &'a mut crate::database::Transaction,
    ) -> impl Future<Output = DatabaseResult<bool>> + Send + 'a {
        with_query_caller(
            std::panic::Location::caller(),
            persistence::restore(self, Some(transaction)),
        )
    }

    fn trashed(&self) -> bool {
        Self::metadata()
            .deleted_at
            .is_some_and(|column| soft_deletes::is_trashed(self.get_attribute(column)))
    }
}
