//! Eloquent model factories for seeders and tests.

use std::{
    any::{Any, TypeId, type_name},
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use tokio::sync::OnceCell;

use crate::{
    contracts::container::Container as _,
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Db, Value,
    },
    support::{Str, calendar},
};

use super::{
    Attributes, Collection, Model, casts,
    collection::without_relationship_autoloading,
    relations::{BelongsToMany, Relation},
    soft_deletes,
};

type Definition = dyn Fn(&Connection, &std::sync::OnceLock<Arc<Faker>>) -> DatabaseResult<Attributes>
    + Send
    + Sync;
type FactoryParentModel = dyn Any + Send + Sync;
type State = dyn Fn(&Attributes, &Connection, Option<&FactoryParentModel>) -> DatabaseResult<Attributes>
    + Send
    + Sync;
type CreateCallback<M> =
    dyn Fn(&mut M, Option<&FactoryParentModel>) -> DatabaseResult<()> + Send + Sync;

#[derive(Clone, Copy)]
struct RelationContext<'a> {
    attributes: Option<&'a (Attributes, Attributes)>,
    parent: &'a FactoryParentModel,
}

impl<'a> RelationContext<'a> {
    fn parent_only(parent: &'a FactoryParentModel) -> Self {
        Self {
            attributes: None,
            parent,
        }
    }
}
type Callback<M> = dyn Fn(&mut M) -> DatabaseResult<()> + Send + Sync;
type PivotResolver<M> = dyn Fn(&M) -> DatabaseResult<Attributes> + Send + Sync;
type Relationship<M> = dyn for<'a> Fn(
        &'a Connection,
        &'a mut M,
        &'a [RecycledModel],
    ) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;

mod faker;
mod faker_address;
mod faker_file;
mod faker_image;
mod faker_institutions;
mod faker_miscellaneous;
mod faker_payment;
mod faker_provider;
mod faker_rng;
mod regex_pattern;
mod relationships;

pub use faker::{Coordinates, CreditCardDetails, Faker, UniqueFaker, fake};
pub use faker_image::ImageOptions;
pub use faker_provider::{FakerExtension, FakerProvider};

pub use relationships::{FactoryChild, FactoryParent};

type Parent<M> = dyn Fn(&M) -> DatabaseResult<PreparedParent<M>> + Send + Sync;
type ParentResolver<M> = dyn for<'a> Fn(
        &'a Connection,
        &'a mut M,
        &'a [RecycledModel],
    ) -> DatabaseFuture<'a, DatabaseResult<()>>
    + Send
    + Sync;
struct PreparedParent<M> {
    foreign_key: Option<String>,
    morph_type: Option<(String, Value)>,
    resolve: Box<ParentResolver<M>>,
}

struct FactoryAttributes<M> {
    model: M,
    values: Attributes,
    parents: Vec<PreparedParent<M>>,
}

type SharedModel = Arc<dyn Any + Send + Sync>;
struct ResolvedParents<R> {
    owners: Mutex<Vec<ResolvedParent<R>>>,
}

struct ResolvedParent<R> {
    child_source: Connection,
    owner_source: Connection,
    model: Arc<OnceCell<Arc<R>>>,
}

impl<R> ResolvedParents<R> {
    fn new() -> Self {
        Self {
            owners: Mutex::new(Vec::new()),
        }
    }

    fn for_connection(&self, child: &Connection, owner: &Connection) -> Arc<OnceCell<Arc<R>>> {
        let mut owners = self.owners.lock().unwrap();
        if let Some(cached) = owners.iter().find(|cached| {
            cached.child_source.same_query_source(child)
                && cached.owner_source.same_query_source(owner)
        }) {
            return Arc::clone(&cached.model);
        }
        let model = Arc::new(OnceCell::new());
        owners.push(ResolvedParent {
            child_source: child.clone(),
            owner_source: owner.clone(),
            model: Arc::clone(&model),
        });
        model
    }
}

type DeferredAttributeResolver = dyn for<'a> Fn(
        &'a Attributes,
        &'a Connection,
        &'a [RecycledModel],
    ) -> DatabaseFuture<'a, DatabaseResult<Value>>
    + Send
    + Sync;

struct DeferredAttribute {
    name: String,
    model: Option<TypeId>,
    resolve: Box<DeferredAttributeResolver>,
}

/// Identifies a factory-valued parent attribute to exclude from expansion.
pub enum FactoryParentSelector {
    Attribute(String),
    Model(TypeId),
}

impl FactoryParentSelector {
    pub fn attribute(name: impl Into<String>) -> Self {
        Self::Attribute(name.into())
    }

    pub fn model<R: Model>() -> Self {
        Self::Model(TypeId::of::<R>())
    }
}

#[derive(Clone)]
struct RecycledModel {
    connection: Connection,
    model: SharedModel,
}

fn recycled_owner<'a, R: Model>(
    pool: &'a [RecycledModel],
    connection: Option<&Connection>,
) -> Option<&'a RecycledModel> {
    let candidates: Vec<_> = pool
        .iter()
        .filter(|entry| {
            entry.model.is::<R>()
                && connection.is_none_or(|source| entry.connection.same_query_source(source))
        })
        .collect();
    fastrand::choice(candidates)
}

/// A named factory definition with a compile-time model binding.
pub trait FactoryDefinition: Sized + 'static {
    type Model: Model;

    fn definition() -> Attributes;

    /// Configures the fresh factory after its initial state is registered.
    fn configure(factory: Factory<Self::Model>) -> Factory<Self::Model> {
        factory
    }

    fn factory() -> Factory<Self::Model> {
        Self::new(Attributes::new())
    }

    /// Builds a factory with an initial state, then applies its configuration.
    #[allow(
        clippy::new_ret_no_self,
        reason = "A named definition returns its typed factory builder"
    )]
    fn new(attributes: Attributes) -> Factory<Self::Model> {
        if attributes.is_empty() {
            Self::configure(Factory::new(Self::definition))
        } else {
            Self::new_with_state(move |_| attributes.clone())
        }
    }

    /// Builds a factory with a computed initial state before configuration.
    fn new_with_state(
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Factory<Self::Model> {
        Self::configure(Factory::new(Self::definition).state(state))
    }

    /// Builds a configured factory for the given number of models.
    fn times(count: usize) -> FactoryBatch<Self::Model> {
        Self::factory().count(count)
    }
}

/// Associates a model with the factory definition chosen by its application.
pub trait HasFactory: Model {
    fn factory() -> Factory<Self>;
}

/// A child factory with either a single result or an explicit batch size.
pub enum FactoryChildren<R: Model> {
    Uncounted(Factory<R>),
    Counted(FactoryBatch<R>),
}

impl<R: Model> From<Factory<R>> for FactoryChildren<R> {
    fn from(factory: Factory<R>) -> Self {
        Self::Uncounted(factory)
    }
}

impl<R: Model> From<FactoryBatch<R>> for FactoryChildren<R> {
    fn from(factory: FactoryBatch<R>) -> Self {
        Self::Counted(factory)
    }
}

/// Selects generated or already persisted models for a factory attachment.
pub enum FactoryAttachment<R: Model> {
    Generated(Box<FactoryBatch<R>>),
    Uncounted(Box<Factory<R>>),
    Existing(Vec<R>),
}

impl<R: Model> From<Factory<R>> for FactoryAttachment<R> {
    fn from(factory: Factory<R>) -> Self {
        Self::Uncounted(Box::new(factory))
    }
}

impl<R: Model> From<FactoryBatch<R>> for FactoryAttachment<R> {
    fn from(factory: FactoryBatch<R>) -> Self {
        Self::Generated(Box::new(factory))
    }
}

impl<R: Model> From<R> for FactoryAttachment<R> {
    fn from(model: R) -> Self {
        Self::Existing(vec![model])
    }
}

impl<R: Model> From<Vec<R>> for FactoryAttachment<R> {
    fn from(models: Vec<R>) -> Self {
        Self::Existing(models)
    }
}

impl<R: Model> From<Collection<R>> for FactoryAttachment<R> {
    fn from(models: Collection<R>) -> Self {
        Self::Existing(models.into_iter().collect())
    }
}

/// Supplies pivot attributes to each model created by an attached factory.
pub enum FactoryPivot<M: Model> {
    Shared(Attributes),
    Sequence(Vec<Attributes>),
    Resolve(Box<PivotResolver<M>>),
}

impl<M: Model> FactoryPivot<M> {
    pub fn resolve(
        callback: impl Fn(&M) -> DatabaseResult<Attributes> + Send + Sync + 'static,
    ) -> Self {
        Self::Resolve(Box::new(callback))
    }
}

impl<M: Model> From<Attributes> for FactoryPivot<M> {
    fn from(attributes: Attributes) -> Self {
        Self::Shared(attributes)
    }
}

impl<M: Model> From<Vec<Attributes>> for FactoryPivot<M> {
    fn from(attributes: Vec<Attributes>) -> Self {
        Self::Sequence(attributes)
    }
}

/// The position and length seen by an entry in a factory sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FactorySequencePosition {
    pub index: usize,
    pub count: usize,
}

type SequenceResolver = dyn Fn(FactorySequencePosition, &Attributes, Option<&FactoryParentModel>) -> Attributes
    + Send
    + Sync;

/// An attribute map or a contextual callback in a factory sequence.
pub struct FactorySequence(SequenceEntry);

enum SequenceEntry {
    Attributes(Attributes),
    Resolve(Box<SequenceResolver>),
}

impl FactorySequence {
    /// Creates an entry that can read the sequence, current attributes and typed parent.
    pub fn resolve<P: Model>(
        callback: impl Fn(FactorySequencePosition, &Attributes, Option<&P>) -> Attributes
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(SequenceEntry::Resolve(Box::new(
            move |position, attributes, parent| {
                callback(
                    position,
                    attributes,
                    parent.and_then(|parent| parent.downcast_ref::<P>()),
                )
            },
        )))
    }
}

impl From<Attributes> for FactorySequence {
    fn from(attributes: Attributes) -> Self {
        Self(SequenceEntry::Attributes(attributes))
    }
}

/// Selects the records to create with an Eloquent factory.
#[derive(Default)]
pub enum FactoryRecords {
    #[default]
    Default,
    Count(usize),
    Attributes(Vec<Attributes>),
}

impl FactoryRecords {
    fn into_attributes(self, default_count: usize) -> impl Iterator<Item = Attributes> {
        let (count, attributes) = match self {
            Self::Default => (default_count, Vec::new()),
            Self::Count(count) => (count, Vec::new()),
            Self::Attributes(attributes) => (0, attributes),
        };
        attributes
            .into_iter()
            .chain(std::iter::repeat_with(Attributes::new).take(count))
    }
}

// Both factory variants use the same builder transformations.
macro_rules! conditional_states {
    () => {
        /// Transforms this factory when the condition is true.
        #[must_use]
        pub fn when(self, condition: bool, transform: impl FnOnce(Self) -> Self) -> Self {
            self.when_else(condition, transform, core::convert::identity)
        }

        /// Transforms this factory when the condition is false.
        #[must_use]
        pub fn unless(self, condition: bool, transform: impl FnOnce(Self) -> Self) -> Self {
            self.when(!condition, transform)
        }

        /// Applies either the transformation or the fallback.
        #[must_use]
        pub fn when_else(
            self,
            condition: bool,
            transform: impl FnOnce(Self) -> Self,
            fallback: impl FnOnce(Self) -> Self,
        ) -> Self {
            if condition {
                transform(self)
            } else {
                fallback(self)
            }
        }

        /// Applies the transformation on false and the fallback on true.
        #[must_use]
        pub fn unless_else(
            self,
            condition: bool,
            transform: impl FnOnce(Self) -> Self,
            fallback: impl FnOnce(Self) -> Self,
        ) -> Self {
            self.when_else(!condition, transform, fallback)
        }
    };
}

static EXPAND_RELATIONSHIPS_BY_DEFAULT: AtomicBool = AtomicBool::new(true);

/// A model definition with ordered attribute states and relationship callbacks.
pub struct Factory<M: Model> {
    definition: Box<Definition>,
    connection: Option<String>,
    states: Vec<Box<State>>,
    recycled: Vec<RecycledModel>,
    parents: Vec<Box<Parent<M>>>,
    deferred_attributes: Vec<DeferredAttribute>,
    expand_relationships: bool,
    excluded_parents: Vec<FactoryParentSelector>,
    children: Vec<Box<Relationship<M>>>,
    after_making: Vec<Box<Callback<M>>>,
    after_creating: Vec<Box<CreateCallback<M>>>,
}

impl<M: Model> Factory<M> {
    conditional_states!();

    pub fn new(definition: impl Fn() -> Attributes + Send + Sync + 'static) -> Self {
        Self::from_definition(move |_, _| Ok(definition()))
    }

    /// Defines model attributes using the application's configured Faker locale.
    pub fn with_faker(definition: impl Fn(&Faker) -> Attributes + Send + Sync + 'static) -> Self {
        Self::try_with_faker(move |faker| Ok(definition(faker)))
    }

    /// Defines model attributes with a generator that can return an error.
    pub fn try_with_faker(
        definition: impl Fn(&Faker) -> DatabaseResult<Attributes> + Send + Sync + 'static,
    ) -> Self {
        Self::from_definition(move |connection, faker| {
            if faker.get().is_none() {
                let _ = faker.set(Faker::for_connection(connection)?);
            }
            definition(faker.get().expect("factory generator initialized"))
        })
    }

    fn from_definition(
        definition: impl Fn(&Connection, &std::sync::OnceLock<Arc<Faker>>) -> DatabaseResult<Attributes>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            definition: Box::new(definition),
            connection: None,
            states: Vec::new(),
            recycled: Vec::new(),
            parents: Vec::new(),
            deferred_attributes: Vec::new(),
            expand_relationships: EXPAND_RELATIONSHIPS_BY_DEFAULT.load(Ordering::Relaxed),
            excluded_parents: Vec::new(),
            children: Vec::new(),
            after_making: Vec::new(),
            after_creating: Vec::new(),
        }
    }

    /// Selects the connection used by operations without an explicit connection argument.
    #[must_use]
    pub fn connection(mut self, name: Option<&str>) -> Self {
        self.connection = name.map(str::to_owned);
        self
    }

    /// Builds the factory explicitly bound to this model by the application.
    pub fn factory_for_model() -> Self
    where
        M: HasFactory,
    {
        M::factory()
    }

    /// The Rust type name of the model bound to this factory.
    pub fn model_name(&self) -> &'static str {
        type_name::<M>()
    }

    /// Constructs an unsaved model from caller attributes using normal mass assignment.
    pub fn new_model(&self, attributes: Attributes) -> DatabaseResult<M> {
        M::new_instance(attributes)
    }

    pub fn get_connection_name(&self) -> Option<&str> {
        self.connection.as_deref()
    }

    fn resolve_connection(&self) -> DatabaseResult<Connection> {
        Db::connection(self.get_connection_name().or(M::metadata().connection))
    }

    fn resolve_related_connection(&self, inherited: &Connection) -> DatabaseResult<Connection> {
        let name = self.get_connection_name().or(M::metadata().connection);
        let Some(name) = name.filter(|name| *name != inherited.get_name()) else {
            return Ok(inherited.clone());
        };
        match Connection::model_container(Some(inherited)) {
            Some(container) => container
                .get::<DatabaseManager>("db")?
                .connection(Some(name)),
            None => Db::connection(Some(name)),
        }
    }

    /// Adds a persisted model to the pool used when resolving factory owners.
    #[must_use]
    pub fn recycle<R: Model>(self, model: R) -> Self {
        self.recycle_many([model])
    }

    /// Adds several persisted models of one type to the recycle pool.
    #[must_use]
    pub fn recycle_many<R: Model>(mut self, models: impl IntoIterator<Item = R>) -> Self {
        self.recycled.extend(models.into_iter().filter_map(|model| {
            if !model.state().exists() {
                return None;
            }
            Some(RecycledModel {
                connection: model.state().connection.clone()?,
                model: Arc::new(model),
            })
        }));
        self
    }

    /// Selects a recycled model of the requested type without accessing the database.
    pub fn get_random_recycled_model<R: Model>(&self) -> Option<&R> {
        recycled_owner::<R>(&self.recycled, None)?
            .model
            .downcast_ref()
    }

    fn recycle_pool(&self, inherited: &[RecycledModel]) -> Vec<RecycledModel> {
        inherited.iter().chain(&self.recycled).cloned().collect()
    }

    #[must_use]
    pub fn state(
        mut self,
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.states
            .push(Box::new(move |attributes, _, _| Ok(state(attributes))));
        self
    }

    /// Appends a state that can read the enclosing relationship factory's parent.
    #[must_use]
    pub fn state_with_parent<P: Model>(
        mut self,
        state: impl Fn(&Attributes, Option<&P>) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.states.push(Box::new(move |attributes, _, parent| {
            Ok(state(
                attributes,
                parent.and_then(|model| model.downcast_ref::<P>()),
            ))
        }));
        self
    }

    /// Runs this transformation before the factory's existing states.
    #[must_use]
    pub fn prepend_state(
        mut self,
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.states
            .insert(0, Box::new(move |attributes, _, _| Ok(state(attributes))));
        self
    }

    /// Appends a state that sets one attribute.
    #[must_use]
    pub fn set(self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        let name = name.into();
        let value = value.into();
        self.state(move |_| Attributes::from([(name.clone(), value.clone())]))
    }

    /// Appends the soft-deleted state using the supplied date or the previous local day.
    pub fn trashed(mut self, deleted_at: Option<Value>) -> DatabaseResult<Self> {
        let column = M::metadata()
            .deleted_at
            .ok_or_else(soft_deletes::unsupported)?;
        if let Some(value) = deleted_at.filter(|value| !matches!(value, Value::Null)) {
            return Ok(self.set(column, value));
        }

        let now = jiff::Timestamp::now();
        self.states.push(Box::new(move |_, connection, _| {
            let zone = connection.model_timezone.clone();
            let (_, yesterday) = calendar::date_interval("now", "-1 day", now, zone.clone())
                .map_err(|_| DatabaseError::InvalidQuery("invalid factory deletion timestamp"))?;
            let value = casts::date::format_for_storage(
                yesterday.to_zoned(zone),
                M::metadata().date_format,
            )?;
            Ok(Attributes::from([(column.to_owned(), value)]))
        }));
        Ok(self)
    }

    #[must_use]
    pub fn sequence(self, sequence: impl Fn(usize) -> Attributes + Send + Sync + 'static) -> Self {
        self.sequence_with_state(move |index, _| sequence(index))
    }

    /// Appends a sequence that can read the attributes assembled by earlier states.
    #[must_use]
    pub fn sequence_with_state(
        self,
        sequence: impl Fn(usize, &Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.sequence_with_parent::<M>(move |index, attributes, _| sequence(index, attributes))
    }

    /// Appends a sequence that can read current attributes and a typed parent.
    #[must_use]
    pub fn sequence_with_parent<P: Model>(
        self,
        sequence: impl Fn(usize, &Attributes, Option<&P>) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        let index = AtomicUsize::new(0);
        self.state_with_parent(move |attributes, parent| {
            sequence(index.fetch_add(1, Ordering::Relaxed), attributes, parent)
        })
    }

    /// Cycles through attribute maps and contextual callbacks in declaration order.
    #[must_use]
    pub fn sequence_entries(mut self, entries: Vec<FactorySequence>) -> Self {
        let count = entries.len();
        if count == 0 {
            return self;
        }
        let index = AtomicUsize::new(0);
        self.states.push(Box::new(move |attributes, _, parent| {
            let index = index.fetch_add(1, Ordering::Relaxed);
            Ok(match &entries[index % count].0 {
                SequenceEntry::Attributes(values) => values.clone(),
                SequenceEntry::Resolve(callback) => {
                    callback(FactorySequencePosition { index, count }, attributes, parent)
                }
            })
        }));
        self
    }

    /// Sets the pending batch size to the number of mixed sequence entries.
    pub fn for_each_sequence_entries(self, entries: Vec<FactorySequence>) -> FactoryBatch<M> {
        let count = entries.len();
        self.sequence_entries(entries).count(count)
    }

    /// Sets a sequence of attribute maps and makes the batch as long as the sequence.
    pub fn for_each_sequence(self, states: Vec<Attributes>) -> FactoryBatch<M> {
        self.for_each_sequence_entries(states.into_iter().map(FactorySequence::from).collect())
    }

    /// Cycles through combinations in declaration order without allocating the product.
    pub fn cross_join_sequence(self, sequences: Vec<Vec<Attributes>>) -> DatabaseResult<Self> {
        if sequences.iter().any(Vec::is_empty) {
            return Err(DatabaseError::InvalidQuery(
                "factory cross join sequence is empty",
            ));
        }
        Ok(self.sequence(move |mut index| {
            let selected = sequences.iter().rev().map(|sequence| {
                let state = &sequence[index % sequence.len()];
                index /= sequence.len();
                state
            });
            let mut attributes = Attributes::new();
            for state in selected {
                for (name, value) in state {
                    attributes
                        .entry(name.clone())
                        .or_insert_with(|| value.clone());
                }
            }
            attributes
        }))
    }

    pub fn count(self, count: usize) -> FactoryBatch<M> {
        FactoryBatch {
            factory: self,
            count: AtomicUsize::new(count),
        }
    }

    #[must_use]
    pub fn after_making(
        mut self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.after_making.push(Box::new(callback));
        self
    }

    #[must_use]
    pub fn after_creating(
        mut self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.after_creating
            .push(Box::new(move |model, _| callback(model)));
        self
    }

    /// Registers a creation callback that can read the enclosing relationship factory's parent.
    #[must_use]
    pub fn after_creating_with_parent<P: Model>(
        mut self,
        callback: impl Fn(&mut M, Option<&P>) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.after_creating.push(Box::new(move |model, parent| {
            callback(model, parent.and_then(|model| model.downcast_ref::<P>()))
        }));
        self
    }

    /// Removes the current construction callbacks from this factory.
    #[must_use]
    pub fn without_after_making(mut self) -> Self {
        self.after_making.clear();
        self
    }

    /// Removes the current post-creation callbacks from this factory.
    #[must_use]
    pub fn without_after_creating(mut self) -> Self {
        self.after_creating.clear();
        self
    }

    #[must_use]
    pub fn has<R: Model>(
        self,
        relation: impl FactoryChild<M, R>,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        let relation = Arc::new(relation);
        self.has_resolved(move |_| Ok(relation.clone()), children.into())
    }

    /// Resolves a registered model relationship by name and Rust descriptor type.
    #[must_use]
    pub fn has_named<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
        self,
        name: impl Into<String>,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        let name = name.into();
        self.has_resolved(
            move |model| named_descriptor::<M, D>(model, &name).map(|relation| Arc::new(relation)),
            children.into(),
        )
    }

    /// Infers a child relationship from the related model type.
    #[must_use]
    pub fn has_inferred<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
        self,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        let plural = inferred_relation_name::<R>(2);
        let name = if M::named_relations().contains(&plural) {
            plural
        } else {
            pluralizer::pluralize(&plural, 1, false)
        };
        self.has_named::<R, D>(name, children)
    }

    fn has_resolved<R: Model, D: FactoryChild<M, R>>(
        mut self,
        relation: impl Fn(&M) -> DatabaseResult<Arc<D>> + Send + Sync + 'static,
        children: FactoryChildren<R>,
    ) -> Self {
        let relation = Arc::new((relation, children));
        self.children
            .push(Box::new(move |connection, parent, inherited| {
                let relation = Arc::clone(&relation);
                Box::pin(async move {
                    let (relation, children) = &*relation;
                    let relation = relation(parent)?;
                    let factory = match children {
                        FactoryChildren::Uncounted(factory) => factory,
                        FactoryChildren::Counted(batch) => &batch.factory,
                    };
                    let connection = factory.resolve_related_connection(connection)?;
                    let pool = factory.recycle_pool(inherited);
                    let attributes = relation.attributes(parent)?;
                    let context = Some(RelationContext {
                        attributes: Some(&attributes),
                        parent,
                    });
                    let mut single;
                    let mut batch;
                    let models = match children {
                        FactoryChildren::Uncounted(factory) => {
                            single = factory
                                .make_one_prepared(
                                    &connection,
                                    &pool,
                                    &std::sync::OnceLock::new(),
                                    context,
                                    |_| Attributes::new(),
                                )
                                .await?;
                            std::slice::from_mut(&mut single)
                        }
                        FactoryChildren::Counted(children) => {
                            batch = children.make_with_pool(&connection, &pool, context).await?;
                            batch.models_mut()
                        }
                    };
                    for child in &mut *models {
                        if !relation.save(parent, child).await? {
                            return Err(DatabaseError::InvalidQuery(
                                "factory child creation cancelled",
                            ));
                        }
                        factory.create_children(&connection, child, &pool).await?;
                    }
                    factory.run_after_creating(models, Some(parent))?;
                    Ok(())
                })
            }));
        self
    }

    #[must_use]
    pub fn r#for<R: Model>(self, relation: impl FactoryParent<M, R>, owner: Factory<R>) -> Self {
        let relation = Arc::new(relation);
        self.for_resolved(move |_| Ok(relation.clone()), owner)
    }

    /// Resolves a registered model relationship by name and Rust descriptor type.
    #[must_use]
    pub fn for_named<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        name: impl Into<String>,
        owner: Factory<R>,
    ) -> Self {
        let name = name.into();
        self.for_resolved(
            move |model| named_descriptor::<M, D>(model, &name).map(|relation| Arc::new(relation)),
            owner,
        )
    }

    /// Infers the owner relationship from the owner model type.
    #[must_use]
    pub fn for_inferred<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        owner: Factory<R>,
    ) -> Self {
        self.for_named::<R, D>(inferred_relation_name::<R>(1), owner)
    }

    fn for_resolved<R: Model, D: FactoryParent<M, R>>(
        mut self,
        relation: impl Fn(&M) -> DatabaseResult<Arc<D>> + Send + Sync + 'static,
        owner: Factory<R>,
    ) -> Self {
        let owner = Arc::new(owner);
        let resolved = Arc::new(ResolvedParents::<R>::new());
        self.parents.push(Box::new(move |model| {
            let relation = relation(model)?;
            let mut prototype = R::from_state(Default::default());
            prototype.state_mut().connection = model.state().connection.clone();
            let foreign_key = relation.factory_foreign_key().map(str::to_owned);
            let morph_type = relation
                .factory_morph_type(&prototype)?
                .map(|(name, alias)| (name.to_owned(), alias.into()));
            let owner = Arc::clone(&owner);
            let resolved = Arc::clone(&resolved);
            Ok(PreparedParent {
                foreign_key,
                morph_type,
                resolve: Box::new(move |connection, model, inherited| {
                    let relation = Arc::clone(&relation);
                    let owner = Arc::clone(&owner);
                    let resolved = Arc::clone(&resolved);
                    Box::pin(async move {
                        let owner_connection = owner.resolve_related_connection(connection)?;
                        let cache = resolved.for_connection(connection, &owner_connection);
                        let owner = cache
                            .get_or_try_init(|| async {
                                let pool = owner.recycle_pool(inherited);
                                if let Some(entry) =
                                    recycled_owner::<R>(&pool, Some(&owner_connection))
                                {
                                    return Ok(Arc::clone(&entry.model)
                                        .downcast::<R>()
                                        .expect("factory owner type"));
                                }
                                let model = owner
                                    .create_one_prepared(
                                        &owner_connection,
                                        &pool,
                                        &std::sync::OnceLock::new(),
                                        None,
                                        |_| Attributes::new(),
                                    )
                                    .await?;
                                Ok::<_, DatabaseError>(Arc::new(model))
                            })
                            .await?;
                        relation.associate(model, owner)
                    })
                }),
            })
        }));
        self
    }

    /// Computes an attribute from the current values after states and overrides.
    #[must_use]
    pub fn attribute(
        mut self,
        name: impl Into<String>,
        resolve: impl Fn(&Attributes) -> DatabaseResult<Value> + Send + Sync + 'static,
    ) -> Self {
        self.set_deferred_attribute(
            name.into(),
            None,
            Box::new(move |values, _, _| Box::pin(std::future::ready(resolve(values)))),
        );
        self
    }

    /// Projects an existing model attribute to its primary key.
    #[must_use]
    pub fn model_attribute<R: Model>(self, name: impl Into<String>, model: &R) -> Self {
        let key = model.get_key().cloned().unwrap_or(Value::Null);
        self.attribute(name, move |_| Ok(key.clone()))
    }

    /// Declares a factory-valued foreign key in the model definition.
    #[must_use]
    pub fn parent_attribute<R: Model>(self, name: impl Into<String>, owner: Factory<R>) -> Self {
        let owner = Arc::new(owner);
        self.set_parent_attribute(name.into(), move |_| Arc::clone(&owner))
    }

    /// Computes a factory-valued foreign key from the current attribute values.
    #[must_use]
    pub fn parent_attribute_with<R: Model>(
        self,
        name: impl Into<String>,
        owner: impl Fn(&Attributes) -> Factory<R> + Send + Sync + 'static,
    ) -> Self {
        self.set_parent_attribute(name.into(), move |values| Arc::new(owner(values)))
    }

    fn set_deferred_attribute(
        &mut self,
        name: String,
        model: Option<TypeId>,
        resolve: Box<DeferredAttributeResolver>,
    ) {
        let attribute = DeferredAttribute {
            name,
            model,
            resolve,
        };
        if let Some(existing) = self
            .deferred_attributes
            .iter_mut()
            .find(|existing| existing.name == attribute.name)
        {
            *existing = attribute;
        } else {
            self.deferred_attributes.push(attribute);
        }
    }

    fn set_parent_attribute<R: Model>(
        mut self,
        name: String,
        owner: impl Fn(&Attributes) -> Arc<Factory<R>> + Send + Sync + 'static,
    ) -> Self {
        self.set_deferred_attribute(
            name,
            Some(TypeId::of::<R>()),
            Box::new(move |values, connection, inherited| {
                let owner = owner(values);
                Box::pin(async move {
                    let connection = owner.resolve_related_connection(connection)?;
                    let pool = owner.recycle_pool(inherited);
                    let existing = recycled_owner::<R>(&pool, Some(&connection));
                    let generated;
                    let related = if let Some(existing) = &existing {
                        existing
                            .model
                            .downcast_ref::<R>()
                            .expect("factory owner type")
                    } else {
                        generated = owner
                            .create_one_prepared(
                                &connection,
                                &pool,
                                &std::sync::OnceLock::new(),
                                None,
                                |_| Attributes::new(),
                            )
                            .await?;
                        &generated
                    };
                    related
                        .get_key()
                        .filter(|key| !matches!(key, Value::Null))
                        .cloned()
                        .ok_or(DatabaseError::InvalidQuery("factory related primary key"))
                })
            }),
        );
        self
    }

    /// Enables factory-valued parent expansion on newly constructed factories.
    pub fn expand_relationships_by_default() {
        EXPAND_RELATIONSHIPS_BY_DEFAULT.store(true, Ordering::Relaxed);
    }

    /// Disables factory-valued parent expansion on newly constructed factories.
    pub fn dont_expand_relationships_by_default() {
        EXPAND_RELATIONSHIPS_BY_DEFAULT.store(false, Ordering::Relaxed);
    }

    /// Restores the process-wide factory defaults.
    pub fn flush_state() {
        Self::expand_relationships_by_default();
    }

    /// Excludes the selected factory-valued attributes, or all when the list is empty.
    #[must_use]
    pub fn without_parents(mut self, parents: Vec<FactoryParentSelector>) -> Self {
        if parents.is_empty() {
            self.expand_relationships = false;
        } else {
            self.excluded_parents = parents;
        }
        self
    }

    #[must_use]
    pub fn for_existing<R: Model>(self, relation: impl FactoryParent<M, R>, owner: R) -> Self {
        let relation = Arc::new(relation);
        self.for_existing_resolved(move |_| Ok(Arc::clone(&relation)), owner)
    }

    /// Associates an existing model using a registered relationship name and descriptor type.
    #[must_use]
    pub fn for_existing_named<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        name: impl Into<String>,
        owner: R,
    ) -> Self {
        let name = name.into();
        self.for_existing_resolved(
            move |model| named_descriptor::<M, D>(model, &name).map(Arc::new),
            owner,
        )
    }

    /// Infers the relationship to an existing owner from its model type.
    #[must_use]
    pub fn for_existing_inferred<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        owner: R,
    ) -> Self {
        self.for_existing_named::<R, D>(inferred_relation_name::<R>(1), owner)
    }

    fn for_existing_resolved<R: Model, D: FactoryParent<M, R>>(
        mut self,
        relation: impl Fn(&M) -> DatabaseResult<Arc<D>> + Send + Sync + 'static,
        owner: R,
    ) -> Self {
        let owner = Arc::new(owner);
        self.parents.push(Box::new(move |model| {
            let relation = relation(model)?;
            let foreign_key = relation.factory_foreign_key().map(str::to_owned);
            let morph_type = relation
                .factory_morph_type(&owner)?
                .map(|(name, alias)| (name.to_owned(), alias.into()));
            let owner = Arc::clone(&owner);
            Ok(PreparedParent {
                foreign_key,
                morph_type,
                resolve: Box::new(move |_, model, _| {
                    let relation = Arc::clone(&relation);
                    let owner = Arc::clone(&owner);
                    Box::pin(async move { relation.associate(model, &owner) })
                }),
            })
        }));
        self
    }

    #[must_use]
    pub fn has_attached<R: Model>(
        self,
        relation: impl Into<BelongsToMany<M, R>>,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        let relation = Arc::new(relation.into());
        self.has_attached_resolved(
            move |_| Ok(relation.clone()),
            models.into(),
            pivot.into(),
            None,
        )
    }

    /// Resolves a registered model relationship by name and Rust descriptor type.
    #[must_use]
    pub fn has_attached_named<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
        self,
        name: impl Into<String>,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        let name = name.into();
        let resolve_name = name.clone();
        self.has_attached_resolved(
            move |model| {
                named_descriptor::<M, D>(model, &resolve_name)
                    .map(|relation| Arc::new(relation.into()))
            },
            models.into(),
            pivot.into(),
            Some(name),
        )
    }

    /// Infers the plural attachment relationship from the related model type.
    #[must_use]
    pub fn has_attached_inferred<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
        self,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        self.has_attached_named::<R, D>(inferred_relation_name::<R>(2), models, pivot)
    }

    fn has_attached_resolved<R: Model>(
        mut self,
        relation: impl Fn(&M) -> DatabaseResult<Arc<BelongsToMany<M, R>>> + Send + Sync + 'static,
        models: FactoryAttachment<R>,
        pivot: FactoryPivot<M>,
        name: Option<String>,
    ) -> Self {
        let models = match models {
            FactoryAttachment::Uncounted(factory) => {
                let count = match &pivot {
                    FactoryPivot::Sequence(states) if !states.is_empty() => states.len(),
                    _ => 1,
                };
                FactoryAttachment::from(factory.count(count))
            }
            models => models,
        };
        let relation = Arc::new((relation, models, pivot, AtomicUsize::new(0), name));
        self.children
            .push(Box::new(move |connection, parent, inherited| {
                let relation = Arc::clone(&relation);
                Box::pin(async move {
                    let (relation, models, pivot, pivot_index, name) = &*relation;
                    let relation = relation(parent)?;
                    let generated;
                    let models: &[R] = match models {
                        FactoryAttachment::Generated(factory) => {
                            let connection =
                                factory.factory.resolve_related_connection(connection)?;
                            let attributes =
                                (relation.factory_defaults(&connection)?, Attributes::new());
                            generated = factory
                                .create_with_pool(
                                    &connection,
                                    inherited,
                                    Some(RelationContext {
                                        attributes: Some(&attributes),
                                        parent,
                                    }),
                                )
                                .await?;
                            &generated
                        }
                        FactoryAttachment::Uncounted(_) => {
                            unreachable!("attachment count resolved during registration")
                        }
                        FactoryAttachment::Existing(models) => {
                            relation.validate_factory_attachments(parent, models)?;
                            models
                        }
                    };
                    let keys = models
                        .iter()
                        .map(|model| {
                            model
                                .get_attribute(relation.related_key())
                                .filter(|key| !matches!(key, Value::Null))
                                .cloned()
                                .ok_or(DatabaseError::InvalidQuery("factory related primary key"))
                        })
                        .collect::<DatabaseResult<Vec<_>>>()?;
                    if let FactoryPivot::Shared(attributes) = pivot {
                        relation
                            .attach_with(parent, &keys, attributes.clone())
                            .await?;
                    } else {
                        for key in &keys {
                            let attributes = match pivot {
                                FactoryPivot::Shared(_) => unreachable!(),
                                FactoryPivot::Sequence(states) => pivot_index
                                    .fetch_add(1, Ordering::Relaxed)
                                    .checked_rem(states.len())
                                    .map(|offset| states[offset].clone())
                                    .unwrap_or_default(),
                                FactoryPivot::Resolve(resolve) => resolve(parent)?,
                            };
                            relation
                                .attach_with(parent, std::slice::from_ref(key), attributes)
                                .await?;
                        }
                    }
                    if let Some(name) = name
                        && !keys.is_empty()
                        && (parent.relation_loaded(name) || relation.touches_related(parent)?)
                    {
                        parent.load(name, relation.as_ref()).await?;
                    }
                    Ok(())
                })
            }));
        self
    }

    pub async fn make(&self) -> DatabaseResult<M> {
        self.make_on(&self.resolve_connection()?).await
    }

    pub async fn make_on(&self, connection: &Connection) -> DatabaseResult<M> {
        self.make_one_on(connection, Attributes::new()).await
    }

    pub async fn make_one(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.make_one_on(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn make_one_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.make_with_state_on(connection, |_| attributes).await
    }

    /// Applies a final state to the accumulated attributes for this operation.
    pub async fn make_with_state(
        &self,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.make_with_state_on(&self.resolve_connection()?, state)
            .await
    }

    pub async fn make_with_state_on(
        &self,
        connection: &Connection,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.make_one_prepared(
            connection,
            &self.recycled,
            &std::sync::OnceLock::new(),
            None,
            state,
        )
        .await
    }

    /// Constructs an unsaved model with a parent available to factory states.
    pub async fn make_with_parent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.make_with_parent_on(&self.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn make_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.make_with_parent_state_on(connection, parent, |_, _| attributes)
            .await
    }

    /// Builds unsaved models with a final state that can read the parent.
    pub async fn make_with_parent_state<P: Model>(
        &self,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.make_with_parent_state_on(&self.resolve_connection()?, parent, state)
            .await
    }

    pub async fn make_with_parent_state_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.make_one_prepared(
            connection,
            &self.recycled,
            &std::sync::OnceLock::new(),
            Some(RelationContext::parent_only(parent)),
            |attributes| state(attributes, parent),
        )
        .await
    }

    pub async fn make_many(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.make_many_on(&self.resolve_connection()?, records)
            .await
    }

    pub async fn make_many_on(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.make_many_prepared(connection, records, 1).await
    }

    async fn make_many_prepared(
        &self,
        connection: &Connection,
        records: FactoryRecords,
        default_count: usize,
    ) -> DatabaseResult<Collection<M>> {
        let faker = std::sync::OnceLock::new();
        let mut models = Vec::new();
        for attributes in records.into_attributes(default_count) {
            models.push(
                self.make_one_prepared(connection, &self.recycled, &faker, None, |_| attributes)
                    .await?,
            );
        }
        Ok(Collection::new(models).without_relationship_autoloading())
    }

    /// Returns a reusable callback that creates the model when awaited.
    pub fn lazy<'a>(
        &'a self,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<M>> + 'a {
        move || Box::pin(self.create_one(attributes.clone()))
    }

    /// Creates models with a parent available to states and afterCreating callbacks.
    pub async fn create_with_parent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.create_with_parent_on(&self.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn create_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.create_with_parent_state_on(connection, parent, |_, _| attributes)
            .await
    }

    /// Persists models with a final state that can read the parent.
    pub async fn create_with_parent_state<P: Model>(
        &self,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.create_with_parent_state_on(&self.resolve_connection()?, parent, state)
            .await
    }

    pub async fn create_with_parent_state_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.create_one_prepared(
            connection,
            &self.recycled,
            &std::sync::OnceLock::new(),
            Some(RelationContext::parent_only(parent)),
            |attributes| state(attributes, parent),
        )
        .await
    }

    /// Creates models with parent context while suppressing Eloquent events.
    pub async fn create_quietly_with_parent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        M::without_events(self.create_with_parent(parent, attributes)).await
    }

    pub async fn create_quietly_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        M::without_events(self.create_with_parent_on(connection, parent, attributes)).await
    }

    /// Applies a final parent-aware state while suppressing model events.
    pub async fn create_quietly_with_parent_state<P: Model>(
        &self,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.create_quietly_with_parent_state_on(&self.resolve_connection()?, parent, state)
            .await
    }

    pub async fn create_quietly_with_parent_state_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        M::without_events(self.create_with_parent_state_on(connection, parent, state)).await
    }

    /// Returns a reusable callback borrowing the parent until invocation.
    pub fn lazy_with_parent<'a, P: Model>(
        &'a self,
        parent: &'a P,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<M>> + 'a {
        move || Box::pin(self.create_with_parent(parent, attributes.clone()))
    }

    pub async fn create(&self) -> DatabaseResult<M> {
        self.create_on(&self.resolve_connection()?).await
    }

    /// Inserts the generated model without saving relations or firing model persistence events.
    pub async fn insert(&self, attributes: Attributes) -> DatabaseResult<()> {
        self.insert_on(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn insert_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let model = self.make_one_on(connection, attributes).await?;
        Self::insert_models(connection, vec![model]).await
    }

    /// Inserts a model constructed with a parent available to factory states.
    pub async fn insert_with_parent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        self.insert_with_parent_on(&self.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn insert_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let model = self
            .make_with_parent_on(connection, parent, attributes)
            .await?;
        Self::insert_models(connection, vec![model]).await
    }

    async fn insert_models(connection: &Connection, models: Vec<M>) -> DatabaseResult<()> {
        let rows = models
            .into_iter()
            .map(|mut model| {
                model.set_hidden(&[]);
                super::serialization::projected_attributes(&model)
                    .map(|(name, value)| Ok((name.clone(), value?)))
                    .collect::<DatabaseResult<Attributes>>()
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        M::query_on(connection)
            .without_global_scopes()
            .fill_and_insert(rows)
            .await?;
        Ok(())
    }

    pub async fn create_on(&self, connection: &Connection) -> DatabaseResult<M> {
        self.create_one_on(connection, Attributes::new()).await
    }

    pub async fn create_one(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.create_one_on(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn create_one_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.create_with_state_on(connection, |_| attributes).await
    }

    /// Applies a final state to the accumulated attributes for this operation.
    pub async fn create_with_state(
        &self,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.create_with_state_on(&self.resolve_connection()?, state)
            .await
    }

    pub async fn create_with_state_on(
        &self,
        connection: &Connection,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.create_one_prepared(
            connection,
            &self.recycled,
            &std::sync::OnceLock::new(),
            None,
            state,
        )
        .await
    }

    pub async fn create_one_quietly(&self, attributes: Attributes) -> DatabaseResult<M> {
        M::without_events(self.create_one(attributes)).await
    }

    pub async fn create_one_quietly_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        M::without_events(self.create_one_on(connection, attributes)).await
    }

    pub async fn create_many(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.create_many_on(&self.resolve_connection()?, records)
            .await
    }

    pub async fn create_many_on(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.create_many_prepared(connection, records, 1).await
    }

    pub async fn create_many_quietly(
        &self,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_many(records)).await
    }

    pub async fn create_many_quietly_on(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_many_on(connection, records)).await
    }

    async fn create_many_prepared(
        &self,
        connection: &Connection,
        records: FactoryRecords,
        default_count: usize,
    ) -> DatabaseResult<Collection<M>> {
        let faker = std::sync::OnceLock::new();
        let mut models = Vec::new();
        for attributes in records.into_attributes(default_count) {
            models.push(
                self.create_one_prepared(connection, &self.recycled, &faker, None, |_| attributes)
                    .await?,
            );
        }
        Ok(Collection::new(models).without_relationship_autoloading())
    }

    pub async fn create_quietly(&self) -> DatabaseResult<M> {
        M::without_events(self.create()).await
    }

    pub async fn create_quietly_on(&self, connection: &Connection) -> DatabaseResult<M> {
        M::without_events(self.create_on(connection)).await
    }

    /// Applies a final state while suppressing model events.
    pub async fn create_quietly_with_state(
        &self,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        M::without_events(self.create_with_state(state)).await
    }

    pub async fn create_quietly_with_state_on(
        &self,
        connection: &Connection,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        M::without_events(self.create_with_state_on(connection, state)).await
    }

    /// Expands the factory definition and parent keys without applying model casts or callbacks.
    pub async fn raw(&self) -> DatabaseResult<Attributes> {
        self.raw_on(&self.resolve_connection()?).await
    }

    pub async fn raw_on(&self, connection: &Connection) -> DatabaseResult<Attributes> {
        self.raw_with_state_on(connection, |_| Attributes::new())
            .await
    }

    /// Expands raw attributes with a one-time override after the configured states.
    pub async fn raw_with(&self, attributes: Attributes) -> DatabaseResult<Attributes> {
        self.raw_with_on(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn raw_with_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Attributes> {
        self.raw_with_state_on(connection, |_| attributes).await
    }

    /// Computes the last state from the accumulated attributes before expansion.
    pub async fn raw_with_state(
        &self,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Attributes> {
        self.raw_with_state_on(&self.resolve_connection()?, state)
            .await
    }

    pub async fn raw_with_state_on(
        &self,
        connection: &Connection,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Attributes> {
        self.raw_attributes(connection, &std::sync::OnceLock::new(), None, state)
            .await
    }

    /// Expands raw attributes with a parent available to the configured and final states.
    pub async fn raw_with_parent<P: Model>(
        &self,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Attributes> {
        self.raw_with_parent_on(&self.resolve_connection()?, parent, state)
            .await
    }

    pub async fn raw_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Attributes> {
        let context = RelationContext::parent_only(parent);
        self.raw_attributes(
            connection,
            &std::sync::OnceLock::new(),
            Some(context),
            |attributes| state(attributes, parent),
        )
        .await
    }

    async fn raw_attributes(
        &self,
        connection: &Connection,
        faker: &std::sync::OnceLock<Arc<Faker>>,
        context: Option<RelationContext<'_>>,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Attributes> {
        let mut pending = self.prepare_attributes(connection, context, faker)?;
        let attributes = state(&pending.values);
        pending.values.extend(attributes);
        self.resolve_attributes(connection, &self.recycled, &mut pending)
            .await?;
        let mut expanded = pending.model.state().attributes.clone();
        expanded.extend(pending.values);
        Ok(expanded)
    }

    async fn apply_attributes(
        connection: &Connection,
        mut model: M,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        // Keep model-specific assignment hooks, including temporary attributes.
        model.state_mut().connection = Some(connection.clone());
        model.force_fill_async(attributes).await?;
        Ok(model)
    }

    fn prepare_attributes(
        &self,
        connection: &Connection,
        relation: Option<RelationContext<'_>>,
        faker: &std::sync::OnceLock<Arc<Faker>>,
    ) -> DatabaseResult<FactoryAttributes<M>> {
        let mut attributes = (self.definition)(connection, faker)?;
        let mut model = M::from_state(Default::default());
        model.state_mut().connection = Some(connection.clone());
        let mut parents = Vec::with_capacity(self.parents.len());
        for parent in &self.parents {
            let parent = parent(&model)?;
            if let Some(name) = &parent.foreign_key {
                attributes.remove(name);
            }
            if let Some((name, alias)) = &parent.morph_type {
                attributes.insert(name.clone(), alias.clone());
            }
            parents.push(parent);
        }
        if let Some(defaults) = relation.and_then(|context| context.attributes) {
            attributes.extend(defaults.0.clone());
        }
        for state in &self.states {
            let changes = state(
                &attributes,
                connection,
                relation.map(|context| context.parent),
            )?;
            attributes.extend(changes);
        }
        if let Some(defaults) = relation.and_then(|context| context.attributes) {
            attributes.extend(defaults.1.clone());
        }
        Ok(FactoryAttributes {
            model,
            values: attributes,
            parents,
        })
    }

    async fn resolve_attributes(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        pending: &mut FactoryAttributes<M>,
    ) -> DatabaseResult<()> {
        let mut last_parent = BTreeMap::new();
        for (index, parent) in pending.parents.iter().enumerate() {
            if let Some(key) = &parent.foreign_key {
                last_parent.insert(key, index);
            }
        }
        for (index, parent) in pending.parents.iter().enumerate() {
            if let Some(key) = &parent.foreign_key
                && (pending.values.contains_key(key) || last_parent[key] != index)
            {
                continue;
            }
            (parent.resolve)(connection, &mut pending.model, pool).await?;
        }
        self.expand_deferred_attributes(
            connection,
            &pending.model,
            &mut pending.values,
            pool,
            &last_parent,
        )
        .await
    }

    async fn expand_deferred_attributes(
        &self,
        connection: &Connection,
        model: &M,
        values: &mut Attributes,
        pool: &[RecycledModel],
        explicit_parents: &BTreeMap<&String, usize>,
    ) -> DatabaseResult<()> {
        if self.deferred_attributes.is_empty() {
            return Ok(());
        }
        let mut resolved = model.state().attributes.clone();
        resolved.extend(values.clone());
        for attribute in &self.deferred_attributes {
            if values.contains_key(&attribute.name)
                || explicit_parents.contains_key(&attribute.name)
            {
                continue;
            }
            let excluded = attribute.model.is_some()
                && (!self.expand_relationships
                    || self.excluded_parents.iter().any(|selector| match selector {
                        FactoryParentSelector::Attribute(name) => *name == attribute.name,
                        FactoryParentSelector::Model(model) => Some(*model) == attribute.model,
                    }));
            let value = if excluded {
                Value::Null
            } else {
                (attribute.resolve)(&resolved, connection, pool).await?
            };
            resolved.insert(attribute.name.clone(), value.clone());
            values.insert(attribute.name.clone(), value);
        }
        Ok(())
    }

    async fn make_one_prepared(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        faker: &std::sync::OnceLock<Arc<Faker>>,
        context: Option<RelationContext<'_>>,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        without_relationship_autoloading(async {
            let mut pending = self.prepare_attributes(connection, context, faker)?;
            let attributes = state(&pending.values);
            pending.values.extend(attributes);
            self.resolve_attributes(connection, pool, &mut pending)
                .await?;
            let mut model =
                Self::apply_attributes(connection, pending.model, pending.values).await?;
            self.run_callbacks(std::slice::from_mut(&mut model), &self.after_making)?;
            Ok(model)
        })
        .await
    }

    async fn create_one_prepared(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        faker: &std::sync::OnceLock<Arc<Faker>>,
        context: Option<RelationContext<'_>>,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        let mut model = self
            .make_one_prepared(connection, pool, faker, context, state)
            .await?;
        self.store(connection, &mut model, pool).await?;
        self.run_after_creating(
            std::slice::from_mut(&mut model),
            context.map(|context| context.parent),
        )?;
        Ok(model)
    }

    async fn store(
        &self,
        connection: &Connection,
        model: &mut M,
        pool: &[RecycledModel],
    ) -> DatabaseResult<()> {
        if !model.save().await? {
            return Err(DatabaseError::InvalidQuery(
                "factory model creation cancelled",
            ));
        }
        self.create_children(connection, model, pool).await
    }

    fn run_callbacks(
        &self,
        models: &mut [M],
        callbacks: &[Box<Callback<M>>],
    ) -> DatabaseResult<()> {
        for model in models {
            for callback in callbacks {
                callback(model)?;
            }
        }
        Ok(())
    }

    fn run_after_creating(
        &self,
        models: &mut [M],
        parent: Option<&FactoryParentModel>,
    ) -> DatabaseResult<()> {
        for model in models {
            for callback in &self.after_creating {
                callback(model, parent)?;
            }
        }
        Ok(())
    }

    async fn create_children(
        &self,
        connection: &Connection,
        model: &mut M,
        pool: &[RecycledModel],
    ) -> DatabaseResult<()> {
        model.state_mut().relations.remove_empty_collections();
        for child in &self.children {
            child(connection, model, pool).await?;
        }
        Ok(())
    }
}

fn inferred_relation_name<R: Model>(count: isize) -> String {
    let name = type_name::<R>().split('<').next().expect("model type name");
    let basename = name.rsplit("::").next().expect("model type basename");
    if count == 1 {
        Str::camel(basename)
    } else {
        Str::camel(&pluralizer::pluralize(basename, count, false))
    }
}

fn named_descriptor<M: Model, D: Relation<Parent = M>>(model: &M, name: &str) -> DatabaseResult<D> {
    match super::dynamic_relations::RelationResolvers::resolve(model, name)? {
        Some(descriptor) => Ok(descriptor),
        None => M::named_relations().factory_descriptor(name),
    }
}

/// A factory that produces the requested number of models.
pub struct FactoryBatch<M: Model> {
    factory: Factory<M>,
    count: AtomicUsize,
}

impl<M: Model> FactoryBatch<M> {
    conditional_states!();

    fn map_factory(self, transform: impl FnOnce(Factory<M>) -> Factory<M>) -> Self {
        Self {
            factory: transform(self.factory),
            count: self.count,
        }
    }

    /// Replaces the pending batch size.
    #[must_use]
    pub fn count(self, count: usize) -> Self {
        self.factory.count(count)
    }

    #[must_use]
    pub fn sequence(self, sequence: impl Fn(usize) -> Attributes + Send + Sync + 'static) -> Self {
        self.map_factory(|factory| factory.sequence(sequence))
    }

    /// Appends a context-aware sequence without changing the pending count.
    #[must_use]
    pub fn sequence_with_state(
        self,
        sequence: impl Fn(usize, &Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.sequence_with_state(sequence))
    }

    /// Appends a typed parent-aware sequence without changing the pending count.
    #[must_use]
    pub fn sequence_with_parent<P: Model>(
        self,
        sequence: impl Fn(usize, &Attributes, Option<&P>) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.sequence_with_parent(sequence))
    }

    /// Appends a mixed sequence without changing the pending count.
    #[must_use]
    pub fn sequence_entries(self, entries: Vec<FactorySequence>) -> Self {
        self.map_factory(|factory| factory.sequence_entries(entries))
    }

    /// Replaces the pending batch size with the number of mixed entries.
    #[must_use]
    pub fn for_each_sequence_entries(self, entries: Vec<FactorySequence>) -> Self {
        self.factory.for_each_sequence_entries(entries)
    }

    /// Sets the pending batch size to the sequence length.
    #[must_use]
    pub fn for_each_sequence(self, states: Vec<Attributes>) -> Self {
        self.factory.for_each_sequence(states)
    }

    pub fn cross_join_sequence(self, sequences: Vec<Vec<Attributes>>) -> DatabaseResult<Self> {
        Ok(Self {
            factory: self.factory.cross_join_sequence(sequences)?,
            count: self.count,
        })
    }

    #[must_use]
    pub fn after_making(
        self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.after_making(callback))
    }

    #[must_use]
    pub fn after_creating(
        self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.after_creating(callback))
    }

    #[must_use]
    pub fn has<R: Model>(
        self,
        relation: impl FactoryChild<M, R>,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        self.map_factory(|factory| factory.has(relation, children))
    }

    #[must_use]
    pub fn has_named<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
        self,
        name: impl Into<String>,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        self.map_factory(|factory| factory.has_named::<R, D>(name, children))
    }

    #[must_use]
    pub fn r#for<R: Model>(self, relation: impl FactoryParent<M, R>, owner: Factory<R>) -> Self {
        self.map_factory(|factory| factory.r#for(relation, owner))
    }

    #[must_use]
    pub fn for_named<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        name: impl Into<String>,
        owner: Factory<R>,
    ) -> Self {
        self.map_factory(|factory| factory.for_named::<R, D>(name, owner))
    }

    #[must_use]
    pub fn for_existing<R: Model>(self, relation: impl FactoryParent<M, R>, owner: R) -> Self {
        self.map_factory(|factory| factory.for_existing(relation, owner))
    }

    /// Associates an existing model using a registered relationship name and descriptor type.
    #[must_use]
    pub fn for_existing_named<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        name: impl Into<String>,
        owner: R,
    ) -> Self {
        self.map_factory(|factory| factory.for_existing_named::<R, D>(name, owner))
    }

    #[must_use]
    pub fn has_attached<R: Model>(
        self,
        relation: impl Into<BelongsToMany<M, R>>,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        self.map_factory(|factory| factory.has_attached(relation, models, pivot))
    }

    #[must_use]
    pub fn has_attached_named<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
        self,
        name: impl Into<String>,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        self.map_factory(|factory| factory.has_attached_named::<R, D>(name, models, pivot))
    }

    /// Infers a child relationship from the related model type.
    #[must_use]
    pub fn has_inferred<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
        self,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        self.map_factory(|factory| factory.has_inferred::<R, D>(children))
    }

    /// Infers the owner relationship from the owner model type.
    #[must_use]
    pub fn for_inferred<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        owner: Factory<R>,
    ) -> Self {
        self.map_factory(|factory| factory.for_inferred::<R, D>(owner))
    }

    /// Infers the relationship to an existing owner from its model type.
    #[must_use]
    pub fn for_existing_inferred<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        owner: R,
    ) -> Self {
        self.map_factory(|factory| factory.for_existing_inferred::<R, D>(owner))
    }

    /// Infers the plural attachment relationship from the related model type.
    #[must_use]
    pub fn has_attached_inferred<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
        self,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        self.map_factory(|factory| factory.has_attached_inferred::<R, D>(models, pivot))
    }

    #[must_use]
    pub fn attribute(
        self,
        name: impl Into<String>,
        resolve: impl Fn(&Attributes) -> DatabaseResult<Value> + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.attribute(name, resolve))
    }

    #[must_use]
    pub fn model_attribute<R: Model>(self, name: impl Into<String>, model: &R) -> Self {
        self.map_factory(|factory| factory.model_attribute(name, model))
    }

    #[must_use]
    pub fn parent_attribute<R: Model>(self, name: impl Into<String>, owner: Factory<R>) -> Self {
        self.map_factory(|factory| factory.parent_attribute(name, owner))
    }

    #[must_use]
    pub fn parent_attribute_with<R: Model>(
        self,
        name: impl Into<String>,
        owner: impl Fn(&Attributes) -> Factory<R> + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.parent_attribute_with(name, owner))
    }

    #[must_use]
    pub fn without_parents(mut self, parents: Vec<FactoryParentSelector>) -> Self {
        self.factory
            .without_parents(parents)
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn connection(mut self, name: Option<&str>) -> Self {
        self.factory.connection(name).count(*self.count.get_mut())
    }

    /// The Rust type name of the model bound to this factory.
    pub fn model_name(&self) -> &'static str {
        self.factory.model_name()
    }

    /// Constructs an unsaved model from caller attributes using normal mass assignment.
    pub fn new_model(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.new_model(attributes)
    }

    pub fn get_connection_name(&self) -> Option<&str> {
        self.factory.get_connection_name()
    }

    pub fn trashed(mut self, deleted_at: Option<Value>) -> DatabaseResult<Self> {
        Ok(self
            .factory
            .trashed(deleted_at)?
            .count(*self.count.get_mut()))
    }

    #[must_use]
    pub fn state(
        mut self,
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.factory.state(state).count(*self.count.get_mut())
    }

    #[must_use]
    pub fn state_with_parent<P: Model>(
        mut self,
        state: impl Fn(&Attributes, Option<&P>) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.factory
            .state_with_parent(state)
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn after_creating_with_parent<P: Model>(
        mut self,
        callback: impl Fn(&mut M, Option<&P>) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.factory
            .after_creating_with_parent(callback)
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn prepend_state(
        mut self,
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.factory
            .prepend_state(state)
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn set(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.factory.set(name, value).count(*self.count.get_mut())
    }

    #[must_use]
    pub fn without_after_making(mut self) -> Self {
        self.factory
            .without_after_making()
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn without_after_creating(mut self) -> Self {
        self.factory
            .without_after_creating()
            .count(*self.count.get_mut())
    }

    pub async fn make_one(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.make_one(attributes).await
    }

    pub async fn make_one_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory.make_one_on(connection, attributes).await
    }

    pub async fn make_many(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.make_many_on(&self.factory.resolve_connection()?, records)
            .await
    }

    pub async fn make_many_on(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.factory
            .make_many_prepared(connection, records, self.count.swap(1, Ordering::Relaxed))
            .await
    }

    /// Returns a reusable callback that creates the batch when awaited.
    pub fn lazy<'a>(
        &'a self,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<Collection<M>>> + 'a {
        move || Box::pin(self.create_with(attributes.clone()))
    }

    pub async fn create_one(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.create_one(attributes).await
    }

    pub async fn create_one_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory.create_one_on(connection, attributes).await
    }

    pub async fn create_one_quietly(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.create_one_quietly(attributes).await
    }

    pub async fn create_one_quietly_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory
            .create_one_quietly_on(connection, attributes)
            .await
    }

    pub async fn create_many(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.create_many_on(&self.factory.resolve_connection()?, records)
            .await
    }

    pub async fn create_many_on(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.factory
            .create_many_prepared(connection, records, self.count.swap(1, Ordering::Relaxed))
            .await
    }

    pub async fn create_many_quietly(
        &self,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_many(records)).await
    }

    pub async fn create_many_quietly_on(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_many_on(connection, records)).await
    }

    pub async fn raw(&self) -> DatabaseResult<Vec<Attributes>> {
        self.raw_on(&self.factory.resolve_connection()?).await
    }

    pub async fn raw_on(&self, connection: &Connection) -> DatabaseResult<Vec<Attributes>> {
        self.raw_with_state_on(connection, |_| Attributes::new())
            .await
    }

    /// Expands each row with a one-time override after the configured states.
    pub async fn raw_with(&self, attributes: Attributes) -> DatabaseResult<Vec<Attributes>> {
        self.raw_with_on(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn raw_with_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Vec<Attributes>> {
        self.raw_with_state_on(connection, |_| attributes.clone())
            .await
    }

    /// Computes the last state separately for each row before expansion.
    pub async fn raw_with_state(
        &self,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        self.raw_with_state_on(&self.factory.resolve_connection()?, state)
            .await
    }

    pub async fn raw_with_state_on(
        &self,
        connection: &Connection,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        self.raw_with_state_on_prepared(connection, None, state)
            .await
    }

    /// Expands each row with a parent available to the configured and final states.
    pub async fn raw_with_parent<P: Model>(
        &self,
        parent: &P,
        state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        self.raw_with_parent_on(&self.factory.resolve_connection()?, parent, state)
            .await
    }

    pub async fn raw_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        mut state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        let context = RelationContext::parent_only(parent);
        self.raw_with_state_on_prepared(connection, Some(context), |attributes| {
            state(attributes, parent)
        })
        .await
    }

    async fn raw_with_state_on_prepared(
        &self,
        connection: &Connection,
        context: Option<RelationContext<'_>>,
        mut state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        let faker = std::sync::OnceLock::new();
        let count = self.count.load(Ordering::Relaxed);
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            records.push(
                self.factory
                    .raw_attributes(connection, &faker, context, &mut state)
                    .await?,
            );
        }
        Ok(records)
    }

    /// Selects a recycled model of the requested type without accessing the database.
    pub fn get_random_recycled_model<R: Model>(&self) -> Option<&R> {
        self.factory.get_random_recycled_model()
    }

    #[must_use]
    pub fn recycle<R: Model>(self, model: R) -> Self {
        self.recycle_many([model])
    }

    #[must_use]
    pub fn recycle_many<R: Model>(self, models: impl IntoIterator<Item = R>) -> Self {
        Self {
            factory: self.factory.recycle_many(models),
            count: self.count,
        }
    }

    pub async fn make(&self) -> DatabaseResult<Collection<M>> {
        self.make_on(&self.factory.resolve_connection()?).await
    }

    pub async fn make_on(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        self.make_with_pool(connection, &self.factory.recycled, None)
            .await
    }

    /// Applies the same final attributes to every model in this operation.
    pub async fn make_with(&self, attributes: Attributes) -> DatabaseResult<Collection<M>> {
        self.make_with_on(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn make_with_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_state_on(connection, |_| attributes.clone())
            .await
    }

    /// Computes the final state separately for every model in the batch.
    pub async fn make_with_state(
        &self,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_state_on(&self.factory.resolve_connection()?, state)
            .await
    }

    pub async fn make_with_state_on(
        &self,
        connection: &Connection,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_state_on_prepared(connection, &self.factory.recycled, None, state)
            .await
    }

    /// Constructs unsaved models with a parent available to factory states.
    pub async fn make_with_parent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_parent_on(&self.factory.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn make_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_parent_state_on(connection, parent, |_, _| attributes.clone())
            .await
    }

    /// Builds unsaved models with a final state that can read the parent.
    pub async fn make_with_parent_state<P: Model>(
        &self,
        parent: &P,
        state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_parent_state_on(&self.factory.resolve_connection()?, parent, state)
            .await
    }

    pub async fn make_with_parent_state_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        mut state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_state_on_prepared(
            connection,
            &self.factory.recycled,
            Some(RelationContext::parent_only(parent)),
            |attributes| state(attributes, parent),
        )
        .await
    }

    async fn make_with_pool(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        relation: Option<RelationContext<'_>>,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_state_on_prepared(connection, pool, relation, |_| Attributes::new())
            .await
    }

    async fn make_with_state_on_prepared(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        relation: Option<RelationContext<'_>>,
        mut state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        without_relationship_autoloading(async {
            let count = self.count.load(Ordering::Relaxed);
            let mut models = Vec::with_capacity(count);
            let faker = std::sync::OnceLock::new();
            for _ in 0..count {
                let mut pending = self
                    .factory
                    .prepare_attributes(connection, relation, &faker)?;
                let changes = state(&pending.values);
                pending.values.extend(changes);
                self.factory
                    .resolve_attributes(connection, pool, &mut pending)
                    .await?;
                models.push(
                    Factory::<M>::apply_attributes(connection, pending.model, pending.values)
                        .await?,
                );
            }
            let mut models = M::new_collection(models);
            self.factory
                .run_callbacks(models.models_mut(), &self.factory.after_making)?;
            Ok(models)
        })
        .await
    }

    /// Creates models with a parent available to states and afterCreating callbacks.
    pub async fn create_with_parent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.create_with_parent_on(&self.factory.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn create_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.create_with_parent_state_on(connection, parent, |_, _| attributes.clone())
            .await
    }

    /// Persists models with a final state that can read the parent.
    pub async fn create_with_parent_state<P: Model>(
        &self,
        parent: &P,
        state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.create_with_parent_state_on(&self.factory.resolve_connection()?, parent, state)
            .await
    }

    pub async fn create_with_parent_state_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        mut state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        Box::pin(self.create_with_state_on_prepared(
            connection,
            &[],
            Some(RelationContext::parent_only(parent)),
            |attributes| state(attributes, parent),
        ))
        .await
    }

    /// Creates models with parent context while suppressing Eloquent events.
    pub async fn create_quietly_with_parent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_with_parent(parent, attributes)).await
    }

    pub async fn create_quietly_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_with_parent_on(connection, parent, attributes)).await
    }

    /// Applies a final parent-aware state while suppressing model events.
    pub async fn create_quietly_with_parent_state<P: Model>(
        &self,
        parent: &P,
        state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.create_quietly_with_parent_state_on(&self.factory.resolve_connection()?, parent, state)
            .await
    }

    pub async fn create_quietly_with_parent_state_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_with_parent_state_on(connection, parent, state)).await
    }

    /// Returns a reusable callback borrowing the parent until invocation.
    pub fn lazy_with_parent<'a, P: Model>(
        &'a self,
        parent: &'a P,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<Collection<M>>> + 'a {
        move || Box::pin(self.create_with_parent(parent, attributes.clone()))
    }

    pub async fn create(&self) -> DatabaseResult<Collection<M>> {
        self.create_on(&self.factory.resolve_connection()?).await
    }

    /// Applies the same final attributes to every model in this operation.
    pub async fn create_with(&self, attributes: Attributes) -> DatabaseResult<Collection<M>> {
        self.create_with_on(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn create_with_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.create_with_state_on(connection, |_| attributes.clone())
            .await
    }

    /// Computes the final state separately for every model in the batch.
    pub async fn create_with_state(
        &self,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.create_with_state_on(&self.factory.resolve_connection()?, state)
            .await
    }

    pub async fn create_with_state_on(
        &self,
        connection: &Connection,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        Box::pin(self.create_with_state_on_prepared(connection, &[], None, state)).await
    }

    /// Inserts the generated batch in one statement after running afterMaking callbacks.
    pub async fn insert(&self, attributes: Attributes) -> DatabaseResult<()> {
        self.insert_on(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn insert_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let models = self
            .make_with_state_on_prepared(connection, &self.factory.recycled, None, |_| {
                attributes.clone()
            })
            .await?;
        Factory::<M>::insert_models(connection, models.into_iter().collect()).await
    }

    /// Inserts a batch constructed with a parent available to factory states.
    pub async fn insert_with_parent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        self.insert_with_parent_on(&self.factory.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn insert_with_parent_on<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let models = self
            .make_with_parent_on(connection, parent, attributes)
            .await?;
        Factory::<M>::insert_models(connection, models.into_iter().collect()).await
    }

    pub async fn create_on(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        Box::pin(self.create_with_pool(connection, &[], None)).await
    }

    pub async fn create_quietly(&self) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create()).await
    }

    pub async fn create_quietly_on(
        &self,
        connection: &Connection,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_on(connection)).await
    }

    /// Applies the same final attributes to every model in this operation.
    pub async fn create_quietly_with(
        &self,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.create_quietly_with_on(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn create_quietly_with_on(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.create_quietly_with_state_on(connection, |_| attributes.clone())
            .await
    }

    /// Computes the final state separately for every model in the batch.
    pub async fn create_quietly_with_state(
        &self,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.create_quietly_with_state_on(&self.factory.resolve_connection()?, state)
            .await
    }

    pub async fn create_quietly_with_state_on(
        &self,
        connection: &Connection,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        M::without_events(self.create_with_state_on(connection, state)).await
    }

    async fn create_with_pool(
        &self,
        connection: &Connection,
        inherited: &[RecycledModel],
        relation: Option<RelationContext<'_>>,
    ) -> DatabaseResult<Collection<M>> {
        self.create_with_state_on_prepared(connection, inherited, relation, |_| Attributes::new())
            .await
    }

    async fn create_with_state_on_prepared(
        &self,
        connection: &Connection,
        inherited: &[RecycledModel],
        relation: Option<RelationContext<'_>>,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        let pool = self.factory.recycle_pool(inherited);
        let mut models = self
            .make_with_state_on_prepared(connection, &pool, relation, state)
            .await?;
        self.store_models(
            connection,
            &pool,
            &mut models,
            relation.map(|context| context.parent),
        )
        .await?;
        Ok(models)
    }

    async fn store_models(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        models: &mut Collection<M>,
        parent: Option<&FactoryParentModel>,
    ) -> DatabaseResult<()> {
        for model in models.models_mut() {
            self.factory.store(connection, model, pool).await?;
        }
        self.factory.run_after_creating(models.models_mut(), parent)
    }
}
