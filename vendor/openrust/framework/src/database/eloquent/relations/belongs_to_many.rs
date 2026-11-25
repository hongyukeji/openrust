use futures_util::TryStreamExt as _;
use std::{any::TypeId, collections::HashMap, sync::Arc};

use indexmap::IndexMap;

use crate::database::{
    Connection, DatabaseCursor, DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value,
    eloquent::{
        Attributes, Builder, Model, ModelState,
        casts::cast_pivot_key,
        concerns::{
            has_relationships::{LoadedRelations, load_eager_with_transaction},
            has_timestamps::timestamp,
            queries_relationships::cast_existence,
        },
        touches::TouchedOwners,
    },
    events::with_query_caller,
    query,
};

use crate::pagination::{
    CursorOptions, CursorPaginator, LengthAwarePaginator, PageOptions, Paginator,
};

use super::super::persistence::is_unique_violation;

use super::{
    ExistenceRelation, Pivot, Relation,
    custom_pivot::{CustomPivot, PivotIdentity},
    pivot_constraints::{PivotConstraints, pivot_predicates},
    relation::{ParentKeys, RelationKeys, dictionary_key},
};

/// Links two models through an intermediate table.
pub struct BelongsToMany<P: Model, R: Model> {
    keys: RelationKeys<P, R>,
    pivot: PivotKeys,
    pivot_columns: Vec<&'static str>,
    pivot_accessor: &'static str,
    pivot_constraints: PivotConstraints,
    pivot_orders: Vec<(String, String)>,
    pivot_timestamps: Option<PivotTimestampColumns>,
    custom_pivot: Option<CustomPivot>,
    pivot_inverses: (Option<String>, Option<String>),
    touch_parent: bool,
    touch_related: bool,
}

/// Attributes supplied directly or computed when a relation needs to write a model.
pub struct RelatedValues<'a>(RelatedValueSource<'a>);

enum RelatedValueSource<'a> {
    Attributes(Attributes),
    Deferred(Box<dyn FnMut() -> Attributes + Send + 'a>),
}

impl<'a> RelatedValues<'a> {
    pub fn deferred(resolve: impl FnMut() -> Attributes + Send + 'a) -> Self {
        Self(RelatedValueSource::Deferred(Box::new(resolve)))
    }

    pub(super) fn resolve(&mut self) -> Attributes {
        match &mut self.0 {
            RelatedValueSource::Attributes(attributes) => attributes.clone(),
            RelatedValueSource::Deferred(resolve) => resolve(),
        }
    }
}

impl From<Attributes> for RelatedValues<'_> {
    fn from(attributes: Attributes) -> Self {
        Self(RelatedValueSource::Attributes(attributes))
    }
}

#[derive(Clone, Copy)]
struct RelatedLookup<'a> {
    selection: super::lookup::LookupSelection<'a>,
    use_writer: bool,
    include_trashed: bool,
}

/// Keys attached, detached, or updated by a pivot synchronization.
#[derive(Debug, PartialEq, Default)]
pub struct SyncChanges {
    pub attached: Vec<Value>,
    pub detached: Vec<Value>,
    pub updated: Vec<Value>,
}

/// Keys inserted or removed when toggling intermediate links.
#[derive(Debug, PartialEq, Default)]
pub struct ToggleChanges {
    pub attached: Vec<Value>,
    pub detached: Vec<Value>,
}

#[derive(Clone, Copy, PartialEq)]
enum AttachmentMode {
    Default,
    Transactional,
    Required,
}

#[derive(Clone, Copy)]
enum PivotOperation {
    Sync { detach: bool },
    Toggle,
}

impl PivotOperation {
    fn removes(self, requested: bool) -> bool {
        match self {
            Self::Sync { detach } => detach && !requested,
            Self::Toggle => requested,
        }
    }
}

impl<P: Model, R: Model> BelongsToMany<P, R> {
    pub const fn new(
        table: &'static str,
        foreign_pivot_key: &'static str,
        related_pivot_key: &'static str,
        parent_key: &'static str,
        related_key: &'static str,
    ) -> Self {
        Self {
            keys: RelationKeys::new(parent_key, related_key),
            pivot: PivotKeys {
                table,
                foreign: foreign_pivot_key,
                related: related_pivot_key,
                morph: None,
            },
            pivot_columns: Vec::new(),
            pivot_accessor: "pivot",
            pivot_constraints: PivotConstraints::new(),
            pivot_orders: Vec::new(),
            pivot_timestamps: None,
            custom_pivot: None,
            pivot_inverses: (None, None),
            touch_parent: false,
            touch_related: false,
        }
    }

    pub(super) const fn morph(mut self, column: &'static str, alias: &'static str) -> Self {
        self.pivot.morph = Some((column, alias));
        self
    }

    /// Applies constraints and nested eager loads to the related query.
    #[must_use]
    pub fn constrain(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.keys.constrain(configure);
        self
    }

    #[must_use]
    pub fn with_pivot(mut self, columns: impl IntoIterator<Item = &'static str>) -> Self {
        for column in columns {
            if !matches!(column, "*")
                && !self.pivot_columns.contains(&column)
                && column != self.pivot.foreign
                && column != self.pivot.related
                && self.pivot.morph.is_none_or(|(kind, _)| column != kind)
            {
                self.pivot_columns.push(column);
            }
        }
        self
    }

    #[must_use]
    pub fn r#as(mut self, name: &'static str) -> Self {
        self.pivot_accessor = name;
        self
    }

    /// Orders related rows by a column of the intermediate table.
    #[must_use]
    pub fn order_by_pivot(mut self, column: &str, direction: &str) -> Self {
        let column = if column.contains('.') {
            column.to_owned()
        } else {
            format!("{}.{column}", self.pivot.table)
        };
        self.pivot_orders.push((column, direction.into()));
        self
    }

    #[must_use]
    pub fn order_by_pivot_desc(self, column: &str) -> Self {
        self.order_by_pivot(column, "desc")
    }

    /// Selects and maintains the parent's conventional timestamp columns on the pivot.
    #[must_use]
    pub fn with_timestamps(self) -> Self {
        let (created, updated) = Self::default_timestamp_columns();
        self.set_pivot_timestamp_columns((Some(created), Some(updated)))
    }

    /// Sets explicit timestamp columns. A `None` skips that column.
    pub fn with_timestamp_columns(
        self,
        created: Option<&'static str>,
        updated: Option<&'static str>,
    ) -> DatabaseResult<Self> {
        let columns = Attributes::from_iter(
            created
                .into_iter()
                .chain(updated)
                .map(|column| (column.into(), Value::Null)),
        );
        if columns
            .keys()
            .any(|name| name.is_empty() || name.contains('.'))
            || matches!((created, updated), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b))
        {
            return Err(DatabaseError::InvalidQuery(
                "invalid pivot timestamp columns",
            ));
        }
        self.validate_pivot_attributes(&columns)?;
        Ok(self.set_pivot_timestamp_columns((created, updated)))
    }

    fn set_pivot_timestamp_columns(mut self, columns: PivotTimestampColumns) -> Self {
        self.pivot_timestamps = Some(columns);
        self
    }

    fn default_timestamp_columns() -> (&'static str, &'static str) {
        let metadata = P::metadata();
        (
            metadata.created_at.unwrap_or("created_at"),
            metadata.updated_at.unwrap_or("updated_at"),
        )
    }

    fn timestamp_columns(&self) -> PivotTimestampColumns {
        self.pivot_timestamps.unwrap_or_else(|| {
            let (created, updated) = Self::default_timestamp_columns();
            (
                self.pivot_columns.contains(&created).then_some(created),
                self.pivot_columns.contains(&updated).then_some(updated),
            )
        })
    }

    fn pivot_write_timestamps(
        &self,
        connection: &Connection,
    ) -> DatabaseResult<PivotWriteTimestamps> {
        let (created, updated) = self.timestamp_columns();
        let mut insert = Attributes::new();
        let mut update = Attributes::new();
        if created.is_some() || updated.is_some() {
            let now = timestamp(connection, P::metadata().date_format)?;
            if let Some(column) = created {
                insert.insert(column.into(), now.clone());
            }
            if let Some(column) = updated {
                insert.insert(column.into(), now.clone());
                update.insert(column.into(), now);
            }
            self.validate_pivot_attributes(&insert)?;
        }
        Ok(PivotWriteTimestamps { insert, update })
    }

    /// Declares this relationship's table and keys for custom pivot queue restoration.
    pub fn queueable_pivot(&self) -> super::super::QueueablePivot {
        super::super::QueueablePivot(self.pivot.identity(self.timestamp_columns()))
    }

    /// Hydrates and persists links using the declared model.
    #[must_use]
    pub fn using<M: Model>(mut self) -> Self {
        self.custom_pivot = Some(CustomPivot::new::<M>());
        self.pivot_inverses = (None, None);
        self
    }

    /// Hydrates declared inverse relations from the models already participating in this load.
    pub fn chaperone(self) -> Self {
        self.chaperone_with(None, None)
            .expect("inferred names cannot fail validation")
    }

    /// Uses explicit inverse names for either endpoint and infers omitted names.
    pub fn chaperone_with(
        mut self,
        declaring: Option<&str>,
        related: Option<&str>,
    ) -> DatabaseResult<Self> {
        if let Some(custom) = self.custom_pivot {
            self.pivot_inverses = (
                pivot_inverse::<P>(custom, declaring, self.pivot.foreign)?,
                pivot_inverse::<R>(custom, related, self.pivot.related)?,
            );
        }
        Ok(self)
    }

    #[must_use]
    pub fn without_chaperone(mut self) -> Self {
        self.pivot_inverses = (None, None);
        self
    }

    /// Refreshes the declaring model's timestamp when its links change.
    #[must_use]
    pub fn with_touching_parent(mut self) -> Self {
        self.touch_parent = true;
        self
    }

    /// Refreshes the timestamps of models still linked after a pivot change.
    #[must_use]
    pub fn with_touching_related(mut self) -> Self {
        self.touch_related = true;
        self
    }

    query_callers! {
    /// Refreshes the timestamps of the models linked through this relation.
    pub async fn touch(&self, parent: &P) -> DatabaseResult<()> {
        if R::is_ignoring_touch() {
            return Ok(());
        }
        let connection = self.keys.connection(parent)?;
        if let Some(touch) = self.related_touch(self.owner(parent)?) {
            let query = touch.query::<R>(&connection)?;
            query.update(touch.attributes::<R>(&connection)?).await?;
        }
        Ok(())
    }
    }

    pub(in crate::database::eloquent) async fn touch_owners(
        &self,
        parent: &P,
        attributes: &Attributes,
        visited: &mut TouchedOwners,
        mut transaction: Option<&mut Transaction>,
    ) -> DatabaseResult<()> {
        let Some(owner) = attributes
            .get(self.keys.parent)
            .filter(|value| **value != Value::Null)
        else {
            return Ok(());
        };
        let connection = self.keys.connection(parent)?;
        let mut linked = self
            .pivot_constraints
            .apply(
                self.pivot
                    .for_owner(connection.table(self.pivot.table), owner),
            )
            .select([self.pivot.related]);
        if let Some(touch) = self.related_touch(owner) {
            match transaction.as_deref_mut() {
                Some(tx) => touch.run::<R>(&connection, tx).await?,
                None => {
                    touch
                        .query::<R>(&connection)?
                        .update(touch.attributes::<R>(&connection)?)
                        .await?;
                }
            }
        }

        let touches = R::touches();
        if touches.is_empty() {
            return Ok(());
        }
        let relation = self.keys.configure(R::query_on(&connection))?;
        let casts = relation.casts.clone();
        let query = relation
            .scoped()?
            .0
            .where_in_sub(self.keys.related, &mut linked)?
            .use_write_pdo();
        let rows = match transaction.as_deref_mut() {
            Some(tx) => query.in_transaction(tx)?.get().await?,
            None => query.get().await?,
        };
        for row in rows {
            let attributes: Attributes = row.into_columns().into_iter().collect();
            let key = attributes
                .get(R::metadata().primary_key)
                .filter(|value| **value != Value::Null)
                .ok_or(DatabaseError::InvalidQuery("touch owner primary key"))?;
            if visited.visit::<R>(&connection, Some(key))? {
                let model = R::from_state(ModelState::hydrated_attributes(
                    attributes.clone(),
                    connection.clone(),
                    casts.clone(),
                ));
                touches
                    .run(&model, &attributes, visited, transaction.as_deref_mut())
                    .await?;
            }
        }
        Ok(())
    }

    pub(in crate::database::eloquent) fn touch_identity(&self) -> PivotTouchIdentity<'_> {
        PivotTouchIdentity {
            related_type: TypeId::of::<R>(),
            pivot: self.pivot,
            parent_key: self.keys.parent,
            related_key: self.keys.related,
            constraints: &self.pivot_constraints,
        }
    }

    pub(in crate::database::eloquent) fn touches_related(
        &self,
        parent: &P,
    ) -> DatabaseResult<bool> {
        self.touches_related_on(&self.keys.connection(parent)?)
    }

    fn touches_related_on(&self, connection: &Connection) -> DatabaseResult<bool> {
        Ok(!R::is_ignoring_touch()
            && (self.touch_related
                || P::touches().contains_pivot(&self.touch_identity(), connection)?))
    }

    fn pivot_touches(
        &self,
        parent: &P,
        owner: &Value,
        connection: &Connection,
    ) -> DatabaseResult<(Option<Arc<ModelState>>, Option<RelatedTouch>)> {
        let identity = self.touch_identity();
        let parent_state = if P::metadata().updated_at.is_some()
            && (self.touch_parent
                || R::touches().contains_pivot(&identity.inverse::<P>(), connection)?)
        {
            Some(self.touch_state(parent, connection)?)
        } else {
            None
        };
        let related_touch = if self.touches_related_on(connection)? {
            self.related_touch(owner)
        } else {
            None
        };
        Ok((parent_state, related_touch))
    }

    fn related_touch(&self, owner: &Value) -> Option<RelatedTouch> {
        if R::is_ignoring_touch() {
            return None;
        }
        Some(RelatedTouch {
            pivot: self.pivot,
            constraints: self.pivot_constraints.clone(),
            owner: owner.clone(),
            related_key: self.keys.related,
            column: R::metadata().updated_at?,
        })
    }

    pivot_predicates!(apply_pivot_predicate, apply_pivot_group);

    pub(super) fn apply_pivot_predicate(
        mut self,
        column: &str,
        predicate: impl FnOnce(query::Conditions, &str) -> query::Conditions,
    ) -> Self {
        self.pivot_constraints
            .add(self.pivot.table, column, predicate);
        self
    }

    pub(super) fn apply_pivot_group(
        mut self,
        or: bool,
        configure: impl FnOnce(query::Conditions) -> query::Conditions,
    ) -> Self {
        self.pivot_constraints
            .add_group(self.pivot.table, or, configure);
        self
    }

    /// Filters the relation and supplies the value when inserting new links.
    pub fn with_pivot_value(
        mut self,
        column: &str,
        value: impl Into<Value>,
    ) -> DatabaseResult<Self> {
        let value = value.into();
        if matches!(value, Value::Null) || column.is_empty() || column.contains('.') {
            return Err(DatabaseError::InvalidQuery(
                "pivot default requires a column and a non-null value",
            ));
        }
        self.validate_pivot_attributes(&Attributes::from([(column.into(), value.clone())]))?;
        self.pivot_constraints
            .defaults
            .insert(column.into(), value.clone());
        Ok(self.where_pivot(column, "=", value))
    }

    query_callers! {
    /// Writes the related table without attaching or updating pivot rows.
    pub async fn upsert(
        &self,
        parent: &P,
        rows: Vec<Attributes>,
        unique_by: &[&str],
        update: Option<&[&str]>,
    ) -> DatabaseResult<u64> {
        if rows.is_empty() {
            return Ok(0);
        }
        let connection = self.keys.connection(parent)?;
        self.keys
            .configure(R::query_on(&connection))?
            .upsert(rows, unique_by, update)
            .await
    }

    /// Finds an attached primary key or returns an unsaved model.
    pub async fn find_or_new(&self, parent: &P, key: impl Into<Value> + Send) -> DatabaseResult<R> {
        match self.find(parent, key).await? {
            Some(model) => Ok(model),
            None => R::new_instance_on(&self.keys.connection(parent)?, Attributes::new()),
        }
    }
    }

    super::lookup::relation_lookup!();

    async fn lookup_models(
        &self,
        parent: &P,
        keys: &Attributes,
        limit: u64,
    ) -> DatabaseResult<Vec<R>> {
        let lookup = RelatedLookup {
            selection: super::lookup::LookupSelection::limited(keys, limit),
            use_writer: false,
            include_trashed: false,
        };
        let mut groups = self.match_many(&[parent], None, Some(lookup)).await?;
        Ok(groups
            .pop()
            .unwrap_or_default()
            .into_iter()
            .map(owned_model)
            .collect())
    }

    async fn lookup_many(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> DatabaseResult<crate::database::eloquent::Collection<R>> {
        let lookup = RelatedLookup {
            selection: super::lookup::LookupSelection::Keys(keys),
            use_writer: false,
            include_trashed: false,
        };
        let mut groups = self.match_many(&[parent], None, Some(lookup)).await?;
        let models = groups
            .pop()
            .unwrap_or_default()
            .into_iter()
            .map(owned_model)
            .collect();
        Ok(R::new_collection(models))
    }

    query_callers! {
    /// Finds an attached model, attaches an existing match, or creates a new model.
    pub async fn first_or_create<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
        joining: Attributes,
    ) -> DatabaseResult<Arc<R>> {
        self.first_or_create_with_trashed_option(
            parent,
            attributes,
            &mut values.into(),
            joining,
            false,
        )
        .await
    }

    /// Fills and saves an existing related model, or creates and attaches a new one.
    pub async fn update_or_create<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
        joining: Attributes,
    ) -> DatabaseResult<Arc<R>> {
        let mut values = values.into();
        let model = self
            .first_or_create_with_trashed_option(parent, attributes, &mut values, joining, false)
            .await?;
        if model.state().was_recently_created() {
            return Ok(model);
        }

        let mut model = owned_model(model);
        model.fill_async(values.resolve()).await?;
        super::super::persistence::save_with_touch(&mut model, None, false).await?;
        Ok(Arc::new(model))
    }

    /// Creates a related model with an initial counter or atomically increments an existing one.
    pub async fn increment_or_create(
        &self,
        parent: &P,
        attributes: Attributes,
        column: &str,
        default: impl Into<Value>,
        step: impl Into<Value>,
        mut extra: Attributes,
    ) -> DatabaseResult<Arc<R>> {
        let default = default.into();
        let step = step.into();
        super::super::persistence::validate_counter::<R>(column, &default, &step, &extra)?;
        let mut values = extra.clone();
        values.insert(column.into(), default);
        extra.remove(column);
        let model = self
            .first_or_create(parent, attributes, values, Attributes::new())
            .await?;
        if model.state().was_recently_created() {
            return Ok(model);
        }
        super::super::persistence::increment_related(owned_model(model), column, step, extra)
            .await
            .map(Arc::new)
    }

    /// Finds or creates a related model and restores it through the model lifecycle.
    pub async fn restore_or_create<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
        joining: Attributes,
    ) -> DatabaseResult<Arc<R>> {
        self.require_soft_deletes()?;
        let model = self
            .first_or_create_with_trashed_option(
                parent,
                attributes,
                &mut values.into(),
                joining,
                true,
            )
            .await?;
        restore_related(model).await
    }

    /// Attempts insertion first, recovering and restoring a matching soft-deleted model.
    pub async fn create_or_restore<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
        joining: Attributes,
    ) -> DatabaseResult<Arc<R>> {
        self.require_soft_deletes()?;
        let model = self
            .create_or_first_with_trashed_option(
                parent,
                attributes,
                &mut values.into(),
                joining,
                true,
            )
            .await?;
        restore_related(model).await
    }
    }

    fn require_soft_deletes(&self) -> DatabaseResult<()> {
        R::metadata()
            .deleted_at
            .map(|_| ())
            .ok_or_else(super::super::soft_deletes::unsupported)
    }

    async fn first_or_create_with_trashed_option(
        &self,
        parent: &P,
        attributes: Attributes,
        values: &mut RelatedValues<'_>,
        joining: Attributes,
        include_trashed: bool,
    ) -> DatabaseResult<Arc<R>> {
        self.owner(parent)?;
        let lookup = RelatedLookup {
            selection: super::lookup::LookupSelection::limited(&attributes, 1),
            use_writer: false,
            include_trashed,
        };
        if let Some(model) = self.find_attached(parent, lookup).await? {
            return Ok(model);
        }
        let connection = self.keys.connection(parent)?;
        if let Some(model) = self.find_candidate(&connection, lookup).await? {
            self.attach_found(parent, model, lookup, joining).await
        } else {
            self.create_or_first_with_trashed_option(
                parent,
                attributes,
                values,
                joining,
                include_trashed,
            )
            .await
        }
    }

    async fn find_candidate(
        &self,
        connection: &Connection,
        lookup: RelatedLookup<'_>,
    ) -> DatabaseResult<Option<R>> {
        let query = lookup.selection.apply(
            self.keys
                .configure(R::query_on(connection))?
                .unconstrained_relation_query(),
        );
        let query = if lookup.include_trashed {
            query.with_trashed()
        } else {
            query
        };
        let query = if lookup.use_writer {
            query.use_write_pdo()
        } else {
            query
        };
        query.first().await
    }

    async fn find_attached(
        &self,
        parent: &P,
        lookup: RelatedLookup<'_>,
    ) -> DatabaseResult<Option<Arc<R>>> {
        Ok(self
            .match_many(&[parent], None, Some(lookup))
            .await?
            .pop()
            .and_then(|models| models.into_iter().next()))
    }

    query_callers! {
    /// Creates and attaches a model, or attaches a matching model after a unique conflict.
    pub async fn create_or_first<'a>(
        &self,
        parent: &P,
        attributes: Attributes,
        values: impl Into<RelatedValues<'a>> + Send,
        joining: Attributes,
    ) -> DatabaseResult<Arc<R>> {
        self.create_or_first_with_trashed_option(
            parent,
            attributes,
            &mut values.into(),
            joining,
            false,
        )
        .await
    }
    }

    async fn create_or_first_with_trashed_option(
        &self,
        parent: &P,
        attributes: Attributes,
        values: &mut RelatedValues<'_>,
        joining: Attributes,
        include_trashed: bool,
    ) -> DatabaseResult<Arc<R>> {
        let connection = self.keys.connection(parent)?;
        let mut insert = attributes.clone();
        insert.extend(values.resolve());
        let attempt = self
            .create(parent, insert, joining.clone())
            .await
            .map(Arc::new);
        let conflict = match attempt {
            Err(error) if is_unique_violation(&error) => error,
            result => return result,
        };

        let lookup = RelatedLookup {
            selection: super::lookup::LookupSelection::limited(&attributes, 1),
            use_writer: true,
            include_trashed,
        };
        let Some(model) = self.find_candidate(&connection, lookup).await? else {
            return Err(conflict);
        };
        self.attach_found(parent, model, lookup, joining).await
    }

    query_callers! {
    /// Saves the related model and attaches it in the same transaction.
    pub async fn save(&self, parent: &P, model: &mut R, joining: Attributes) -> DatabaseResult<()> {
        self.save_and_attach(parent, model, joining, true).await
    }

    pub async fn save_quietly(
        &self,
        parent: &P,
        model: &mut R,
        joining: Attributes,
    ) -> DatabaseResult<()> {
        R::without_events(self.save(parent, model, joining)).await
    }

    pub async fn save_many(
        &self,
        parent: &P,
        models: &mut [R],
        joinings: &[Attributes],
    ) -> DatabaseResult<()> {
        for (index, model) in models.iter_mut().enumerate() {
            self.save_and_attach(
                parent,
                model,
                joinings.get(index).cloned().unwrap_or_default(),
                false,
            )
            .await?;
        }
        self.touch_after_batch(parent).await
    }

    pub async fn save_many_quietly(
        &self,
        parent: &P,
        models: &mut [R],
        joinings: &[Attributes],
    ) -> DatabaseResult<()> {
        R::without_events(self.save_many(parent, models, joinings)).await
    }

    pub async fn create(
        &self,
        parent: &P,
        attributes: Attributes,
        joining: Attributes,
    ) -> DatabaseResult<R> {
        self.create_and_attach(parent, attributes, joining, true)
            .await
    }

    pub async fn create_quietly(
        &self,
        parent: &P,
        attributes: Attributes,
        joining: Attributes,
    ) -> DatabaseResult<R> {
        R::without_events(self.create(parent, attributes, joining)).await
    }

    pub async fn create_many(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
        joinings: &[Attributes],
    ) -> DatabaseResult<Vec<R>> {
        let mut models = Vec::new();
        for (index, attributes) in records.into_iter().enumerate() {
            models.push(
                self.create_and_attach(
                    parent,
                    attributes,
                    joinings.get(index).cloned().unwrap_or_default(),
                    false,
                )
                .await?,
            );
        }
        self.touch_after_batch(parent).await?;
        Ok(models)
    }

    pub async fn create_many_quietly(
        &self,
        parent: &P,
        records: impl IntoIterator<Item = Attributes, IntoIter: Send> + Send,
        joinings: &[Attributes],
    ) -> DatabaseResult<Vec<R>> {
        R::without_events(self.create_many(parent, records, joinings)).await
    }

    pub async fn force_create(
        &self,
        parent: &P,
        attributes: Attributes,
        joining: Attributes,
    ) -> DatabaseResult<R> {
        self.owner(parent)?;
        self.validate_pivot_attributes(&joining)?;
        let connection = self.keys.connection(parent)?;
        let query = self.keys.configure(R::query_on(&connection))?;
        let mut model = R::new_instance_on(&connection, Attributes::new())?;
        for (column, value) in query.creation_attributes(attributes) {
            model.set_attribute_value_async(&column, value).await?;
        }
        self.save(parent, &mut model, joining).await?;
        Ok(model)
    }

    pub async fn force_create_quietly(
        &self,
        parent: &P,
        attributes: Attributes,
        joining: Attributes,
    ) -> DatabaseResult<R> {
        R::without_events(self.force_create(parent, attributes, joining)).await
    }
    }

    async fn create_and_attach(
        &self,
        parent: &P,
        attributes: Attributes,
        joining: Attributes,
        touch: bool,
    ) -> DatabaseResult<R> {
        self.owner(parent)?;
        self.validate_pivot_attributes(&joining)?;
        let connection = self.keys.connection(parent)?;
        let mut model = self
            .keys
            .configure(R::query_on(&connection))?
            .make_async(attributes)
            .await?;
        self.save_and_attach(parent, &mut model, joining, touch)
            .await?;
        Ok(model)
    }

    async fn save_and_attach(
        &self,
        parent: &P,
        model: &mut R,
        joining: Attributes,
        touch: bool,
    ) -> DatabaseResult<()> {
        self.validate_pivot_attributes(&joining)?;
        let owner = self.owner(parent)?.clone();
        let connection = self.keys.connection(parent)?;
        let (parent_state, related_touch) = if touch {
            self.pivot_touches(parent, &owner, &connection)?
        } else {
            (None, None)
        };
        let pivot = self.pivot;
        let identity = pivot.identity(self.timestamp_columns());
        let custom = self.custom_pivot;
        let related_key = self.keys.related;
        let mut pivot_attributes = self.pivot_write_timestamps(&connection)?.insert;
        pivot_attributes.extend(self.pivot_constraints.defaults.clone());
        pivot_attributes.extend(joining);
        let initial = model.state();
        let saved = connection
            .transaction(
                |transaction| {
                    let mut model = R::from_state(initial.clone());
                    let connection = connection.clone();
                    let owner = owner.clone();
                    let pivot_attributes = pivot_attributes.clone();
                    let parent_state = parent_state.clone();
                    let related_touch = related_touch.clone();
                    Box::pin(async move {
                        if !super::super::persistence::save_with_touch(
                            &mut model,
                            Some(transaction),
                            false,
                        )
                        .await?
                        {
                            return Err(DatabaseError::InvalidQuery(
                                "related model creation cancelled",
                            ));
                        }
                        let key = attachment_key(&model, related_key)?;
                        let mut rows = pivot.records(&owner, std::iter::once(key));
                        rows[0].extend(pivot_attributes);
                        if !insert_pivot_rows(transaction, &connection, identity, custom, &rows)
                            .await?
                        {
                            return Err(DatabaseError::InvalidQuery("pivot attachment cancelled"));
                        }
                        touch_endpoints::<P, R>(
                            parent_state,
                            related_touch,
                            &connection,
                            transaction,
                        )
                        .await?;
                        Ok(model.state().clone())
                    })
                },
                1,
            )
            .await?;
        *model.state_mut() = saved;
        Ok(())
    }

    async fn touch_after_batch(&self, parent: &P) -> DatabaseResult<()> {
        let connection = self.keys.connection(parent)?;
        let (state, related) = self.pivot_touches(parent, self.owner(parent)?, &connection)?;
        if state.is_none() && related.is_none() {
            return Ok(());
        }
        connection
            .transaction(
                |transaction| {
                    let state = state.clone();
                    let related = related.clone();
                    let connection = connection.clone();
                    Box::pin(async move {
                        touch_endpoints::<P, R>(state, related, &connection, transaction).await
                    })
                },
                1,
            )
            .await
    }

    async fn attach_found(
        &self,
        parent: &P,
        model: R,
        lookup: RelatedLookup<'_>,
        joining: Attributes,
    ) -> DatabaseResult<Arc<R>> {
        let key = attachment_key(&model, self.keys.related)?;
        match self
            .attach_records(
                parent,
                std::slice::from_ref(key),
                joining,
                AttachmentMode::Required,
            )
            .await
        {
            Ok(()) => Ok(Arc::new(model)),
            Err(error) if is_unique_violation(&error) => self
                .find_attached(
                    parent,
                    RelatedLookup {
                        use_writer: true,
                        ..lookup
                    },
                )
                .await?
                .ok_or(error),
            Err(error) => Err(error),
        }
    }

    fn attachment_rows<'a>(
        &self,
        owner: &Value,
        keys: impl Iterator<Item = &'a Value>,
        attributes: Attributes,
        connection: &Connection,
    ) -> DatabaseResult<Vec<Attributes>> {
        let timestamps = self.pivot_write_timestamps(connection)?;
        let mut rows = self.pivot.records(owner, keys);
        for row in &mut rows {
            row.extend(timestamps.insert.clone());
            row.extend(self.pivot_constraints.defaults.clone());
            row.extend(attributes.clone());
        }
        Ok(rows)
    }

    pub(in crate::database::eloquent) fn factory_defaults(
        &self,
        connection: &Connection,
    ) -> DatabaseResult<Attributes> {
        Ok(self
            .keys
            .configure(R::query_on(connection))?
            .pending_attributes)
    }

    pub(in crate::database::eloquent) fn related_key(&self) -> &'static str {
        self.keys.related
    }

    pub(in crate::database::eloquent) fn validate_factory_attachments(
        &self,
        parent: &P,
        models: &[R],
    ) -> DatabaseResult<()> {
        if models.is_empty() {
            return Ok(());
        }
        let connection = self.keys.connection(parent)?;
        for model in models {
            if !model.state().exists() {
                return Err(DatabaseError::InvalidQuery(
                    "factory attachment requires a persisted model",
                ));
            }
            if !model
                .state()
                .connection
                .as_ref()
                .is_some_and(|source| connection.same_query_source(source))
            {
                return Err(DatabaseError::InvalidQuery(
                    "mixed relationship connections",
                ));
            }
        }
        Ok(())
    }

    #[track_caller]
    pub fn attach<'a>(
        &'a self,
        parent: &'a P,
        keys: &'a [Value],
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        self.attach_with(parent, keys, Attributes::new())
    }

    #[track_caller]
    pub fn attach_with<'a>(
        &'a self,
        parent: &'a P,
        keys: &'a [Value],
        attributes: Attributes,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(with_query_caller(
            std::panic::Location::caller(),
            self.attach_records(parent, keys, attributes, AttachmentMode::Default),
        ))
    }

    /// Runs the pivot write and any model events in a single transaction.
    #[track_caller]
    pub fn attach_or_fail<'a>(
        &'a self,
        parent: &'a P,
        keys: &'a [Value],
        attributes: Attributes,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(with_query_caller(
            std::panic::Location::caller(),
            self.attach_records(parent, keys, attributes, AttachmentMode::Transactional),
        ))
    }

    fn attach_records<'a>(
        &'a self,
        parent: &'a P,
        keys: &'a [Value],
        attributes: Attributes,
        mode: AttachmentMode,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            self.validate_pivot_attributes(&attributes)?;
            let owner = self.owner(parent)?;
            let keys = key_map(keys.iter().cloned())?;
            let connection = self.keys.connection(parent)?;
            let rows = self.attachment_rows(owner, keys.values(), attributes, &connection)?;
            let (parent_state, related_touch) = self.pivot_touches(parent, owner, &connection)?;
            let custom = self.custom_pivot;
            if mode != AttachmentMode::Default
                || custom.is_some()
                || parent_state.is_some()
                || related_touch.is_some()
            {
                let identity = self.pivot.identity(self.timestamp_columns());
                let source = connection.clone();
                let rows = Arc::new(rows);
                connection
                    .transaction(
                        move |transaction| {
                            let related_touch = related_touch.clone();
                            let source = source.clone();
                            let rows = rows.clone();
                            let parent_state = parent_state.clone();
                            Box::pin(async move {
                                if insert_pivot_rows(transaction, &source, identity, custom, &rows)
                                    .await?
                                {
                                    touch_endpoints::<P, R>(
                                        parent_state,
                                        related_touch,
                                        &source,
                                        transaction,
                                    )
                                    .await?;
                                } else if mode == AttachmentMode::Required {
                                    return Err(DatabaseError::InvalidQuery(
                                        "pivot attachment cancelled",
                                    ));
                                }
                                Ok(())
                            })
                        },
                        3,
                    )
                    .await?;
            } else {
                connection.table(self.pivot.table).insert(&rows).await?;
            }
            Ok(())
        })
    }

    #[track_caller]
    pub fn update_existing_pivot<'a>(
        &'a self,
        parent: &'a P,
        related: &'a Value,
        attributes: Attributes,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(with_query_caller(
            std::panic::Location::caller(),
            self.update_pivot(parent, related, attributes, false),
        ))
    }

    /// Runs the pivot write and any model events in a single transaction.
    #[track_caller]
    pub fn update_existing_pivot_or_fail<'a>(
        &'a self,
        parent: &'a P,
        related: &'a Value,
        attributes: Attributes,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(with_query_caller(
            std::panic::Location::caller(),
            self.update_pivot(parent, related, attributes, true),
        ))
    }

    fn update_pivot<'a>(
        &'a self,
        parent: &'a P,
        related: &'a Value,
        mut attributes: Attributes,
        transactional: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(async move {
            self.validate_pivot_attributes(&attributes)?;
            let owner = self.owner(parent)?;
            if dictionary_key(Some(related))?.is_none() {
                return Err(DatabaseError::InvalidQuery("pivot key is null"));
            }
            let (_, updated) = self.timestamp_columns();
            if attributes.is_empty() && (updated.is_none() || self.custom_pivot.is_some()) {
                return Ok(0);
            }
            let connection = self.keys.connection(parent)?;
            let custom = self.custom_pivot;
            if custom.is_none()
                && let Some(column) = updated
            {
                attributes.retain(|name, _| !name.eq_ignore_ascii_case(column));
                attributes.extend(self.pivot_write_timestamps(&connection)?.update);
            }
            let (parent_state, related_touch) = self.pivot_touches(parent, owner, &connection)?;
            if transactional
                || custom.is_some()
                || parent_state.is_some()
                || related_touch.is_some()
            {
                let pivot = self.pivot;
                let identity = pivot.identity(self.timestamp_columns());
                let owner = owner.clone();
                let related = related.clone();
                let constraints = self.pivot_constraints.clone();
                let source = connection.clone();
                return connection
                    .transaction(
                        move |transaction| {
                            let related_touch = related_touch.clone();
                            let (owner, related, constraints, source, attributes, parent_state) = (
                                owner.clone(),
                                related.clone(),
                                constraints.clone(),
                                source.clone(),
                                attributes.clone(),
                                parent_state.clone(),
                            );
                            Box::pin(async move {
                                let query = constraints
                                    .apply(pivot.for_owner(transaction.table(pivot.table), &owner))
                                    .r#where(pivot.related, "=", related);
                                let affected = if let Some(custom) = custom {
                                    match query.lock_for_update().first().await? {
                                        Some(raw) => u64::from(
                                            (custom.update)(
                                                transaction,
                                                &source,
                                                identity,
                                                raw.into_columns().into_iter().collect(),
                                                attributes,
                                            )
                                            .await?,
                                        ),
                                        None => 0,
                                    }
                                } else {
                                    query.update(attributes).await?
                                };
                                if affected != 0 {
                                    touch_endpoints::<P, R>(
                                        parent_state,
                                        related_touch,
                                        &source,
                                        transaction,
                                    )
                                    .await?;
                                }
                                Ok(affected)
                            })
                        },
                        3,
                    )
                    .await;
            }
            let query = connection.table(self.pivot.table);
            self.pivot_constraints
                .apply(self.pivot.for_owner(query, owner))
                .r#where(self.pivot.related, "=", related.clone())
                .update(attributes)
                .await
        })
    }

    fn validate_pivot_attributes(&self, attributes: &Attributes) -> DatabaseResult<()> {
        let protected = [self.pivot.foreign, self.pivot.related]
            .into_iter()
            .chain(self.pivot.morph.map(|(column, _)| column));
        if attributes.keys().any(|column| {
            protected
                .clone()
                .any(|key| column.eq_ignore_ascii_case(key))
        }) {
            return Err(DatabaseError::InvalidQuery(
                "pivot attributes cannot replace relationship keys",
            ));
        }
        Ok(())
    }

    #[track_caller]
    pub fn detach<'a>(
        &'a self,
        parent: &'a P,
        keys: Option<&'a [Value]>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(with_query_caller(
            std::panic::Location::caller(),
            self.detach_records(parent, keys, false),
        ))
    }

    /// Runs the pivot write and any model events in a single transaction.
    #[track_caller]
    pub fn detach_or_fail<'a>(
        &'a self,
        parent: &'a P,
        keys: Option<&'a [Value]>,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(with_query_caller(
            std::panic::Location::caller(),
            self.detach_records(parent, keys, true),
        ))
    }

    fn detach_records<'a>(
        &'a self,
        parent: &'a P,
        keys: Option<&'a [Value]>,
        transactional: bool,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(async move {
            let owner = self.owner(parent)?;
            let connection = self.keys.connection(parent)?;
            let (parent_state, related_touch) = self.pivot_touches(parent, owner, &connection)?;
            let custom = self.custom_pivot;
            if transactional
                || custom.is_some()
                || parent_state.is_some()
                || related_touch.is_some()
            {
                let pivot = self.pivot;
                let identity = pivot.identity(self.timestamp_columns());
                let constraints = self.pivot_constraints.clone();
                let owner = owner.clone();
                let keys = keys
                    .map(|keys| {
                        key_map(keys.iter().cloned())
                            .map(|keys| keys.into_values().collect::<Vec<_>>())
                    })
                    .transpose()?;
                let source = connection.clone();
                return connection
                    .transaction(
                        move |transaction| {
                            let related_touch = related_touch.clone();
                            let (source, constraints, keys, owner, parent_state) = (
                                source.clone(),
                                constraints.clone(),
                                keys.clone(),
                                owner.clone(),
                                parent_state.clone(),
                            );
                            Box::pin(async move {
                                let mut query = constraints
                                    .apply(pivot.for_owner(transaction.table(pivot.table), &owner));
                                if let Some(keys) = keys {
                                    query = query.where_in(pivot.related, keys);
                                }
                                let removed = if let Some(custom) = custom {
                                    let rows = query.lock_for_update().get().await?;
                                    let mut count = 0;
                                    for row in rows {
                                        count += u64::from(
                                            (custom.delete)(
                                                transaction,
                                                &source,
                                                identity,
                                                row.into_columns().into_iter().collect(),
                                            )
                                            .await?,
                                        );
                                    }
                                    count
                                } else {
                                    query.delete().await?
                                };
                                if removed != 0 {
                                    touch_endpoints::<P, R>(
                                        parent_state,
                                        related_touch,
                                        &source,
                                        transaction,
                                    )
                                    .await?;
                                }
                                Ok(removed)
                            })
                        },
                        3,
                    )
                    .await;
            }
            let query = connection.table(self.pivot.table);
            let mut query = self
                .pivot_constraints
                .apply(self.pivot.for_owner(query, owner));
            if let Some(keys) = keys {
                let keys = key_map(keys.iter().cloned())?;
                query = query.where_in(self.pivot.related, keys.into_values().collect());
            }
            query.delete().await
        })
    }

    #[track_caller]
    pub fn sync(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> impl Future<Output = DatabaseResult<SyncChanges>> {
        self.sync_with_pivot_values(parent, keys, Attributes::new(), true)
    }

    #[track_caller]
    pub fn sync_without_detaching(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> impl Future<Output = DatabaseResult<SyncChanges>> {
        self.sync_with_pivot_values(parent, keys, Attributes::new(), false)
    }

    #[track_caller]
    pub fn sync_with_pivot_values(
        &self,
        parent: &P,
        keys: &[Value],
        attributes: Attributes,
        detach: bool,
    ) -> impl Future<Output = DatabaseResult<SyncChanges>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.synchronize(
                parent,
                keys.iter().cloned().map(|key| (key, attributes.clone())),
                PivotOperation::Sync { detach },
            )
            .await
        })
    }

    /// Synchronizes in the same transaction used by all sync variants.
    #[track_caller]
    pub fn sync_or_fail(
        &self,
        parent: &P,
        keys: &[Value],
        detach: bool,
    ) -> impl Future<Output = DatabaseResult<SyncChanges>> {
        self.sync_with_pivot_values_or_fail(parent, keys, Attributes::new(), detach)
    }

    #[track_caller]
    pub fn sync_without_detaching_or_fail(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> impl Future<Output = DatabaseResult<SyncChanges>> {
        self.sync_or_fail(parent, keys, false)
    }

    #[track_caller]
    pub fn sync_with_pivot_values_or_fail(
        &self,
        parent: &P,
        keys: &[Value],
        attributes: Attributes,
        detach: bool,
    ) -> impl Future<Output = DatabaseResult<SyncChanges>> {
        self.sync_with_pivot_values(parent, keys, attributes, detach)
    }

    /// Synchronizes related keys with a distinct set of pivot values for each key.
    #[track_caller]
    pub fn sync_with_pivot_attributes(
        &self,
        parent: &P,
        records: &[(Value, Attributes)],
        detach: bool,
    ) -> impl Future<Output = DatabaseResult<SyncChanges>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.synchronize(
                parent,
                records.iter().cloned(),
                PivotOperation::Sync { detach },
            )
            .await
        })
    }

    /// Switches only the requested links in a single transaction.
    #[track_caller]
    pub fn toggle(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> impl Future<Output = DatabaseResult<ToggleChanges>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.toggle_records(
                parent,
                keys.iter().cloned().map(|key| (key, Attributes::new())),
            )
            .await
        })
    }

    /// Switches links, applying per-key attributes when a link is inserted.
    #[track_caller]
    pub fn toggle_with_pivot_attributes(
        &self,
        parent: &P,
        records: &[(Value, Attributes)],
    ) -> impl Future<Output = DatabaseResult<ToggleChanges>> {
        with_query_caller(std::panic::Location::caller(), async move {
            self.toggle_records(parent, records.iter().cloned()).await
        })
    }

    /// Switches links transactionally, rolling back all changes on error.
    #[track_caller]
    pub fn toggle_or_fail(
        &self,
        parent: &P,
        keys: &[Value],
    ) -> impl Future<Output = DatabaseResult<ToggleChanges>> {
        self.toggle(parent, keys)
    }

    async fn toggle_records(
        &self,
        parent: &P,
        records: impl Iterator<Item = (Value, Attributes)> + Send,
    ) -> DatabaseResult<ToggleChanges> {
        let changes = self
            .synchronize(parent, records, PivotOperation::Toggle)
            .await?;
        Ok(ToggleChanges {
            attached: changes.attached,
            detached: changes.detached,
        })
    }

    fn synchronize<'a>(
        &'a self,
        parent: &'a P,
        records: impl Iterator<Item = (Value, Attributes)> + Send + 'a,
        operation: PivotOperation,
    ) -> DatabaseFuture<'a, DatabaseResult<SyncChanges>> {
        Box::pin(async move {
            let owner = self.owner(parent)?.clone();
            let desired = records
                .map(|(key, attributes)| {
                    self.validate_pivot_attributes(&attributes)?;
                    let key = cast_pivot_key::<R>(self.keys.related, key)?;
                    let normalized = dictionary_key(Some(&key))?
                        .ok_or(DatabaseError::InvalidQuery("pivot key is null"))?;
                    Ok((normalized, (key, attributes)))
                })
                .collect::<DatabaseResult<DesiredPivots>>()?;
            if desired.is_empty() && !operation.removes(false) {
                return Ok(SyncChanges::default());
            }
            let desired = Arc::new(desired);
            let pivot = self.pivot;
            let related_key = self.keys.related;
            let constraints = Arc::new(self.pivot_constraints.clone());
            let connection = self.keys.connection(parent)?;
            let timestamps = Arc::new(self.pivot_write_timestamps(&connection)?);
            let custom = self.custom_pivot;
            let identity = pivot.identity(self.timestamp_columns());
            let source = connection.clone();
            let (parent_state, related_touch) = self.pivot_touches(parent, &owner, &connection)?;
            connection
                .transaction(
                    move |transaction| {
                        let related_touch = related_touch.clone();
                        let parent_state = parent_state.clone();
                        let (source, owner, desired, constraints, timestamps) = (
                            source.clone(),
                            owner.clone(),
                            desired.clone(),
                            constraints.clone(),
                            timestamps.clone(),
                        );
                        Box::pin(async move {
                            let mut changes = if let Some(custom) = custom {
                                sync_custom_pivot(
                                    transaction,
                                    CustomSync {
                                        pivot,
                                        custom,
                                        identity,
                                        source: source.clone(),
                                        owner: owner.clone(),
                                        desired: desired.clone(),
                                        constraints: constraints.clone(),
                                        timestamps: timestamps.clone(),
                                        operation,
                                    },
                                )
                                .await?
                            } else {
                                sync_pivot(
                                    transaction,
                                    pivot,
                                    owner.clone(),
                                    desired.clone(),
                                    constraints.clone(),
                                    timestamps.clone(),
                                    operation,
                                )
                                .await?
                            };
                            changes.detached = changes
                                .detached
                                .into_iter()
                                .map(|key| cast_pivot_key::<R>(related_key, key))
                                .collect::<DatabaseResult<_>>()?;
                            if !changes.attached.is_empty()
                                || !changes.detached.is_empty()
                                || !changes.updated.is_empty()
                            {
                                touch_endpoints::<P, R>(
                                    parent_state,
                                    related_touch,
                                    &source,
                                    transaction,
                                )
                                .await?;
                            }
                            Ok(changes)
                        })
                    },
                    3,
                )
                .await
        })
    }

    fn touch_state(&self, parent: &P, connection: &Connection) -> DatabaseResult<Arc<ModelState>> {
        if !parent.state().exists() {
            return Err(DatabaseError::InvalidQuery(
                "pivot parent touch requires a persisted model",
            ));
        }
        if parent
            .state()
            .connection
            .as_ref()
            .is_some_and(|source| !connection.same_query_source(source))
            || P::metadata()
                .connection
                .is_some_and(|name| name != connection.get_name())
        {
            return Err(DatabaseError::InvalidQuery(
                "pivot parent touch requires one connection",
            ));
        }
        Ok(Arc::new(parent.state().clone()))
    }

    fn owner<'a>(&self, parent: &'a P) -> DatabaseResult<&'a Value> {
        parent
            .get_attribute(self.keys.parent)
            .filter(|value| !matches!(value, Value::Null))
            .ok_or(DatabaseError::InvalidQuery("relationship parent key"))
    }

    #[track_caller]
    pub fn paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<R>>> {
        self.paginate_with(
            parent,
            PageOptions {
                per_page: Some(per_page),
                ..Default::default()
            },
        )
    }

    #[track_caller]
    pub fn paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<LengthAwarePaginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            let names = self.pivot_names();
            let columns = self.page_pivot_columns(&names, false);
            self.pagination_query(parent, options.columns, &columns)?
                .paginate_hydrated(options, |models, connection| {
                    self.hydrate_page_pivots(parent, models, &columns, &names, connection)
                })
                .await
        })
    }

    #[track_caller]
    pub fn simple_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<Paginator<R>>> {
        self.simple_paginate_with(
            parent,
            PageOptions {
                per_page: Some(per_page),
                ..Default::default()
            },
        )
    }

    #[track_caller]
    pub fn simple_paginate_with(
        &self,
        parent: &P,
        options: PageOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<Paginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            let names = self.pivot_names();
            let columns = self.page_pivot_columns(&names, false);
            self.pagination_query(parent, options.columns, &columns)?
                .simple_paginate_hydrated(options, |models, connection| {
                    self.hydrate_page_pivots(parent, models, &columns, &names, connection)
                })
                .await
        })
    }

    #[track_caller]
    pub fn cursor_paginate(
        &self,
        parent: &P,
        per_page: u64,
    ) -> impl Future<Output = DatabaseResult<CursorPaginator<R>>> {
        self.cursor_paginate_with(
            parent,
            CursorOptions {
                per_page: Some(per_page),
                ..Default::default()
            },
        )
    }

    #[track_caller]
    pub fn cursor_paginate_with(
        &self,
        parent: &P,
        options: CursorOptions<'_>,
    ) -> impl Future<Output = DatabaseResult<CursorPaginator<R>>> {
        with_query_caller(std::panic::Location::caller(), async move {
            let names = self.pivot_names();
            let columns = self.page_pivot_columns(&names, true);
            self.pagination_query(parent, options.columns, &columns)?
                .cursor_paginate_hydrated(options, |models, connection| {
                    self.hydrate_page_pivots(parent, models, &columns, &names, connection)
                })
                .await
        })
    }

    super::key_chunks::lazy_chunks!(R);

    fn lazy_relation<'a>(
        &'a self,
        parent: &'a P,
        chunk_size: u64,
        key: Option<(Option<&'a str>, Option<&'a str>, bool)>,
    ) -> DatabaseCursor<'a, R> {
        query::flatten_batches(
            self.pivot_batches(parent, chunk_size, key, true),
            |models| models,
        )
    }

    pub async fn each<Fut>(
        &self,
        parent: &P,
        callback: impl FnMut(R, crate::support::ArrayKey) -> Fut,
        count: u64,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let batches = self
            .pivot_batches(parent, count, None, false)
            .map_ok(crate::database::eloquent::Collection::to_base);
        query::visit_batches(batches, |_, key| Ok(key), callback).await
    }

    pub async fn each_by_id<Fut>(
        &self,
        parent: &P,
        callback: impl FnMut(R, i64) -> Fut,
        count: u64,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let batches = self
            .pivot_batches(parent, count, Some((column, alias, false)), false)
            .map_ok(crate::database::eloquent::Collection::to_base);
        query::visit_batches(
            batches,
            |batch, key| query::collection_chunk_index(batch, count, key),
            callback,
        )
        .await
    }

    fn pivot_batches<'a>(
        &'a self,
        parent: &'a P,
        count: u64,
        key: Option<(Option<&'a str>, Option<&'a str>, bool)>,
        lazy: bool,
    ) -> DatabaseCursor<'a, crate::database::eloquent::Collection<R>> {
        Box::pin(async_stream::try_stream! {
            let names = self.pivot_names();
            let columns = self.aliased_pivot_columns(&names);
            let query = self.pagination_query(parent, &["*"], &columns)?;
            let key = key.map(|(column, alias, descending)| query::ChunkKey {
                column: column.map_or_else(|| query.qualify_column(self.keys.related), str::to_owned),
                alias: alias.unwrap_or(self.keys.related).into(),
                descending,
                lazy,
            });
            let mut batches = query.chunk_collections(count, key, |models, connection| {
                self.hydrate_page_pivots(parent, models, &columns, &names, connection)
            });
            while let Some(batch) = batches.try_next().await? {
                yield batch;
            }
        })
    }

    /// Reads this parent's related models in ordered batches.
    pub async fn chunk<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.chunk_pivots(parent, count, callback, None).await
    }

    pub async fn chunk_by_id<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.ordered_chunk_by_id(parent, count, callback, column, alias, false)
            .await
    }

    pub async fn chunk_by_id_desc<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.ordered_chunk_by_id(parent, count, callback, column, alias, true)
            .await
    }

    pub async fn ordered_chunk_by_id<Fut>(
        &self,
        parent: &P,
        count: u64,
        callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
        descending: bool,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.chunk_pivots(parent, count, callback, Some((column, alias, descending)))
            .await
    }

    async fn chunk_pivots<Fut>(
        &self,
        parent: &P,
        count: u64,
        mut callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
        key: Option<(Option<&str>, Option<&str>, bool)>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let mut batches = self.pivot_batches(parent, count, key, false);
        let mut page = 1;
        while let Some(models) = batches.try_next().await? {
            if !callback(models, page).await? {
                return Ok(false);
            }
            page += 1;
        }
        Ok(true)
    }

    /// Streams related models with their intermediate-table attributes.
    #[track_caller]
    pub fn cursor<'a>(&'a self, parent: &'a P) -> DatabaseCursor<'a, R> {
        Box::pin(crate::database::events::stream_with_query_caller(
            std::panic::Location::caller(),
            async_stream::try_stream! {
                let names = self.pivot_names();
                let columns = self.aliased_pivot_columns(&names);
                let query = self.pagination_query(parent, &["*"], &columns)?;
                let declaring = self.pivot_parent_snapshot(parent);
                let mut models = query.cursor_hydrated(|model, connection| {
                    self.hydrate_row_pivot(model, &columns, &names, connection, declaring.clone())
                });
                while let Some(model) = models.try_next().await? {
                    yield model;
                }
            },
        ))
    }

    fn pagination_query(
        &self,
        parent: &P,
        columns: &[&str],
        pivot_columns: &[String],
    ) -> DatabaseResult<Builder<'static, R>> {
        let connection = self.keys.connection(parent)?;
        let query = R::query_on(&connection);
        let columns = if columns.is_empty() || columns == ["*"] {
            vec![query.qualify_column("*")]
        } else {
            columns
                .iter()
                .map(|column| query.qualify_column(column))
                .collect()
        };
        let query = self
            .keys
            .configure(query.select_relation_columns(columns.iter().map(String::as_str)))?;
        let related = query.qualify_column(self.keys.related);
        let pivot_key = format!("{}.{}", self.pivot.table, self.pivot.related);
        let owner = format!("{}.{}", self.pivot.table, self.pivot.foreign);
        let keys = ParentKeys::new(&[parent], self.keys.parent)?;
        Ok(query
            .restrict_relation(&owner, &keys.bindings)
            .map_relation_query(|query| {
                let query = query.join(self.pivot.table, &related, "=", &pivot_key);
                self.apply_pivot_query(query)
                    .add_select(pivot_columns.iter().map(String::as_str))
            }))
    }

    fn apply_pivot_query(&self, query: query::Builder<'static>) -> query::Builder<'static> {
        let query = self
            .pivot_constraints
            .apply(self.pivot.restrict_type(query, true));
        self.pivot_orders
            .iter()
            .fold(query, |query, (column, direction)| {
                query.order_by(column, direction)
            })
    }

    fn pivot_names(&self) -> Vec<&'static str> {
        let mut names: Vec<_> = [self.pivot.foreign, self.pivot.related]
            .into_iter()
            .chain(self.pivot.morph.map(|(kind, _)| kind))
            .chain(self.pivot_columns.iter().copied())
            .collect();
        if let Some((created, updated)) = self.pivot_timestamps {
            for column in created.into_iter().chain(updated) {
                if !names.contains(&column) {
                    names.push(column);
                }
            }
        }
        if let Some(primary_key) = self.custom_pivot.and_then(|custom| custom.primary_key)
            && !names.contains(&primary_key)
        {
            names.push(primary_key);
        }
        names
    }

    fn page_pivot_columns(&self, names: &[&str], cursor: bool) -> Vec<String> {
        let mut columns = self.aliased_pivot_columns(names);
        if cursor {
            for (column, _) in &self.pivot_orders {
                let already_selected = columns.iter().any(|selected| {
                    selected
                        .split_once(" as ")
                        .is_some_and(|(source, _)| source == column)
                });
                if !already_selected {
                    columns.push(format!("{column} as laravel_pivot_{}", columns.len()));
                }
            }
        }
        columns
    }

    fn aliased_pivot_columns(&self, names: &[&str]) -> Vec<String> {
        names
            .iter()
            .enumerate()
            .map(|(index, column)| {
                format!("{}.{column} as laravel_pivot_{index}", self.pivot.table)
            })
            .collect()
    }

    fn hydrate_page_pivots(
        &self,
        parent: &P,
        models: &mut [R],
        columns: &[String],
        names: &[&str],
        connection: &Connection,
    ) -> DatabaseResult<()> {
        let declaring = self.pivot_parent_snapshot(parent);
        for model in models {
            self.hydrate_row_pivot(model, columns, names, connection, declaring.clone())?;
        }
        Ok(())
    }

    fn pivot_parent_snapshot(&self, parent: &P) -> Option<Arc<P>> {
        self.pivot_inverses
            .0
            .as_ref()
            .map(|_| Arc::new(P::from_state(parent.state().clone())))
    }

    fn hydrate_row_pivot(
        &self,
        model: &mut R,
        columns: &[String],
        names: &[&str],
        connection: &Connection,
        declaring: Option<Arc<P>>,
    ) -> DatabaseResult<()> {
        let state = model.state_mut();
        let mut pivot = Attributes::new();
        for (index, column) in columns.iter().enumerate() {
            let (_, alias) = column.split_once(" as ").expect("fixed pivot projection");
            let value = state
                .attributes
                .remove(alias)
                .ok_or(DatabaseError::InvalidQuery("missing pivot column"))?;
            state.original.remove(alias);
            if let Some(&name) = names.get(index) {
                pivot.insert(name.into(), value);
            }
        }
        self.attach_pivot(state, pivot, connection, declaring)
    }

    fn attach_pivot(
        &self,
        state: &mut ModelState,
        attributes: Attributes,
        connection: &Connection,
        declaring: Option<Arc<P>>,
    ) -> DatabaseResult<()> {
        if let Some(custom) = self.custom_pivot {
            let mut defaults = self.pivot_constraints.defaults.clone();
            defaults.extend(attributes);
            let mut inverses = LoadedRelations::default();
            if let Some(name) = &self.pivot_inverses.0 {
                inverses.insert_inverse(name.clone(), declaring);
            }
            if let Some(name) = &self.pivot_inverses.1 {
                // An owned snapshot avoids a strong reference cycle through the pivot.
                let related = Arc::new(R::from_state(state.clone()));
                inverses.insert_inverse(name.clone(), Some(related));
            }
            (custom.hydrate)(
                &mut state.relations,
                self.pivot_accessor,
                defaults,
                connection,
                self.pivot.identity(self.timestamp_columns()),
                inverses,
            )?;
        } else {
            state.relations.insert_pivot(
                self.pivot_accessor.into(),
                Pivot::new(self.pivot.table, attributes),
            );
        }
        Ok(())
    }

    async fn match_many(
        &self,
        parents: &[&P],
        mut transaction: Option<&mut Transaction>,
        lookup: Option<RelatedLookup<'_>>,
    ) -> DatabaseResult<Vec<Vec<Arc<R>>>> {
        let keys = ParentKeys::new(parents, self.keys.parent)?;
        let mut dictionary: HashMap<_, Vec<_>> = HashMap::new();
        if !keys.bindings.is_empty() {
            let connection = self.keys.batch_connection(parents)?;
            let (declaring_inverse, _) = &self.pivot_inverses;
            let mut declaring_models = HashMap::new();
            if declaring_inverse.is_some() {
                for parent in parents {
                    if let Some(key) = dictionary_key(parent.get_attribute(self.keys.parent))? {
                        declaring_models
                            .entry(key)
                            .or_insert_with(|| Arc::new(P::from_state(parent.state().clone())));
                    }
                }
            }
            let parent_column = format!("{}.{}", self.pivot.table, self.pivot.foreign);
            let pivot_key = format!("{}.{}", self.pivot.table, self.pivot.related);
            let related_key = format!("{}.{}", R::metadata().table, self.keys.related);
            let related_columns = format!("{}.*", R::metadata().table);
            let pivot_names = self.pivot_names();
            let pivot_columns = self.aliased_pivot_columns(&pivot_names);
            for batch in keys.bindings.chunks(500) {
                let mut query = self
                    .keys
                    .configure(
                        R::query_on(&connection)
                            .select_relation_columns([related_columns.as_str()]),
                    )?
                    .apply_default_counts()?
                    .restrict_relation(&parent_column, batch);
                if let Some(lookup) = lookup {
                    query = lookup.selection.apply(query);
                    if lookup.include_trashed {
                        query = query.with_trashed();
                    }
                    if lookup.use_writer {
                        query = query.use_write_pdo();
                    }
                }
                let eager_load = query.take_eager_load();
                let existence = query.take_existence_attributes();
                let casts = query.casts.clone();
                let query = query
                    .scoped()?
                    .0
                    .join(self.pivot.table, &related_key, "=", &pivot_key);
                let query = self.apply_pivot_query(query);
                let rows = query
                    .add_select(pivot_columns.iter().map(String::as_str))
                    .get_with_transaction(transaction.as_deref_mut())
                    .await?;
                let mut models = Vec::with_capacity(rows.len());
                let mut owners = Vec::with_capacity(rows.len());
                for row in rows {
                    let mut columns = row.into_columns();
                    let pivot_start = columns
                        .len()
                        .checked_sub(pivot_names.len())
                        .ok_or(DatabaseError::InvalidQuery("pivot match key"))?;
                    let pivot = columns.split_off(pivot_start);
                    if let Some(owner) = dictionary_key(Some(&pivot[0].1))? {
                        let mut model_attributes = columns.into_iter().collect();
                        cast_existence(&mut model_attributes, &existence);
                        let mut state = ModelState::hydrated_attributes(
                            model_attributes,
                            connection.clone(),
                            casts.clone(),
                        );
                        let attributes = pivot_names
                            .iter()
                            .zip(pivot)
                            .map(|(name, (_, value))| ((*name).into(), value))
                            .collect();
                        self.attach_pivot(
                            &mut state,
                            attributes,
                            &connection,
                            declaring_models.get(&owner).cloned(),
                        )?;
                        owners.push(owner);
                        models.push(R::from_state(state));
                    }
                }
                super::super::concerns::has_events::retrieved_with_transaction(
                    &mut models,
                    &connection,
                    transaction
                        .as_deref_mut()
                        .filter(|tx| tx.belongs_to(&connection)),
                )
                .await?;
                load_eager_with_transaction(eager_load, &mut models, transaction.as_deref_mut())
                    .await?;
                for (owner, model) in owners.into_iter().zip(models) {
                    dictionary.entry(owner).or_default().push(Arc::new(model));
                }
            }
        }
        Ok(keys.match_models(&dictionary))
    }
}

impl<P: Model, R: Model> Relation for BelongsToMany<P, R> {
    type Parent = P;
    type Output = super::super::RelatedCollection<R>;

    fn is_empty_collection(output: &Self::Output) -> bool {
        output.is_empty()
    }

    fn attach_autoload(outputs: &mut [Self::Output]) {
        super::super::detached_autoload::attach_outputs(outputs);
    }

    fn push_loaded<'a>(
        output: &'a Self::Output,
        context: &'a mut super::PushContext,
    ) -> DatabaseFuture<'a, (Option<Self::Output>, DatabaseResult<bool>)> {
        Box::pin(async move {
            let (models, result) = context.push_many(output).await;
            (models.map(|models| output.with_models(models)), result)
        })
    }

    fn autoload_output(output: &Self::Output) -> Option<Self::Output> {
        Some(output.clone())
    }

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::many(output)
    }

    fn queueable_relations(output: &Self::Output) -> Vec<String> {
        super::super::queueable::common_relations(output.iter().map(Arc::as_ref))
    }

    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            self.match_results(parents, None),
        ))
    }

    fn match_results_in_transaction<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(crate::database::events::with_query_caller(
            std::panic::Location::caller(),
            self.match_results(parents, Some(transaction)),
        ))
    }
}

impl<P: Model, R: Model> BelongsToMany<P, R> {
    fn match_results<'a>(
        &'a self,
        parents: &'a [&'a P],
        transaction: Option<&'a mut Transaction>,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<<Self as Relation>::Output>>> {
        Box::pin(async move {
            let results = self.match_many(parents, transaction, None).await?;
            Ok(results
                .into_iter()
                .map(super::super::RelatedCollection::from_models)
                .collect())
        })
    }
}

#[derive(Clone)]
struct RelatedTouch {
    pivot: PivotKeys,
    constraints: PivotConstraints,
    owner: Value,
    related_key: &'static str,
    column: &'static str,
}

impl RelatedTouch {
    fn query<R: Model>(&self, connection: &Connection) -> DatabaseResult<query::Builder<'static>> {
        let mut linked = self
            .constraints
            .apply(
                self.pivot
                    .for_owner(connection.table(self.pivot.table), &self.owner),
            )
            .select([self.pivot.related]);
        R::query_on(connection)
            .scoped()?
            .0
            .where_in_sub(self.related_key, &mut linked)
    }

    fn attributes<R: Model>(&self, connection: &Connection) -> DatabaseResult<Attributes> {
        Ok(Attributes::from([(
            self.column.into(),
            timestamp(connection, R::metadata().date_format)?,
        )]))
    }

    async fn run<R: Model>(
        &self,
        connection: &Connection,
        transaction: &mut Transaction,
    ) -> DatabaseResult<()> {
        self.query::<R>(connection)?
            .in_transaction(transaction)?
            .update(self.attributes::<R>(connection)?)
            .await?;
        Ok(())
    }
}

fn owned_model<R: Model>(model: Arc<R>) -> R {
    Arc::try_unwrap(model).unwrap_or_else(|model| R::from_state(model.state().clone()))
}

async fn restore_related<R: Model>(model: Arc<R>) -> DatabaseResult<Arc<R>> {
    let mut model = owned_model(model);
    model.restore().await?;
    Ok(Arc::new(model))
}

fn attachment_key<'a, R: Model>(model: &'a R, key: &str) -> DatabaseResult<&'a Value> {
    model
        .get_attribute(key)
        .filter(|value| !matches!(value, Value::Null))
        .ok_or(DatabaseError::InvalidQuery("relationship related key"))
}

async fn insert_pivot_rows(
    transaction: &mut Transaction,
    connection: &Connection,
    identity: PivotIdentity,
    custom: Option<CustomPivot>,
    rows: &[Attributes],
) -> DatabaseResult<bool> {
    if let Some(custom) = custom {
        let mut changed = false;
        for row in rows {
            changed |= (custom.insert)(transaction, connection, identity, row.clone()).await?;
        }
        return Ok(changed);
    }
    Ok(transaction.table(identity.table).insert(rows).await? && !rows.is_empty())
}

async fn touch_endpoints<P: Model, R: Model>(
    parent: Option<Arc<ModelState>>,
    related: Option<RelatedTouch>,
    connection: &Connection,
    transaction: &mut Transaction,
) -> DatabaseResult<()> {
    touch_parent::<P>(parent, connection, transaction).await?;
    if let Some(related) = related {
        related.run::<R>(connection, transaction).await?;
    }
    Ok(())
}

async fn touch_parent<P: Model>(
    state: Option<Arc<ModelState>>,
    connection: &Connection,
    transaction: &mut Transaction,
) -> DatabaseResult<()> {
    if let Some(state) = state {
        let mut model = P::from_state(Arc::unwrap_or_clone(state));
        let original = model.state().original.clone();
        model.state_mut().replace_attributes(original);
        let column = P::metadata()
            .updated_at
            .expect("touch enabled with a timestamp");
        model.set_attribute(column, timestamp(connection, P::metadata().date_format)?);
        model.save_in_transaction(transaction).await?;
    }
    Ok(())
}

fn pivot_inverse<M: Model>(
    pivot: CustomPivot,
    explicit: Option<&str>,
    foreign_key: &str,
) -> DatabaseResult<Option<String>> {
    use crate::support::Str;

    if let Some(name) = explicit {
        if !pivot.inverse_relations.contains(&name) {
            return Err(DatabaseError::InvalidQuery(
                "unknown custom pivot inverse relation",
            ));
        }
        return Ok(Some(name.to_owned()));
    }
    let stem = foreign_key
        .rsplit_once(M::metadata().primary_key)
        .map_or(foreign_key, |(stem, _)| stem);
    let model_name = std::any::type_name::<M>()
        .split('<')
        .next()
        .unwrap_or_default()
        .rsplit("::")
        .next()
        .unwrap_or_default();
    Ok([Str::camel(stem), Str::camel(model_name)]
        .into_iter()
        .find(|candidate| {
            !candidate.is_empty() && pivot.inverse_relations.contains(&candidate.as_str())
        }))
}

fn key_map(keys: impl IntoIterator<Item = Value>) -> DatabaseResult<IndexMap<Vec<u8>, Value>> {
    keys.into_iter()
        .map(|key| {
            let normalized = dictionary_key(Some(&key))?
                .ok_or(DatabaseError::InvalidQuery("pivot key is null"))?;
            Ok((normalized, key))
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PivotKeys {
    table: &'static str,
    foreign: &'static str,
    related: &'static str,
    morph: Option<(&'static str, &'static str)>,
}

pub(in crate::database::eloquent) struct PivotTouchIdentity<'a> {
    related_type: TypeId,
    pivot: PivotKeys,
    parent_key: &'static str,
    related_key: &'static str,
    constraints: &'a PivotConstraints,
}

impl PivotTouchIdentity<'_> {
    fn inverse<P: Model>(&self) -> Self {
        Self {
            related_type: TypeId::of::<P>(),
            pivot: PivotKeys {
                foreign: self.pivot.related,
                related: self.pivot.foreign,
                ..self.pivot
            },
            parent_key: self.related_key,
            related_key: self.parent_key,
            constraints: self.constraints,
        }
    }

    pub(in crate::database::eloquent) fn matches(
        &self,
        other: &Self,
        connection: &Connection,
    ) -> DatabaseResult<bool> {
        if self.related_type != other.related_type
            || self.pivot != other.pivot
            || self.parent_key != other.parent_key
            || self.related_key != other.related_key
        {
            return Ok(false);
        }
        let mut left = self.constraints.apply(connection.table(self.pivot.table));
        let mut right = other.constraints.apply(connection.table(other.pivot.table));
        Ok(left.to_sql()? == right.to_sql()? && left.get_bindings()? == right.get_bindings()?)
    }
}

impl PivotKeys {
    fn identity(self, timestamps: PivotTimestampColumns) -> PivotIdentity {
        PivotIdentity {
            table: self.table,
            foreign: self.foreign,
            related: self.related,
            morph: self.morph,
            timestamps,
        }
    }

    fn restrict_type<'a>(&self, query: query::Builder<'a>, qualified: bool) -> query::Builder<'a> {
        match self.morph {
            Some((column, alias)) if qualified => {
                query.r#where(&format!("{}.{column}", self.table), "=", alias)
            }
            Some((column, alias)) => query.r#where(column, "=", alias),
            None => query,
        }
    }

    fn for_owner<'a>(&self, query: query::Builder<'a>, owner: &Value) -> query::Builder<'a> {
        self.restrict_type(query.r#where(self.foreign, "=", owner.clone()), false)
    }

    fn record(&self, owner: &Value, key: &Value) -> Attributes {
        let mut attributes = Attributes::from([
            (self.foreign.into(), owner.clone()),
            (self.related.into(), key.clone()),
        ]);
        if let Some((column, alias)) = self.morph {
            attributes.insert(column.into(), alias.into());
        }
        attributes
    }

    fn records<'a>(&self, owner: &Value, keys: impl Iterator<Item = &'a Value>) -> Vec<Attributes> {
        keys.map(|key| self.record(owner, key)).collect()
    }
}

type PivotTimestampColumns = (Option<&'static str>, Option<&'static str>);

struct PivotWriteTimestamps {
    insert: Attributes,
    update: Attributes,
}

type DesiredPivots = IndexMap<Vec<u8>, (Value, Attributes)>;

struct CustomSync {
    pivot: PivotKeys,
    custom: CustomPivot,
    identity: PivotIdentity,
    source: Connection,
    owner: Value,
    desired: Arc<DesiredPivots>,
    constraints: Arc<PivotConstraints>,
    timestamps: Arc<PivotWriteTimestamps>,
    operation: PivotOperation,
}

async fn sync_custom_pivot(
    transaction: &mut Transaction,
    sync: CustomSync,
) -> DatabaseResult<SyncChanges> {
    let CustomSync {
        pivot,
        custom,
        identity,
        source,
        owner,
        desired,
        constraints,
        timestamps,
        operation,
    } = sync;
    let query = constraints.apply(pivot.for_owner(transaction.table(pivot.table), &owner));
    let rows = if matches!(operation, PivotOperation::Toggle) {
        query.where_in(
            pivot.related,
            desired.values().map(|(key, _)| key.clone()).collect(),
        )
    } else {
        query
    };
    let rows = rows.lock_for_update().get().await?;
    let mut current = IndexMap::new();
    let mut changes = SyncChanges::default();
    for row in rows {
        let raw: Attributes = row.into_columns().into_iter().collect();
        let key = raw
            .get(pivot.related)
            .ok_or(DatabaseError::InvalidQuery("missing related pivot key"))?
            .clone();
        let normalized =
            dictionary_key(Some(&key))?.ok_or(DatabaseError::InvalidQuery("pivot key is null"))?;
        if operation.removes(desired.contains_key(&normalized)) {
            if (custom.delete)(transaction, &source, identity, raw).await? {
                changes.detached.push(key);
            }
        } else if matches!(operation, PivotOperation::Sync { .. }) {
            current.insert(normalized, Some(raw));
            continue;
        }
        current.insert(normalized, None);
    }
    for (normalized, (key, attributes)) in desired.iter() {
        if let Some(raw) = current.shift_remove(normalized) {
            if let Some(raw) = raw
                && !attributes.is_empty()
                && (custom.update)(transaction, &source, identity, raw, attributes.clone()).await?
            {
                changes.updated.push(key.clone());
            }
        } else {
            let mut row = pivot.record(&owner, key);
            row.extend(timestamps.insert.clone());
            row.extend(constraints.defaults.clone());
            row.extend(attributes.clone());
            if (custom.insert)(transaction, &source, identity, row).await? {
                changes.attached.push(key.clone());
            }
        }
    }
    Ok(changes)
}

async fn sync_pivot(
    transaction: &mut Transaction,
    pivot: PivotKeys,
    owner: Value,
    desired: Arc<DesiredPivots>,
    constraints: Arc<PivotConstraints>,
    timestamps: Arc<PivotWriteTimestamps>,
    operation: PivotOperation,
) -> DatabaseResult<SyncChanges> {
    let query = constraints.apply(pivot.for_owner(
        transaction.table(pivot.table).select([pivot.related]),
        &owner,
    ));
    let query = if matches!(operation, PivotOperation::Toggle) {
        query.where_in(
            pivot.related,
            desired.values().map(|(key, _)| key.clone()).collect(),
        )
    } else {
        query
    };
    let current = query.lock_for_update().get().await?;
    let current = key_map(
        current
            .into_iter()
            .map(|row| row.into_columns().pop().expect("selected pivot key").1),
    )?;
    let detached: Vec<_> = current
        .iter()
        .filter(|(key, _)| operation.removes(desired.contains_key(*key)))
        .map(|(_, value)| value.clone())
        .collect();
    for keys in detached.chunks(500) {
        let query = transaction.table(pivot.table);
        constraints
            .apply(pivot.for_owner(query, &owner))
            .where_in(pivot.related, keys.to_vec())
            .delete()
            .await?;
    }
    let mut inserts: IndexMap<Vec<String>, Vec<Attributes>> = IndexMap::new();
    let mut changes = SyncChanges {
        detached,
        ..SyncChanges::default()
    };
    for (normalized, (key, attributes)) in desired.iter() {
        if !current.contains_key(normalized) {
            let mut row = pivot.record(&owner, key);
            row.extend(timestamps.insert.clone());
            row.extend(constraints.defaults.clone());
            row.extend(attributes.clone());
            inserts
                .entry(row.keys().cloned().collect())
                .or_default()
                .push(row);
            changes.attached.push(key.clone());
        } else if matches!(operation, PivotOperation::Sync { .. }) && !attributes.is_empty() {
            let mut attributes = attributes.clone();
            attributes.retain(|name, _| {
                !timestamps
                    .update
                    .keys()
                    .any(|column| name.eq_ignore_ascii_case(column))
            });
            attributes.extend(timestamps.update.clone());
            let query = transaction.table(pivot.table);
            let affected = constraints
                .apply(pivot.for_owner(query, &owner))
                .r#where(pivot.related, "=", key.clone())
                .where_not(|conditions| {
                    attributes
                        .iter()
                        .fold(conditions, |conditions, (column, value)| {
                            conditions.where_null_safe_equals(column, value.clone())
                        })
                })
                .update(attributes)
                .await?;
            if affected > 0 {
                changes.updated.push(key.clone());
            }
        }
    }
    for rows in inserts.into_values() {
        for batch in rows.chunks(500) {
            transaction.table(pivot.table).insert(batch).await?;
        }
    }
    Ok(changes)
}

impl<P: Model, R: Model> ExistenceRelation for BelongsToMany<P, R> {
    fn get_relation_existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        self.existence_query(connection, parent_table, None)
    }
}

impl<P: Model, R: Model> BelongsToMany<P, R> {
    pub(in crate::database::eloquent) fn existence_query(
        &self,
        connection: &Connection,
        parent_table: &str,
        target_keys: Option<Vec<Value>>,
    ) -> DatabaseResult<query::Builder<'static>> {
        let query = self.keys.existence_builder(connection, parent_table)?;
        let target_key = query.qualify_column(R::metadata().primary_key);
        let related = query.qualify_column(self.keys.related);
        let related_pivot = format!("{}.{}", self.pivot.table, self.pivot.related);
        let foreign_pivot = format!("{}.{}", self.pivot.table, self.pivot.foreign);
        let parent = format!("{parent_table}.{}", self.keys.parent);
        let query = query
            .scoped()?
            .0
            .join(self.pivot.table, &related, "=", &related_pivot)
            .where_column(&foreign_pivot, "=", &parent);
        let query = self
            .pivot_constraints
            .apply(self.pivot.restrict_type(query, true));
        Ok(match target_keys {
            Some(keys) => query.where_in(&target_key, keys),
            None => query,
        })
    }
}

impl<P: Model, R: Model> super::NestedRelation for BelongsToMany<P, R> {
    type Related = R;

    fn with_nested(mut self, nested: super::super::EagerLoads<R>) -> Self {
        self.keys.with_nested(nested);
        self
    }

    fn with_path_columns(mut self, columns: Vec<String>) -> DatabaseResult<Self> {
        self.keys
            .append_constraint(move |query| query.select_path_columns(&columns));
        Ok(self)
    }

    fn with_path_constraint(
        self,
        constraint: impl Fn(
            crate::database::query::Conditions,
            &str,
        ) -> crate::database::query::Conditions
        + Send
        + Sync
        + 'static,
    ) -> DatabaseResult<Self> {
        Ok(super::ConstrainableRelation::constrain_query(
            self,
            move |query| query.with_path_constraint(&constraint),
        ))
    }
}

impl<P: Model, R: Model> super::ConstrainableRelation for BelongsToMany<P, R> {
    type Related = R;

    fn constrain_query(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.keys.append_constraint(configure);
        self
    }
}
