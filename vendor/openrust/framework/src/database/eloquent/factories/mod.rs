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
        Connection, DB, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
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
mod regex_pattern;
mod relationships;

pub use faker::{Coordinates, CreditCardDetails, Faker, UniqueFaker, fake};
pub use faker_image::ImageOptions;

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
            Self::newWithState(move |_| attributes.clone())
        }
    }

    /// Builds a factory with a computed initial state before configuration.
    #[allow(non_snake_case, reason = "Matches the factory API naming")]
    fn newWithState(
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
            self.whenElse(condition, transform, core::convert::identity)
        }

        /// Transforms this factory when the condition is false.
        #[must_use]
        pub fn unless(self, condition: bool, transform: impl FnOnce(Self) -> Self) -> Self {
            self.when(!condition, transform)
        }

        /// Applies either the transformation or the fallback.
        #[must_use]
        pub fn whenElse(
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
        pub fn unlessElse(
            self,
            condition: bool,
            transform: impl FnOnce(Self) -> Self,
            fallback: impl FnOnce(Self) -> Self,
        ) -> Self {
            self.whenElse(!condition, transform, fallback)
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

#[allow(non_snake_case, reason = "Matches Laravel's factory API")]
impl<M: Model> Factory<M> {
    conditional_states!();

    pub fn new(definition: impl Fn() -> Attributes + Send + Sync + 'static) -> Self {
        Self::from_definition(move |_, _| Ok(definition()))
    }

    /// Defines model attributes using the application's configured Faker locale.
    pub fn withFaker(definition: impl Fn(&Faker) -> Attributes + Send + Sync + 'static) -> Self {
        Self::tryWithFaker(move |faker| Ok(definition(faker)))
    }

    /// Defines model attributes with a generator that can return an error.
    pub fn tryWithFaker(
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
    pub fn factoryForModel() -> Self
    where
        M: HasFactory,
    {
        M::factory()
    }

    /// The Rust type name of the model bound to this factory.
    pub fn modelName(&self) -> &'static str {
        type_name::<M>()
    }

    /// Constructs an unsaved model from caller attributes using normal mass assignment.
    pub fn newModel(&self, attributes: Attributes) -> DatabaseResult<M> {
        M::newInstance(attributes)
    }

    pub fn getConnectionName(&self) -> Option<&str> {
        self.connection.as_deref()
    }

    fn resolve_connection(&self) -> DatabaseResult<Connection> {
        DB::connection(self.getConnectionName().or(M::metadata().connection))
    }

    fn resolve_related_connection(&self, inherited: &Connection) -> DatabaseResult<Connection> {
        let name = self.getConnectionName().or(M::metadata().connection);
        let Some(name) = name.filter(|name| *name != inherited.getName()) else {
            return Ok(inherited.clone());
        };
        match Connection::model_container(Some(inherited)) {
            Some(container) => container
                .get::<DatabaseManager>("db")
                .map_err(DatabaseError::Unavailable)?
                .connection(Some(name)),
            None => DB::connection(Some(name)),
        }
    }

    /// Adds a persisted model to the pool used when resolving factory owners.
    #[must_use]
    pub fn recycle<R: Model>(self, model: R) -> Self {
        self.recycleMany([model])
    }

    /// Adds several persisted models of one type to the recycle pool.
    #[must_use]
    pub fn recycleMany<R: Model>(mut self, models: impl IntoIterator<Item = R>) -> Self {
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
    pub fn getRandomRecycledModel<R: Model>(&self) -> Option<&R> {
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
    pub fn stateWithParent<P: Model>(
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
    pub fn prependState(
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
    pub fn trashed(mut self, deletedAt: Option<Value>) -> DatabaseResult<Self> {
        let column = M::metadata()
            .deleted_at
            .ok_or_else(soft_deletes::unsupported)?;
        if let Some(value) = deletedAt.filter(|value| !matches!(value, Value::Null)) {
            return Ok(self.set(column, value));
        }

        let now = chrono::Utc::now();
        self.states.push(Box::new(move |_, connection, _| {
            let zone = connection.model_timezone;
            let (_, yesterday) = calendar::date_interval("now", "-1 day", now, zone)
                .map_err(|_| DatabaseError::InvalidQuery("invalid factory deletion timestamp"))?;
            let value = casts::date::format_for_storage(
                yesterday.with_timezone(&zone),
                M::metadata().date_format,
            )?;
            Ok(Attributes::from([(column.to_owned(), value)]))
        }));
        Ok(self)
    }

    #[must_use]
    pub fn sequence(
        mut self,
        sequence: impl Fn(usize) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        let index = AtomicUsize::new(0);
        self.states.push(Box::new(move |_, _, _| {
            Ok(sequence(index.fetch_add(1, Ordering::Relaxed)))
        }));
        self
    }

    /// Sets a sequence of attribute maps and makes the batch as long as the sequence.
    pub fn forEachSequence(self, states: Vec<Attributes>) -> FactoryBatch<M> {
        let count = states.len();
        self.sequence(move |index| {
            index
                .checked_rem(count)
                .map(|offset| states[offset].clone())
                .unwrap_or_default()
        })
        .count(count)
    }

    /// Cycles through combinations in declaration order without allocating the product.
    pub fn crossJoinSequence(self, sequences: Vec<Vec<Attributes>>) -> DatabaseResult<Self> {
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
    pub fn afterMaking(
        mut self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.after_making.push(Box::new(callback));
        self
    }

    #[must_use]
    pub fn afterCreating(
        mut self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.after_creating
            .push(Box::new(move |model, _| callback(model)));
        self
    }

    /// Registers a creation callback that can read the enclosing relationship factory's parent.
    #[must_use]
    pub fn afterCreatingWithParent<P: Model>(
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
    pub fn withoutAfterMaking(mut self) -> Self {
        self.after_making.clear();
        self
    }

    /// Removes the current post-creation callbacks from this factory.
    #[must_use]
    pub fn withoutAfterCreating(mut self) -> Self {
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
    pub fn hasNamed<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
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
    pub fn hasInferred<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
        self,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        let plural = inferred_relation_name::<R>(2);
        let name = if M::namedRelations().contains(&plural) {
            plural
        } else {
            pluralizer::pluralize(&plural, 1, false)
        };
        self.hasNamed::<R, D>(name, children)
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
                                .make_one(
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
    pub fn forNamed<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
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
    pub fn forInferred<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        owner: Factory<R>,
    ) -> Self {
        self.forNamed::<R, D>(inferred_relation_name::<R>(1), owner)
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
                                    .create_one(
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
    pub fn modelAttribute<R: Model>(self, name: impl Into<String>, model: &R) -> Self {
        let key = model.getKey().cloned().unwrap_or(Value::Null);
        self.attribute(name, move |_| Ok(key.clone()))
    }

    /// Declares a factory-valued foreign key in the model definition.
    #[must_use]
    pub fn parentAttribute<R: Model>(self, name: impl Into<String>, owner: Factory<R>) -> Self {
        let owner = Arc::new(owner);
        self.set_parent_attribute(name.into(), move |_| Arc::clone(&owner))
    }

    /// Computes a factory-valued foreign key from the current attribute values.
    #[must_use]
    pub fn parentAttributeWith<R: Model>(
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
                            .create_one(
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
                        .getKey()
                        .filter(|key| !matches!(key, Value::Null))
                        .cloned()
                        .ok_or(DatabaseError::InvalidQuery("factory related primary key"))
                })
            }),
        );
        self
    }

    /// Enables factory-valued parent expansion on newly constructed factories.
    pub fn expandRelationshipsByDefault() {
        EXPAND_RELATIONSHIPS_BY_DEFAULT.store(true, Ordering::Relaxed);
    }

    /// Disables factory-valued parent expansion on newly constructed factories.
    pub fn dontExpandRelationshipsByDefault() {
        EXPAND_RELATIONSHIPS_BY_DEFAULT.store(false, Ordering::Relaxed);
    }

    /// Restores the process-wide factory defaults.
    pub fn flushState() {
        Self::expandRelationshipsByDefault();
    }

    /// Excludes the selected factory-valued attributes, or all when the list is empty.
    #[must_use]
    pub fn withoutParents(mut self, parents: Vec<FactoryParentSelector>) -> Self {
        if parents.is_empty() {
            self.expand_relationships = false;
        } else {
            self.excluded_parents = parents;
        }
        self
    }

    #[must_use]
    pub fn forExisting<R: Model>(self, relation: impl FactoryParent<M, R>, owner: R) -> Self {
        let relation = Arc::new(relation);
        self.for_existing_resolved(move |_| Ok(Arc::clone(&relation)), owner)
    }

    /// Associates an existing model using a registered relationship name and descriptor type.
    #[must_use]
    pub fn forExistingNamed<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
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
    pub fn forExistingInferred<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        owner: R,
    ) -> Self {
        self.forExistingNamed::<R, D>(inferred_relation_name::<R>(1), owner)
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
    pub fn hasAttached<R: Model>(
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
    pub fn hasAttachedNamed<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
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
    pub fn hasAttachedInferred<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
        self,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        self.hasAttachedNamed::<R, D>(inferred_relation_name::<R>(2), models, pivot)
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
                                .getAttribute(relation.related_key())
                                .filter(|key| !matches!(key, Value::Null))
                                .cloned()
                                .ok_or(DatabaseError::InvalidQuery("factory related primary key"))
                        })
                        .collect::<DatabaseResult<Vec<_>>>()?;
                    if let FactoryPivot::Shared(attributes) = pivot {
                        relation
                            .attachWith(parent, &keys, attributes.clone())
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
                                .attachWith(parent, std::slice::from_ref(key), attributes)
                                .await?;
                        }
                    }
                    if let Some(name) = name
                        && !keys.is_empty()
                        && (parent.relationLoaded(name) || relation.touches_related(parent)?)
                    {
                        parent.load(name, relation.as_ref()).await?;
                    }
                    Ok(())
                })
            }));
        self
    }

    pub async fn make(&self) -> DatabaseResult<M> {
        self.makeOn(&self.resolve_connection()?).await
    }

    pub async fn makeOn(&self, connection: &Connection) -> DatabaseResult<M> {
        self.makeOneOn(connection, Attributes::new()).await
    }

    pub async fn makeOne(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.makeOneOn(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn makeOneOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.makeWithStateOn(connection, |_| attributes).await
    }

    /// Applies a final state to the accumulated attributes for this operation.
    pub async fn makeWithState(
        &self,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.makeWithStateOn(&self.resolve_connection()?, state)
            .await
    }

    pub async fn makeWithStateOn(
        &self,
        connection: &Connection,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.make_one(
            connection,
            &self.recycled,
            &std::sync::OnceLock::new(),
            None,
            state,
        )
        .await
    }

    /// Constructs an unsaved model with a parent available to factory states.
    pub async fn makeWithParent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.makeWithParentOn(&self.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn makeWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.make_one(
            connection,
            &self.recycled,
            &std::sync::OnceLock::new(),
            Some(RelationContext::parent_only(parent)),
            |_| attributes,
        )
        .await
    }

    pub async fn makeMany(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.makeManyOn(&self.resolve_connection()?, records).await
    }

    pub async fn makeManyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.make_many(connection, records, 1).await
    }

    async fn make_many(
        &self,
        connection: &Connection,
        records: FactoryRecords,
        default_count: usize,
    ) -> DatabaseResult<Collection<M>> {
        let faker = std::sync::OnceLock::new();
        let mut models = Vec::new();
        for attributes in records.into_attributes(default_count) {
            models.push(
                self.make_one(connection, &self.recycled, &faker, None, |_| attributes)
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
        move || Box::pin(self.createOne(attributes.clone()))
    }

    /// Creates models with a parent available to states and afterCreating callbacks.
    pub async fn createWithParent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.createWithParentOn(&self.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn createWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.create_one(
            connection,
            &self.recycled,
            &std::sync::OnceLock::new(),
            Some(RelationContext::parent_only(parent)),
            |_| attributes,
        )
        .await
    }

    /// Creates models with parent context while suppressing Eloquent events.
    pub async fn createQuietlyWithParent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        M::withoutEvents(self.createWithParent(parent, attributes)).await
    }

    pub async fn createQuietlyWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        M::withoutEvents(self.createWithParentOn(connection, parent, attributes)).await
    }

    /// Returns a reusable callback borrowing the parent until invocation.
    pub fn lazyWithParent<'a, P: Model>(
        &'a self,
        parent: &'a P,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<M>> + 'a {
        move || Box::pin(self.createWithParent(parent, attributes.clone()))
    }

    pub async fn create(&self) -> DatabaseResult<M> {
        self.createOn(&self.resolve_connection()?).await
    }

    /// Inserts the generated model without saving relations or firing model persistence events.
    pub async fn insert(&self, attributes: Attributes) -> DatabaseResult<()> {
        self.insertOn(&self.resolve_connection()?, attributes).await
    }

    pub async fn insertOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let model = self.makeOneOn(connection, attributes).await?;
        Self::insert_models(connection, vec![model]).await
    }

    /// Inserts a model constructed with a parent available to factory states.
    pub async fn insertWithParent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        self.insertWithParentOn(&self.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn insertWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let model = self
            .makeWithParentOn(connection, parent, attributes)
            .await?;
        Self::insert_models(connection, vec![model]).await
    }

    async fn insert_models(connection: &Connection, models: Vec<M>) -> DatabaseResult<()> {
        let rows = models
            .into_iter()
            .map(|mut model| {
                model.setHidden(&[]);
                super::serialization::projected_attributes(&model)
                    .map(|(name, value)| Ok((name.clone(), value?)))
                    .collect::<DatabaseResult<Attributes>>()
            })
            .collect::<DatabaseResult<Vec<_>>>()?;
        M::queryOn(connection)
            .withoutGlobalScopes()
            .fillAndInsert(rows)
            .await?;
        Ok(())
    }

    pub async fn createOn(&self, connection: &Connection) -> DatabaseResult<M> {
        self.createOneOn(connection, Attributes::new()).await
    }

    pub async fn createOne(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.createOneOn(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn createOneOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.createWithStateOn(connection, |_| attributes).await
    }

    /// Applies a final state to the accumulated attributes for this operation.
    pub async fn createWithState(
        &self,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.createWithStateOn(&self.resolve_connection()?, state)
            .await
    }

    pub async fn createWithStateOn(
        &self,
        connection: &Connection,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        self.create_one(
            connection,
            &self.recycled,
            &std::sync::OnceLock::new(),
            None,
            state,
        )
        .await
    }

    pub async fn createOneQuietly(&self, attributes: Attributes) -> DatabaseResult<M> {
        M::withoutEvents(self.createOne(attributes)).await
    }

    pub async fn createOneQuietlyOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        M::withoutEvents(self.createOneOn(connection, attributes)).await
    }

    pub async fn createMany(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.createManyOn(&self.resolve_connection()?, records)
            .await
    }

    pub async fn createManyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.create_many(connection, records, 1).await
    }

    pub async fn createManyQuietly(
        &self,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createMany(records)).await
    }

    pub async fn createManyQuietlyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createManyOn(connection, records)).await
    }

    async fn create_many(
        &self,
        connection: &Connection,
        records: FactoryRecords,
        default_count: usize,
    ) -> DatabaseResult<Collection<M>> {
        let faker = std::sync::OnceLock::new();
        let mut models = Vec::new();
        for attributes in records.into_attributes(default_count) {
            models.push(
                self.create_one(connection, &self.recycled, &faker, None, |_| attributes)
                    .await?,
            );
        }
        Ok(Collection::new(models).without_relationship_autoloading())
    }

    pub async fn createQuietly(&self) -> DatabaseResult<M> {
        M::withoutEvents(self.create()).await
    }

    pub async fn createQuietlyOn(&self, connection: &Connection) -> DatabaseResult<M> {
        M::withoutEvents(self.createOn(connection)).await
    }

    /// Applies a final state while suppressing model events.
    pub async fn createQuietlyWithState(
        &self,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        M::withoutEvents(self.createWithState(state)).await
    }

    pub async fn createQuietlyWithStateOn(
        &self,
        connection: &Connection,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        M::withoutEvents(self.createWithStateOn(connection, state)).await
    }

    /// Expands the factory definition and parent keys without applying model casts or callbacks.
    pub async fn raw(&self) -> DatabaseResult<Attributes> {
        self.rawOn(&self.resolve_connection()?).await
    }

    pub async fn rawOn(&self, connection: &Connection) -> DatabaseResult<Attributes> {
        self.rawWithStateOn(connection, |_| Attributes::new()).await
    }

    /// Expands raw attributes with a one-time override after the configured states.
    pub async fn rawWith(&self, attributes: Attributes) -> DatabaseResult<Attributes> {
        self.rawWithOn(&self.resolve_connection()?, attributes)
            .await
    }

    pub async fn rawWithOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Attributes> {
        self.rawWithStateOn(connection, |_| attributes).await
    }

    /// Computes the last state from the accumulated attributes before expansion.
    pub async fn rawWithState(
        &self,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Attributes> {
        self.rawWithStateOn(&self.resolve_connection()?, state)
            .await
    }

    pub async fn rawWithStateOn(
        &self,
        connection: &Connection,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Attributes> {
        self.raw_attributes(connection, &std::sync::OnceLock::new(), None, state)
            .await
    }

    /// Expands raw attributes with a parent available to the configured and final states.
    pub async fn rawWithParent<P: Model>(
        &self,
        parent: &P,
        state: impl FnOnce(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Attributes> {
        self.rawWithParentOn(&self.resolve_connection()?, parent, state)
            .await
    }

    pub async fn rawWithParentOn<P: Model>(
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
        // Parent keys provide defaults. Factory states and explicit overrides can replace them.
        let mut raw = model.state().attributes.clone();
        for (name, value) in attributes {
            casts::write_async::<M>(
                &mut raw,
                name,
                value,
                Some(connection),
                &model.state().casts,
            )
            .await?;
        }
        model.state_mut().replace_attributes(raw);
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

    async fn make_one(
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

    async fn create_one(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        faker: &std::sync::OnceLock<Arc<Faker>>,
        context: Option<RelationContext<'_>>,
        state: impl FnOnce(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<M> {
        let mut model = self
            .make_one(connection, pool, faker, context, state)
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
        None => M::namedRelations().factory_descriptor(name),
    }
}

/// A factory that produces the requested number of models.
pub struct FactoryBatch<M: Model> {
    factory: Factory<M>,
    count: AtomicUsize,
}

#[allow(non_snake_case, reason = "Matches Laravel's factory API")]
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

    /// Sets the pending batch size to the sequence length.
    #[must_use]
    pub fn forEachSequence(self, states: Vec<Attributes>) -> Self {
        self.factory.forEachSequence(states)
    }

    pub fn crossJoinSequence(self, sequences: Vec<Vec<Attributes>>) -> DatabaseResult<Self> {
        Ok(Self {
            factory: self.factory.crossJoinSequence(sequences)?,
            count: self.count,
        })
    }

    #[must_use]
    pub fn afterMaking(
        self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.afterMaking(callback))
    }

    #[must_use]
    pub fn afterCreating(
        self,
        callback: impl Fn(&mut M) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.afterCreating(callback))
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
    pub fn hasNamed<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
        self,
        name: impl Into<String>,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        self.map_factory(|factory| factory.hasNamed::<R, D>(name, children))
    }

    #[must_use]
    pub fn r#for<R: Model>(self, relation: impl FactoryParent<M, R>, owner: Factory<R>) -> Self {
        self.map_factory(|factory| factory.r#for(relation, owner))
    }

    #[must_use]
    pub fn forNamed<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        name: impl Into<String>,
        owner: Factory<R>,
    ) -> Self {
        self.map_factory(|factory| factory.forNamed::<R, D>(name, owner))
    }

    #[must_use]
    pub fn forExisting<R: Model>(self, relation: impl FactoryParent<M, R>, owner: R) -> Self {
        self.map_factory(|factory| factory.forExisting(relation, owner))
    }

    /// Associates an existing model using a registered relationship name and descriptor type.
    #[must_use]
    pub fn forExistingNamed<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        name: impl Into<String>,
        owner: R,
    ) -> Self {
        self.map_factory(|factory| factory.forExistingNamed::<R, D>(name, owner))
    }

    #[must_use]
    pub fn hasAttached<R: Model>(
        self,
        relation: impl Into<BelongsToMany<M, R>>,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        self.map_factory(|factory| factory.hasAttached(relation, models, pivot))
    }

    #[must_use]
    pub fn hasAttachedNamed<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
        self,
        name: impl Into<String>,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        self.map_factory(|factory| factory.hasAttachedNamed::<R, D>(name, models, pivot))
    }

    /// Infers a child relationship from the related model type.
    #[must_use]
    pub fn hasInferred<R: Model, D: Relation<Parent = M> + FactoryChild<M, R>>(
        self,
        children: impl Into<FactoryChildren<R>>,
    ) -> Self {
        self.map_factory(|factory| factory.hasInferred::<R, D>(children))
    }

    /// Infers the owner relationship from the owner model type.
    #[must_use]
    pub fn forInferred<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        owner: Factory<R>,
    ) -> Self {
        self.map_factory(|factory| factory.forInferred::<R, D>(owner))
    }

    /// Infers the relationship to an existing owner from its model type.
    #[must_use]
    pub fn forExistingInferred<R: Model, D: Relation<Parent = M> + FactoryParent<M, R>>(
        self,
        owner: R,
    ) -> Self {
        self.map_factory(|factory| factory.forExistingInferred::<R, D>(owner))
    }

    /// Infers the plural attachment relationship from the related model type.
    #[must_use]
    pub fn hasAttachedInferred<R: Model, D: Relation<Parent = M> + Into<BelongsToMany<M, R>>>(
        self,
        models: impl Into<FactoryAttachment<R>>,
        pivot: impl Into<FactoryPivot<M>>,
    ) -> Self {
        self.map_factory(|factory| factory.hasAttachedInferred::<R, D>(models, pivot))
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
    pub fn modelAttribute<R: Model>(self, name: impl Into<String>, model: &R) -> Self {
        self.map_factory(|factory| factory.modelAttribute(name, model))
    }

    #[must_use]
    pub fn parentAttribute<R: Model>(self, name: impl Into<String>, owner: Factory<R>) -> Self {
        self.map_factory(|factory| factory.parentAttribute(name, owner))
    }

    #[must_use]
    pub fn parentAttributeWith<R: Model>(
        self,
        name: impl Into<String>,
        owner: impl Fn(&Attributes) -> Factory<R> + Send + Sync + 'static,
    ) -> Self {
        self.map_factory(|factory| factory.parentAttributeWith(name, owner))
    }

    #[must_use]
    pub fn withoutParents(mut self, parents: Vec<FactoryParentSelector>) -> Self {
        self.factory
            .withoutParents(parents)
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn connection(mut self, name: Option<&str>) -> Self {
        self.factory.connection(name).count(*self.count.get_mut())
    }

    /// The Rust type name of the model bound to this factory.
    pub fn modelName(&self) -> &'static str {
        self.factory.modelName()
    }

    /// Constructs an unsaved model from caller attributes using normal mass assignment.
    pub fn newModel(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.newModel(attributes)
    }

    pub fn getConnectionName(&self) -> Option<&str> {
        self.factory.getConnectionName()
    }

    pub fn trashed(mut self, deletedAt: Option<Value>) -> DatabaseResult<Self> {
        Ok(self
            .factory
            .trashed(deletedAt)?
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
    pub fn stateWithParent<P: Model>(
        mut self,
        state: impl Fn(&Attributes, Option<&P>) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.factory
            .stateWithParent(state)
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn afterCreatingWithParent<P: Model>(
        mut self,
        callback: impl Fn(&mut M, Option<&P>) -> DatabaseResult<()> + Send + Sync + 'static,
    ) -> Self {
        self.factory
            .afterCreatingWithParent(callback)
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn prependState(
        mut self,
        state: impl Fn(&Attributes) -> Attributes + Send + Sync + 'static,
    ) -> Self {
        self.factory
            .prependState(state)
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn set(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.factory.set(name, value).count(*self.count.get_mut())
    }

    #[must_use]
    pub fn withoutAfterMaking(mut self) -> Self {
        self.factory
            .withoutAfterMaking()
            .count(*self.count.get_mut())
    }

    #[must_use]
    pub fn withoutAfterCreating(mut self) -> Self {
        self.factory
            .withoutAfterCreating()
            .count(*self.count.get_mut())
    }

    pub async fn makeOne(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.makeOne(attributes).await
    }

    pub async fn makeOneOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory.makeOneOn(connection, attributes).await
    }

    pub async fn makeMany(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.makeManyOn(&self.factory.resolve_connection()?, records)
            .await
    }

    pub async fn makeManyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.factory
            .make_many(connection, records, self.count.swap(1, Ordering::Relaxed))
            .await
    }

    /// Returns a reusable callback that creates the batch when awaited.
    pub fn lazy<'a>(
        &'a self,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<Collection<M>>> + 'a {
        move || Box::pin(self.createWith(attributes.clone()))
    }

    pub async fn createOne(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.createOne(attributes).await
    }

    pub async fn createOneOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory.createOneOn(connection, attributes).await
    }

    pub async fn createOneQuietly(&self, attributes: Attributes) -> DatabaseResult<M> {
        self.factory.createOneQuietly(attributes).await
    }

    pub async fn createOneQuietlyOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<M> {
        self.factory
            .createOneQuietlyOn(connection, attributes)
            .await
    }

    pub async fn createMany(&self, records: FactoryRecords) -> DatabaseResult<Collection<M>> {
        self.createManyOn(&self.factory.resolve_connection()?, records)
            .await
    }

    pub async fn createManyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        self.factory
            .create_many(connection, records, self.count.swap(1, Ordering::Relaxed))
            .await
    }

    pub async fn createManyQuietly(
        &self,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createMany(records)).await
    }

    pub async fn createManyQuietlyOn(
        &self,
        connection: &Connection,
        records: FactoryRecords,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createManyOn(connection, records)).await
    }

    pub async fn raw(&self) -> DatabaseResult<Vec<Attributes>> {
        self.rawOn(&self.factory.resolve_connection()?).await
    }

    pub async fn rawOn(&self, connection: &Connection) -> DatabaseResult<Vec<Attributes>> {
        self.rawWithStateOn(connection, |_| Attributes::new()).await
    }

    /// Expands each row with a one-time override after the configured states.
    pub async fn rawWith(&self, attributes: Attributes) -> DatabaseResult<Vec<Attributes>> {
        self.rawWithOn(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn rawWithOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Vec<Attributes>> {
        self.rawWithStateOn(connection, |_| attributes.clone())
            .await
    }

    /// Computes the last state separately for each row before expansion.
    pub async fn rawWithState(
        &self,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        self.rawWithStateOn(&self.factory.resolve_connection()?, state)
            .await
    }

    pub async fn rawWithStateOn(
        &self,
        connection: &Connection,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        self.raw_with_state(connection, None, state).await
    }

    /// Expands each row with a parent available to the configured and final states.
    pub async fn rawWithParent<P: Model>(
        &self,
        parent: &P,
        state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        self.rawWithParentOn(&self.factory.resolve_connection()?, parent, state)
            .await
    }

    pub async fn rawWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        mut state: impl FnMut(&Attributes, &P) -> Attributes + Send,
    ) -> DatabaseResult<Vec<Attributes>> {
        let context = RelationContext::parent_only(parent);
        self.raw_with_state(connection, Some(context), |attributes| {
            state(attributes, parent)
        })
        .await
    }

    async fn raw_with_state(
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
    pub fn getRandomRecycledModel<R: Model>(&self) -> Option<&R> {
        self.factory.getRandomRecycledModel()
    }

    #[must_use]
    pub fn recycle<R: Model>(self, model: R) -> Self {
        self.recycleMany([model])
    }

    #[must_use]
    pub fn recycleMany<R: Model>(self, models: impl IntoIterator<Item = R>) -> Self {
        Self {
            factory: self.factory.recycleMany(models),
            count: self.count,
        }
    }

    pub async fn make(&self) -> DatabaseResult<Collection<M>> {
        self.makeOn(&self.factory.resolve_connection()?).await
    }

    pub async fn makeOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        self.make_with_pool(connection, &self.factory.recycled, None)
            .await
    }

    /// Applies the same final attributes to every model in this operation.
    pub async fn makeWith(&self, attributes: Attributes) -> DatabaseResult<Collection<M>> {
        self.makeWithOn(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn makeWithOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.makeWithStateOn(connection, |_| attributes.clone())
            .await
    }

    /// Computes the final state separately for every model in the batch.
    pub async fn makeWithState(
        &self,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.makeWithStateOn(&self.factory.resolve_connection()?, state)
            .await
    }

    pub async fn makeWithStateOn(
        &self,
        connection: &Connection,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_state(connection, &self.factory.recycled, None, state)
            .await
    }

    /// Constructs unsaved models with a parent available to factory states.
    pub async fn makeWithParent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.makeWithParentOn(&self.factory.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn makeWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_state(
            connection,
            &self.factory.recycled,
            Some(RelationContext::parent_only(parent)),
            |_| attributes.clone(),
        )
        .await
    }

    async fn make_with_pool(
        &self,
        connection: &Connection,
        pool: &[RecycledModel],
        relation: Option<RelationContext<'_>>,
    ) -> DatabaseResult<Collection<M>> {
        self.make_with_state(connection, pool, relation, |_| Attributes::new())
            .await
    }

    async fn make_with_state(
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
            let mut models = M::newCollection(models);
            self.factory
                .run_callbacks(models.models_mut(), &self.factory.after_making)?;
            Ok(models)
        })
        .await
    }

    /// Creates models with a parent available to states and afterCreating callbacks.
    pub async fn createWithParent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.createWithParentOn(&self.factory.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn createWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        Box::pin(self.create_with_state(
            connection,
            &[],
            Some(RelationContext::parent_only(parent)),
            |_| attributes.clone(),
        ))
        .await
    }

    /// Creates models with parent context while suppressing Eloquent events.
    pub async fn createQuietlyWithParent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createWithParent(parent, attributes)).await
    }

    pub async fn createQuietlyWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createWithParentOn(connection, parent, attributes)).await
    }

    /// Returns a reusable callback borrowing the parent until invocation.
    pub fn lazyWithParent<'a, P: Model>(
        &'a self,
        parent: &'a P,
        attributes: Attributes,
    ) -> impl Fn() -> DatabaseFuture<'a, DatabaseResult<Collection<M>>> + 'a {
        move || Box::pin(self.createWithParent(parent, attributes.clone()))
    }

    pub async fn create(&self) -> DatabaseResult<Collection<M>> {
        self.createOn(&self.factory.resolve_connection()?).await
    }

    /// Applies the same final attributes to every model in this operation.
    pub async fn createWith(&self, attributes: Attributes) -> DatabaseResult<Collection<M>> {
        self.createWithOn(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn createWithOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.createWithStateOn(connection, |_| attributes.clone())
            .await
    }

    /// Computes the final state separately for every model in the batch.
    pub async fn createWithState(
        &self,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.createWithStateOn(&self.factory.resolve_connection()?, state)
            .await
    }

    pub async fn createWithStateOn(
        &self,
        connection: &Connection,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        Box::pin(self.create_with_state(connection, &[], None, state)).await
    }

    /// Inserts the generated batch in one statement after running afterMaking callbacks.
    pub async fn insert(&self, attributes: Attributes) -> DatabaseResult<()> {
        self.insertOn(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn insertOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let models = self
            .make_with_state(connection, &self.factory.recycled, None, |_| {
                attributes.clone()
            })
            .await?;
        Factory::<M>::insert_models(connection, models.into_iter().collect()).await
    }

    /// Inserts a batch constructed with a parent available to factory states.
    pub async fn insertWithParent<P: Model>(
        &self,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        self.insertWithParentOn(&self.factory.resolve_connection()?, parent, attributes)
            .await
    }

    pub async fn insertWithParentOn<P: Model>(
        &self,
        connection: &Connection,
        parent: &P,
        attributes: Attributes,
    ) -> DatabaseResult<()> {
        let models = self
            .makeWithParentOn(connection, parent, attributes)
            .await?;
        Factory::<M>::insert_models(connection, models.into_iter().collect()).await
    }

    pub async fn createOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        Box::pin(self.create_with_pool(connection, &[], None)).await
    }

    pub async fn createQuietly(&self) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.create()).await
    }

    pub async fn createQuietlyOn(&self, connection: &Connection) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createOn(connection)).await
    }

    /// Applies the same final attributes to every model in this operation.
    pub async fn createQuietlyWith(&self, attributes: Attributes) -> DatabaseResult<Collection<M>> {
        self.createQuietlyWithOn(&self.factory.resolve_connection()?, attributes)
            .await
    }

    pub async fn createQuietlyWithOn(
        &self,
        connection: &Connection,
        attributes: Attributes,
    ) -> DatabaseResult<Collection<M>> {
        self.createQuietlyWithStateOn(connection, |_| attributes.clone())
            .await
    }

    /// Computes the final state separately for every model in the batch.
    pub async fn createQuietlyWithState(
        &self,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        self.createQuietlyWithStateOn(&self.factory.resolve_connection()?, state)
            .await
    }

    pub async fn createQuietlyWithStateOn(
        &self,
        connection: &Connection,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        M::withoutEvents(self.createWithStateOn(connection, state)).await
    }

    async fn create_with_pool(
        &self,
        connection: &Connection,
        inherited: &[RecycledModel],
        relation: Option<RelationContext<'_>>,
    ) -> DatabaseResult<Collection<M>> {
        self.create_with_state(connection, inherited, relation, |_| Attributes::new())
            .await
    }

    async fn create_with_state(
        &self,
        connection: &Connection,
        inherited: &[RecycledModel],
        relation: Option<RelationContext<'_>>,
        state: impl FnMut(&Attributes) -> Attributes + Send,
    ) -> DatabaseResult<Collection<M>> {
        let pool = self.factory.recycle_pool(inherited);
        let mut models = self
            .make_with_state(connection, &pool, relation, state)
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
