use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use openrust::{
    config::{ConfigError, Environment, Repository},
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        Connection, DatabaseError, DatabaseFuture, DatabaseManager, DatabaseResult, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState, NamedRelations, UniqueId,
            casts::{Attribute, AttributeCast},
            concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
            factories::{
                Factory, FactoryDefinition, FactoryParentSelector, FactoryPivot, FactoryRecords,
                FactorySequence, Faker, FakerProvider, HasFactory, fake,
            },
            relations::{BelongsTo, MorphMany, MorphMap, MorphMapEntry, MorphTo, MorphToMany},
        },
        migrations::{Migration, MigrationContext, MigrationEntry, MigrationRegistry},
        query::Record,
        seeder::{Seeder, SeederContext, SeederEntry, SeederRegistry},
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::Db},
    testing::TestApp,
};
use serde_json::{Value as Json, json};

struct Create;
impl Migration for Create {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            context
                .get_schema_builder()
                .create("seed_items", |table| {
                    table.id();
                    table.string("name", 60);
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            context
                .get_schema_builder()
                .drop_if_exists("seed_items")
                .await
        })
    }
}

struct DatabaseSeeder;
impl Seeder for DatabaseSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            context.call(&["FirstSeeder", "SecondSeeder"]).await?;
            context
                .call_once(&["Database\\Seeders\\FirstSeeder"])
                .await?;
            context.call_silent(&["SilentSeeder"]).await?;
            context
                .call_with(
                    "ParameterSeeder",
                    Record::from([("name".into(), "parameter".into())]),
                )
                .await
        })
    }
}

struct SeederCalls;

impl Seeder for SeederCalls {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            let params = |name: &str| Record::from([("name".into(), name.into())]);
            context
                .call_with_many(&["FirstSeeder", "SecondSeeder"], params("shared"))
                .await?;
            context
                .call_silent_with(&["SilentSeeder"], params("quiet"))
                .await?;
            context
                .call_once_with(
                    &[
                        "SilentSeeder",
                        "ParameterSeeder",
                        "Database\\Seeders\\ParameterSeeder",
                    ],
                    true,
                    params("once"),
                )
                .await?;

            assert!(
                context
                    .call_once_with(&["RetrySeeder", "UnreachedSeeder"], true, params("fail"))
                    .await
                    .is_err()
            );
            context
                .call_once_with(&["RetrySeeder"], false, params("recovered"))
                .await?;
            context.call(&["FirstSeeder"]).await
        })
    }
}

struct RetrySeeder;

impl Seeder for RetrySeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        if context.parameters().get("name") == Some(&Value::from("fail")) {
            return Box::pin(async { Err(DatabaseError::InvalidSeeder("retry probe")) });
        }
        Insert("missing parameter").run(context)
    }
}

struct SeedItem(ModelState);

struct Insert(&'static str);
impl Seeder for Insert {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            let name = context
                .parameters()
                .get("name")
                .cloned()
                .unwrap_or(self.0.into());
            assert_eq!(
                Db::connection(None)?.get_name(),
                context.connection().get_name()
            );
            let attributes = Attributes::from([("name".into(), name)]);
            let guarded = attributes.clone();
            tokio::spawn(async move {
                assert!(matches!(
                    SeedItem::new_instance(guarded),
                    Err(DatabaseError::MassAssignment(name)) if name == "name"
                ));
            })
            .await
            .unwrap();
            SeedItem::create(attributes).await?;
            Ok(())
        })
    }
}

struct Failing;
impl Seeder for Failing {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            assert_eq!(
                Db::connection(None)?.get_name(),
                context.connection().get_name()
            );
            SeedItem::new_instance(Attributes::from([("name".into(), "unsaved".into())]))?;
            Err(DatabaseError::InvalidSeeder("fixture failure"))
        })
    }
}

struct Recursive;
impl Seeder for Recursive {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.call(&["RecursiveSeeder"]))
    }
}

struct FactoryOwner(ModelState);
struct FactoryChild(ModelState);

macro_rules! factory_model {
    ($model:ident, $table:literal, $relations:expr) => {
        impl Model for $model {
            fn metadata() -> &'static ModelMetadata {
                static METADATA: ModelMetadata = ModelMetadata::new($table).timestamps(None, None);
                &METADATA
            }

            fn named_relations() -> NamedRelations<Self> {
                $relations
            }

            fn from_state(state: ModelState) -> Self {
                Self(state)
            }
            fn state(&self) -> &ModelState {
                &self.0
            }
            fn state_mut(&mut self) -> &mut ModelState {
                &mut self.0
            }
        }
    };
}

factory_model!(SeedItem, "seed_items", NamedRelations::default());

factory_model!(
    FactoryOwner,
    "seed_factory_owners",
    NamedRelations::default().nested("attached", || {
        MorphToMany::<FactoryOwner, FactoryChild>::new(
            "seed_factory_links",
            "owner_id",
            "child_id",
            "id",
            "id",
            "owner_type",
            "seed-owner",
        )
    })
);
factory_model!(
    FactoryChild,
    "seed_factory_children",
    NamedRelations::default().morph_to("owner", || {
        MorphTo::<FactoryChild>::new("owner_type", "owner_id")
            .target::<FactoryOwner>("seed-owner", "id")
    })
);

struct PolymorphicFactories;

struct BulkSeed(ModelState);

impl Model for BulkSeed {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("seed_bulk")
            .guarded(&["*"])
            .hidden(&["score"])
            .unique_id(UniqueId::Uuid)
            .unique_ids(&["external_id"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        let mut model = Self(state);
        model.set_attribute("label", "model-default");
        model
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("options", AttributeCast::Json)]
    }

    fn attribute(name: &str) -> Option<Attribute> {
        (name == "label").then(|| {
            Attribute::set(|value, _| {
                let Value::Text(value) = value else {
                    return Err(DatabaseError::InvalidQuery("bulk label"));
                };
                Ok(Attributes::from([(
                    "label".into(),
                    value.trim().to_lowercase().into(),
                )]))
            })
        })
    }
}

impl ModelEventListener<BulkSeed> for CancelFactoryChild {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        _: &'a mut BulkSeed,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async { Ok(ModelEventResponse::Cancel) })
    }
}

struct CancelFactoryChild;

impl ModelEventListener<FactoryChild> for CancelFactoryChild {
    fn handle<'a>(
        &'a self,
        _: ModelEvent,
        model: &'a mut FactoryChild,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if model.get_attribute("generated_name") == Some(&"quiet-fixture".into()) {
                Ok(ModelEventResponse::Cancel)
            } else {
                Ok(ModelEventResponse::Continue)
            }
        })
    }
}

async fn verify_parent_attributes(connection: &Connection) -> DatabaseResult<()> {
    let factory = || {
        Factory::<FactoryChild>::new(Attributes::new)
            .parent_attribute("owner_id", Factory::<FactoryOwner>::new(Attributes::new))
    };
    let omitted = factory()
        .count(2)
        .without_parents(vec![])
        .raw_on(connection)
        .await?;
    assert!(
        omitted
            .iter()
            .all(|attributes| attributes["owner_id"] == Value::Null)
    );
    let selected = factory()
        .without_parents(vec![FactoryParentSelector::model::<FactoryOwner>()])
        .raw_on(connection)
        .await?;
    assert_eq!(selected["owner_id"], Value::Null);
    let selected = factory()
        .without_parents(vec![FactoryParentSelector::attribute("owner_id")])
        .raw_on(connection)
        .await?;
    assert_eq!(selected["owner_id"], Value::Null);
    let explicit = factory()
        .make_one_on(
            connection,
            Attributes::from([("owner_id".into(), 99_i64.into())]),
        )
        .await?;
    assert_eq!(explicit.get_attribute("owner_id"), Some(&99_i64.into()));
    assert_eq!(connection.table("seed_factory_owners").count("*").await?, 0);

    let generated = factory().count(2).create_on(connection).await?;
    assert_ne!(
        generated[0].get_attribute("owner_id"),
        generated[1].get_attribute("owner_id")
    );
    let owner = Factory::<FactoryOwner>::new(Attributes::new)
        .create_on(connection)
        .await?;
    let key = owner.get_key().unwrap().clone();
    let recycled = factory().recycle(owner).raw_on(connection).await?;
    assert_eq!(recycled["owner_id"], key);
    let relation = MorphTo::<FactoryChild>::new("owner_type", "owner_id")
        .target::<FactoryOwner>("seed-owner", "id");
    let explicit_relation = Factory::<FactoryChild>::new(Attributes::new)
        .r#for(relation, Factory::<FactoryOwner>::new(Attributes::new))
        .without_parents(vec![])
        .raw_on(connection)
        .await?;
    assert_eq!(explicit_relation["owner_type"], "seed-owner".into());
    assert_eq!(connection.table("seed_factory_children").delete().await?, 2);
    assert_eq!(connection.table("seed_factory_owners").delete().await?, 4);
    Ok(())
}

async fn verify_existing_attachments(
    connection: &Connection,
    sqlite: &Connection,
) -> DatabaseResult<()> {
    let owners = || Factory::<FactoryOwner>::new(Attributes::new);
    let children = || Factory::<FactoryChild>::new(Attributes::new);
    let pivot = |label: &str| Attributes::from([("label".into(), label.into())]);
    let existing = children()
        .set("generated_name", "existing")
        .count(2)
        .create_on(connection)
        .await?;
    let existing_keys = existing.model_keys();
    let single = children().create_on(connection).await?;
    let single_key = single.get_key().unwrap().clone();
    owners()
        .has_attached_named::<_, MorphToMany<FactoryOwner, FactoryChild>>(
            "attached",
            existing,
            vec![pivot("existing-first"), pivot("existing-second")],
        )
        .has_attached_named::<_, MorphToMany<FactoryOwner, FactoryChild>>(
            "attached",
            single,
            pivot("existing-single"),
        )
        .create_on(connection)
        .await?;
    let links = connection
        .table("seed_factory_links")
        .order_by("child_id", "asc")
        .get()
        .await?;
    assert_eq!(links.len(), 3);
    for (link, key) in links
        .iter()
        .zip(existing_keys.iter().chain(std::iter::once(&single_key)))
    {
        assert_eq!(link.get("child_id"), Some(key));
    }
    assert_eq!(links[1].get("label"), Some(&"existing-second".into()));
    assert_eq!(
        connection.table("seed_factory_children").count("*").await?,
        3
    );
    let invalid = owners().has_attached_named::<_, MorphToMany<FactoryOwner, FactoryChild>>(
        "attached",
        children().make_on(connection).await?,
        pivot("invalid"),
    );
    assert!(matches!(
        invalid.create_on(connection).await,
        Err(DatabaseError::InvalidQuery(
            "factory attachment requires a persisted model"
        ))
    ));
    sqlite
        .statement(
            "CREATE TABLE seed_factory_children (id INTEGER PRIMARY KEY)",
            &[],
        )
        .await?;
    let remote = children().create_on(sqlite).await?;
    let invalid = owners().has_attached_named::<_, MorphToMany<FactoryOwner, FactoryChild>>(
        "attached",
        remote,
        pivot("invalid"),
    );
    assert!(matches!(
        invalid.create_on(connection).await,
        Err(DatabaseError::InvalidQuery(
            "mixed relationship connections"
        ))
    ));
    assert_eq!(connection.table("seed_factory_links").delete().await?, 3);
    assert_eq!(connection.table("seed_factory_children").delete().await?, 3);
    assert_eq!(connection.table("seed_factory_owners").delete().await?, 3);
    Ok(())
}

impl Seeder for PolymorphicFactories {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            let connection = context.connection();
            assert_eq!(connection.get_name(), "alternate");
            let sqlite = Db::connection(Some("sqlite"))?;
            sqlite
                .statement(
                    "CREATE TABLE seed_factory_owners (id INTEGER PRIMARY KEY)",
                    &[],
                )
                .await?;
            let factory = Factory::<FactoryOwner>::new(Attributes::new).connection(Some("sqlite"));
            assert_eq!(factory.get_connection_name(), Some("sqlite"));
            assert_eq!(
                factory.create().await?.get_connection_name(),
                Some("sqlite")
            );
            assert_eq!(
                factory.create_on(connection).await?.get_connection_name(),
                Some("alternate")
            );
            let counted = factory.count(2);
            assert_eq!(counted.get_connection_name(), Some("sqlite"));
            let deferred = counted.lazy(Attributes::new());
            assert!(
                deferred()
                    .await?
                    .iter()
                    .all(|model| model.get_connection_name() == Some("sqlite"))
            );
            drop(deferred);
            assert_eq!(sqlite.table("seed_factory_owners").count("*").await?, 3);
            assert_eq!(connection.table("seed_factory_owners").delete().await?, 1);
            let counted = counted.connection(None);
            assert_eq!(counted.get_connection_name(), None);
            assert!(
                counted
                    .make()
                    .await?
                    .iter()
                    .all(|model| model.get_connection_name() == Some("alternate"))
            );
            let missing = Factory::<FactoryOwner>::new(Attributes::new).connection(Some("missing"));
            assert!(
                matches!(missing.make().await, Err(DatabaseError::UnknownConnection(name)) if name == "missing")
            );
            let owner_relation = || {
                MorphTo::<FactoryChild>::new("owner_type", "owner_id")
                    .target::<FactoryOwner>("seed-owner", "id")
            };
            let nested =
                Factory::<FactoryChild>::new(Attributes::new).r#for(owner_relation(), missing);
            assert!(matches!(
                nested.raw_on(connection).await,
                Err(DatabaseError::UnknownConnection(name)) if name == "missing"
            ));
            let nested = Factory::<FactoryChild>::new(Attributes::new).r#for(
                owner_relation(),
                Factory::<FactoryOwner>::new(Attributes::new).connection(Some("alternate")),
            );
            assert!(matches!(
                nested.raw_on(connection).await?.get("owner_id"),
                Some(Value::Integer(_) | Value::Unsigned(_))
            ));
            assert_eq!(connection.table("seed_factory_owners").delete().await?, 1);
            let query = || {
                BulkSeed::query_on(connection).with_attributes(
                    Attributes::from([("label".into(), "query-default".into())]),
                    false,
                )
            };
            let record = |label: &str| {
                Attributes::from([
                    ("label".into(), label.into()),
                    ("score".into(), "42".into()),
                ])
            };
            let rows = BulkSeed::query_on(connection)
                .fill_for_insert(vec![Attributes::new()])
                .await?;
            assert_eq!(rows[0]["label"], "model-default".into());
            let rows = query()
                .fill_for_insert(vec![Attributes::from([
                    ("score".into(), "42".into()),
                    ("options".into(), true.into()),
                ])])
                .await?;
            assert_eq!(rows[0]["label"], "query-default".into());
            assert_eq!(rows[0]["score"], "42".into());
            assert_eq!(rows[0]["options"], Value::Json(json!(true)));
            assert!(
                matches!(&rows[0]["external_id"], Value::Text(id) if BulkSeed::is_valid_unique_id(id))
            );
            assert_eq!(rows[0]["created_at"], rows[0]["updated_at"]);
            assert!(query().fill_and_insert(vec![]).await?);
            let mut explicit = record(" BULK-A ");
            explicit.insert("created_at".into(), "2001-01-01 00:00:00".into());
            assert!(
                query()
                    .fill_and_insert(vec![explicit, record("bulk-b")])
                    .await?
            );
            assert_eq!(
                query()
                    .fill_and_insert_or_ignore(vec![record("bulk-a"), record("bulk-c")])
                    .await?,
                1
            );
            let id = query().fill_and_insert_get_id(record("bulk-d")).await?;
            let row = connection
                .table("seed_bulk")
                .r#where("id", "=", id)
                .first()
                .await?
                .unwrap();
            assert_eq!(row.get("label"), Some(&"bulk-d".into()));
            let row = connection
                .table("seed_bulk")
                .r#where("label", "=", "bulk-a")
                .first()
                .await?
                .unwrap();
            assert_eq!(row.get("created_at"), Some(&"2001-01-01 00:00:00".into()));
            assert_eq!(connection.table("seed_bulk").count("*").await?, 4);
            let bulk_factory = || {
                Factory::<BulkSeed>::new(|| {
                    Attributes::from([
                        ("label".into(), "factory-single".into()),
                        ("score".into(), 51_i64.into()),
                    ])
                })
            };
            bulk_factory()
                .count(0)
                .insert_on(connection, Attributes::new())
                .await?;
            bulk_factory().insert(Attributes::new()).await?;
            bulk_factory()
                .sequence(|index| {
                    Attributes::from([("label".into(), format!("factory-{index}").into())])
                })
                .after_making(|model| {
                    model.set_attribute("score", 52_i64);
                    Ok(())
                })
                .has(
                    openrust::database::eloquent::relations::HasMany::new("id", "owner_id"),
                    Factory::<FactoryChild>::new(Attributes::new)
                        .after_making(|_| Err(DatabaseError::InvalidQuery("bulk created a child")))
                        .count(1),
                )
                .after_creating(|_| Err(DatabaseError::InvalidQuery("bulk ran afterCreating")))
                .count(2)
                .insert_on(connection, Attributes::new())
                .await?;
            let rows = connection
                .table("seed_bulk")
                .r#where("label", "like", "factory-%")
                .order_by("label", "asc")
                .get()
                .await?;
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0].get("score"), Some(&52_i64.into()));
            assert_eq!(rows[2].get("score"), Some(&51_i64.into()));
            assert!(rows.iter().all(|row| matches!(row.get("external_id"),
                Some(Value::Text(id)) if BulkSeed::is_valid_unique_id(id))));
            assert!(
                rows.iter()
                    .all(|row| row.get("created_at") == row.get("updated_at"))
            );
            assert!(
                bulk_factory()
                    .count(2)
                    .insert_on(
                        connection,
                        Attributes::from([("label".into(), "factory-duplicate".into())])
                    )
                    .await
                    .is_err()
            );
            assert_eq!(connection.table("seed_bulk").count("*").await?, 7);
            let quiet = || {
                Factory::<FactoryChild>::new(|| {
                    Attributes::from([("generated_name".into(), "quiet-fixture".into())])
                })
                .after_making(|_| Err(DatabaseError::InvalidQuery("raw invoked callback")))
            };
            let records = quiet()
                .sequence(|index| Attributes::from([("index".into(), index.to_string().into())]))
                .count(2)
                .raw_on(connection)
                .await?;
            assert_eq!(records[0]["generated_name"], "quiet-fixture".into());
            assert_eq!(records[1]["index"], "1".into());
            assert!(quiet().count(0).raw_on(connection).await?.is_empty());
            let expanded = quiet()
                .r#for(
                    MorphTo::<FactoryChild>::new("owner_type", "owner_id")
                        .target::<FactoryOwner>("seed-owner", "id"),
                    Factory::<FactoryOwner>::new(Attributes::new),
                )
                .count(2)
                .raw_on(connection)
                .await?;
            assert_eq!(expanded[0]["owner_type"], "seed-owner".into());
            assert_eq!(expanded[0]["owner_id"], expanded[1]["owner_id"]);
            assert_eq!(
                connection.table("seed_factory_children").count("*").await?,
                0
            );
            assert_eq!(connection.table("seed_factory_owners").delete().await?, 1);
            let callbacks = AtomicUsize::new(0);
            let callbacks = Arc::new(callbacks);
            let factory = Factory::<FactoryChild>::new(|| {
                Attributes::from([("generated_name".into(), "quiet-fixture".into())])
            })
            .after_creating({
                let callbacks = Arc::clone(&callbacks);
                move |_| {
                    callbacks.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }
            });
            assert!(matches!(
                factory.create().await,
                Err(DatabaseError::InvalidQuery(
                    "factory model creation cancelled"
                ))
            ));
            factory.create_quietly().await?;
            let batch = factory.count(2);
            assert_eq!(batch.create_quietly_on(connection).await?.len(), 2);
            assert_eq!(callbacks.load(Ordering::Relaxed), 3);
            assert!(batch.create_on(connection).await.is_err());
            assert_eq!(
                connection
                    .table("seed_factory_children")
                    .r#where("generated_name", "=", "quiet-fixture")
                    .delete()
                    .await?,
                3
            );
            let phases = Arc::new(std::sync::Mutex::new(Vec::new()));
            let record_phase = |phase| {
                let phases = Arc::clone(&phases);
                move |_: &mut FactoryChild| {
                    phases.lock().unwrap().push(phase);
                    Ok(())
                }
            };
            let batch = Factory::<FactoryChild>::new(Attributes::new)
                .after_making(record_phase("making"))
                .after_creating(record_phase("created"))
                .sequence(|index| {
                    Attributes::from([("generated_name".into(), format!("many-{index}").into())])
                })
                .count(3);
            let overrides = |name: &str| Attributes::from([("generated_name".into(), name.into())]);
            let configured = Factory::<FactoryChild>::new(move || overrides("definition"))
                .set("generated_name", "single-set")
                .state(|attributes| {
                    assert_eq!(attributes["generated_name"], "single-set".into());
                    assert_eq!(attributes["generated_score"], 41_i64.into());
                    Attributes::from([("generated_score".into(), 42_i64.into())])
                })
                .prepend_state(|attributes| {
                    assert_eq!(attributes["generated_score"], 40_i64.into());
                    Attributes::from([("generated_score".into(), 41_i64.into())])
                })
                .count(2)
                .prepend_state(|attributes| {
                    assert_eq!(attributes["generated_name"], "definition".into());
                    Attributes::from([("generated_score".into(), 40_i64.into())])
                })
                .set("generated_name", "batch-set")
                .state(|attributes| {
                    assert_eq!(attributes["generated_name"], "batch-set".into());
                    assert_eq!(attributes["generated_score"], 42_i64.into());
                    Attributes::new()
                });
            let raw = configured.raw_on(connection).await?;
            assert_eq!(raw.len(), 2);
            assert_eq!(raw[0]["generated_name"], "batch-set".into());
            let rows = configured
                .create_many_on(
                    connection,
                    FactoryRecords::Attributes(vec![
                        overrides("override-one"),
                        overrides("override-two"),
                    ]),
                )
                .await?;
            assert_eq!(rows.len(), 2);
            let rows = connection
                .table("seed_factory_children")
                .order_by("id", "asc")
                .get()
                .await?;
            assert_eq!(rows[0].get("generated_name"), Some(&"override-one".into()));
            assert_eq!(rows[1].get("generated_name"), Some(&"override-two".into()));
            assert!(
                rows.iter()
                    .all(|row| row.get("generated_score") == Some(&42_i64.into()))
            );
            assert_eq!(connection.table("seed_factory_children").delete().await?, 2);
            let created = Factory::<FactoryChild>::new(move || overrides("before-callback"))
                .after_making(|_| Err(DatabaseError::InvalidQuery("removed making callback")))
                .without_after_making()
                .after_making(|model| {
                    model.set_attribute("generated_name", "callback-readded");
                    Ok(())
                })
                .after_creating(|model| {
                    assert!(model.state().exists());
                    assert_eq!(
                        model.get_attribute("generated_name"),
                        Some(&"callback-readded".into())
                    );
                    model.set_attribute("generated_name", "callback-created");
                    Ok(())
                })
                .create_on(connection)
                .await?;
            assert_eq!(
                created.get_attribute("generated_name"),
                Some(&"callback-created".into())
            );
            let created = Factory::<FactoryChild>::new(Attributes::new)
                .after_making(|model| {
                    model.set_attribute("generated_name", "callback-batch");
                    Ok(())
                })
                .after_creating(|_| Err(DatabaseError::InvalidQuery("removed creating callback")))
                .count(2)
                .without_after_creating()
                .create_on(connection)
                .await?;
            assert_eq!(created.len(), 2);
            assert!(created.iter().all(
                |model| model.get_attribute("generated_name") == Some(&"callback-batch".into())
            ));
            assert_eq!(connection.table("seed_factory_children").delete().await?, 3);
            assert!(matches!(
                Factory::<FactoryChild>::new(move || overrides("quiet-fixture"))
                    .count(1)
                    .without_after_making()
                    .without_after_creating()
                    .create_on(connection)
                    .await,
                Err(DatabaseError::InvalidQuery(
                    "factory model creation cancelled"
                ))
            ));
            let preview = batch
                .make_one_on(connection, overrides("preview-one"))
                .await?;
            assert!(!preview.state().exists());
            assert_eq!(
                preview.get_attribute("generated_name"),
                Some(&"preview-one".into())
            );
            let previews = batch.make_many(FactoryRecords::Default).await?;
            assert_eq!(previews.len(), 3);
            assert!(previews.iter().all(|model| !model.state().exists()));
            assert_eq!(
                previews[2].get_attribute("generated_name"),
                Some(&"many-3".into())
            );
            let previews = batch
                .make_many_on(
                    connection,
                    FactoryRecords::Attributes(vec![overrides("preview-many")]),
                )
                .await?;
            assert_eq!(previews.len(), 1);
            assert_eq!(
                previews[0].get_attribute("generated_name"),
                Some(&"preview-many".into())
            );
            assert!(
                batch
                    .make_many_on(connection, FactoryRecords::Count(0))
                    .await?
                    .is_empty()
            );
            let deferred = batch.lazy(overrides("deferred-batch"));
            let single_factory = Factory::<FactoryChild>::new(Attributes::new);
            let deferred_one = single_factory.lazy(overrides("deferred-one"));
            assert_eq!(
                connection.table("seed_factory_children").count("*").await?,
                0
            );
            let created = deferred().await?;
            assert_eq!(created.len(), 1);
            assert!(created.iter().all(
                |model| model.get_attribute("generated_name") == Some(&"deferred-batch".into())
            ));
            assert!(deferred_one().await?.state().exists());
            assert!(deferred_one().await?.state().exists());
            assert_eq!(connection.table("seed_factory_children").delete().await?, 3);
            let each = Factory::<FactoryChild>::new(Attributes::new)
                .for_each_sequence(vec![overrides("sequence-one"), overrides("sequence-two")])
                .create_on(connection)
                .await?;
            assert_eq!(each.len(), 2);
            assert_eq!(
                each[1].get_attribute("generated_name"),
                Some(&"sequence-two".into())
            );
            assert!(
                Factory::<FactoryChild>::new(Attributes::new)
                    .for_each_sequence(vec![])
                    .create_on(connection)
                    .await?
                    .is_empty()
            );
            let score = |value: i64| Attributes::from([("generated_score".into(), value.into())]);
            let crossed = Factory::<FactoryChild>::new(Attributes::new)
                .cross_join_sequence(vec![
                    vec![overrides("cross-a"), overrides("cross-b")],
                    vec![score(31), score(32)],
                    vec![score(40), score(41)],
                ])?
                .count(9)
                .create_on(connection)
                .await?;
            let order: Vec<_> = crossed
                .iter()
                .map(|model| {
                    (
                        model.get_attribute("generated_name").cloned(),
                        model.get_attribute("generated_score").cloned(),
                    )
                })
                .collect();
            let expected: Vec<_> = [
                "cross-a", "cross-a", "cross-a", "cross-a", "cross-b", "cross-b", "cross-b",
                "cross-b", "cross-a",
            ]
            .into_iter()
            .zip([40_i64, 41, 40, 41, 40, 41, 40, 41, 40])
            .map(|(name, score)| (Some(name.into()), Some(score.into())))
            .collect();
            assert_eq!(order, expected);
            assert!(matches!(
                Factory::<FactoryChild>::new(Attributes::new).cross_join_sequence(vec![vec![]]),
                Err(DatabaseError::InvalidQuery(
                    "factory cross join sequence is empty"
                ))
            ));
            assert_eq!(
                connection.table("seed_factory_children").delete().await?,
                11
            );
            let one = batch
                .create_one_on(connection, overrides("many-one"))
                .await?;
            assert_eq!(
                one.get_attribute("generated_name"),
                Some(&"many-one".into())
            );
            phases.lock().unwrap().clear();
            let defaults = batch.create_many(FactoryRecords::Default).await?;
            assert_eq!(defaults.len(), 1);
            assert_eq!(*phases.lock().unwrap(), ["making", "created"]);
            assert_eq!(
                defaults[0].get_attribute("generated_name"),
                Some(&"many-7".into())
            );
            let counted = batch
                .create_many_on(connection, FactoryRecords::Count(1))
                .await?;
            assert_eq!(counted.len(), 1);
            assert!(
                batch
                    .create_many_on(connection, FactoryRecords::Count(0))
                    .await?
                    .is_empty()
            );
            assert!(matches!(
                batch
                    .create_many_on(
                        connection,
                        FactoryRecords::Attributes(vec![
                            overrides("many-before-error"),
                            overrides("quiet-fixture"),
                            overrides("many-after-error"),
                        ])
                    )
                    .await,
                Err(DatabaseError::InvalidQuery(
                    "factory model creation cancelled"
                ))
            ));
            assert_eq!(
                connection
                    .table("seed_factory_children")
                    .r#where("generated_name", "=", "many-before-error")
                    .count("*")
                    .await?,
                1
            );
            assert_eq!(
                connection
                    .table("seed_factory_children")
                    .r#where("generated_name", "=", "many-after-error")
                    .count("*")
                    .await?,
                0
            );
            batch.create_one_quietly(overrides("quiet-fixture")).await?;
            let quiet = batch
                .create_many_quietly_on(
                    connection,
                    FactoryRecords::Attributes(vec![
                        overrides("quiet-fixture"),
                        overrides("many-quiet"),
                    ]),
                )
                .await?;
            assert_eq!(quiet.len(), 2);
            assert_eq!(
                quiet[1].get_attribute("generated_name"),
                Some(&"many-quiet".into())
            );
            assert_eq!(connection.table("seed_factory_children").delete().await?, 7);
            assert_eq!(fake(None)?.locale(), "zh-Hant");
            assert_eq!(fake(Some("fr-fr"))?.locale(), "fr-FR");
            let invalid = fake(Some("../en"));
            assert!(matches!(
                invalid,
                Err(DatabaseError::InvalidConfiguration(_))
            ));
            let epoch = jiff::Timestamp::UNIX_EPOCH;
            let taipei = openrust::support::Date::timezone("Asia/Taipei").unwrap();
            let new_york = openrust::support::Date::timezone("America/New_York").unwrap();
            let utc = jiff::tz::TimeZone::UTC;
            let faker = fake(None)?;
            let other = fake(Some("fr-fr"))?;
            assert_eq!(faker.get_default_timezone(), None);
            assert_eq!(faker.iso8601(Some(epoch)), "1970-01-01T08:00:00+0800");
            assert_eq!(
                other
                    .date_time_in_timezone(Some(epoch), None)
                    .time_zone()
                    .clone(),
                taipei
            );
            other.set_default_timezone(Some(new_york.clone()));
            assert_eq!(other.get_default_timezone(), Some(new_york.clone()));
            assert_eq!(other.date(None, Some(epoch))?, "1969-12-31");
            assert_eq!(other.year(Some(epoch)), "1969");
            assert_eq!(other.day_of_week(Some(epoch)), "Wednesday");
            assert_eq!(
                other
                    .unique(false)
                    .date_time_in_timezone(Some(epoch), Some(utc.clone()))?
                    .time_zone()
                    .clone(),
                utc
            );
            assert_eq!(
                faker
                    .date_time_in_timezone(Some(epoch), None)
                    .time_zone()
                    .clone(),
                taipei
            );
            other.set_default_timezone(None);
            assert_eq!(
                other
                    .date_time_in_timezone(Some(epoch), None)
                    .time_zone()
                    .clone(),
                taipei
            );
            assert_eq!(
                openrust::database::eloquent::factories::Faker::new("en")?
                    .date_time_in_timezone(Some(epoch), None)
                    .time_zone()
                    .clone(),
                utc
            );
            faker.set_default_timezone(Some(utc.clone()));
            assert_eq!(
                fake(Some("zh-Hant"))?.get_default_timezone(),
                Some(utc.clone())
            );
            for value in [
                faker.first_name(),
                faker.last_name(),
                faker.user_name(),
                faker.street_name(),
                faker.postcode(),
                faker.phone_number(),
                faker.company(),
                faker.word(),
            ] {
                assert!(!value.trim().is_empty());
            }
            assert!(faker.free_email().contains('@'));

            let start = jiff::Timestamp::new(-1, 0).unwrap();
            let end = jiff::Timestamp::new(1, 0).unwrap();
            let max = Some(faker.resolve_date_max("0")?);
            assert_eq!(faker.unix_time(max), 0);
            assert_eq!(faker.date_time(max), epoch);
            assert_eq!(faker.date(None, max)?, "1970-01-01");
            assert_eq!(faker.time(None, max)?, "00:00:00");
            assert_eq!(
                faker.unique(false).iso8601(max)?,
                "1970-01-01T00:00:00+0000"
            );
            assert!(faker.unique(false).iso8601(Some(epoch)).is_err());
            faker.unique(true);
            assert_eq!(faker.resolve_date_max(" -1.9 ")?.as_second(), -1);
            assert_eq!(faker.resolve_date_max("1e1")?.as_second(), 10);
            assert_eq!(
                faker.resolve_date_max("2024-01-02 12:00")?.as_second(),
                1_704_168_000
            );
            assert_eq!(
                faker.resolve_date_max("2024-01-02 12:00 UTC")?.as_second(),
                1_704_196_800
            );
            let before = jiff::Timestamp::now();
            let resolved = faker.resolve_date_max("")?;
            assert!(
                (before.as_second()..=jiff::Timestamp::now().as_second())
                    .contains(&resolved.as_second())
            );
            assert!(matches!(
                faker.resolve_date_max("bad date"),
                Err(DatabaseError::InvalidQuery("invalid Faker date expression"))
            ));
            assert!(faker.resolve_date_max(" ").is_err());
            assert_eq!(faker.resolve_date_max("\u{b}1e999\u{c}")?, epoch);
            assert_eq!(faker.resolve_date_max("-1e999")?, epoch);
            assert_eq!(faker.unique(false).unix_time_expression("1e999")?, 0);
            assert!(faker.unique(false).unix_time(Some(epoch)).is_err());
            faker.unique(true);
            assert!(faker.unix_time_expression("+1000000-01-01")? >= 0);
            assert!(faker.unix_time_expression("@-9223372036854775808")? <= 0);
            assert!(faker.unix_time_expression("9223372036854775808")? >= 0);

            assert!(matches!(
                faker.resolve_date_max("1e100"),
                Err(DatabaseError::InvalidQuery(
                    "Faker timestamp is out of range"
                ))
            ));
            assert_eq!(faker.unix_time(Some(epoch)), 0);
            assert!((-1..=0).contains(&faker.unix_time(Some(start))));
            assert!((0..=jiff::Timestamp::now().as_second()).contains(&faker.unix_time(None)));
            assert_eq!(faker.date_time(Some(epoch)), epoch);
            let mut china = [
                faker.unique(false).timezone(Some("CN"))?,
                faker.unique(false).timezone(Some("CN"))?,
            ];
            china.sort_unstable();
            assert_eq!(china, [Some("Asia/Shanghai"), Some("Asia/Urumqi")]);
            assert_eq!(faker.timezone(Some("TW")), Some("Asia/Taipei"));
            assert_eq!(faker.timezone(Some("ZZ")), None);
            assert_eq!(faker.timezone(Some("us")), None);
            assert!(faker.timezone(None).is_some());
            assert!(faker.timezone(Some("")).is_some());
            assert_eq!(faker.unique(false).timezone(Some("ZZ"))?, None);
            assert!(matches!(
                faker.unique(false).timezone(Some("CN")),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));

            assert_eq!(faker.date(None, Some(epoch))?, "1970-01-01");
            assert_eq!(faker.time(None, Some(epoch))?, "00:00:00");
            assert_eq!(faker.date(Some("%d/%m/%Y"), Some(epoch))?, "01/01/1970");
            assert_eq!(faker.iso8601(Some(epoch)), "1970-01-01T00:00:00+0000");
            let first_ad = jiff::Timestamp::new(-62_135_597_361, 0).unwrap();
            assert_eq!(faker.date_time_ad(Some(first_ad)), first_ad);
            assert_eq!(faker.am_pm(Some(epoch)), "上午");
            assert_eq!(faker.day_of_month(Some(epoch)), "01");
            assert_eq!(faker.day_of_week(Some(epoch)), "星期四");
            assert_eq!(faker.month(Some(epoch)), "01");
            assert_eq!(faker.month_name(Some(epoch)), "一月");
            assert_eq!(faker.year(Some(epoch)), "1970");
            let calendar_unique = faker.unique(false);
            assert_eq!(calendar_unique.date_time_ad(Some(first_ad))?, first_ad);
            assert_eq!(calendar_unique.am_pm(Some(epoch))?, "上午");
            assert_eq!(calendar_unique.day_of_month(Some(epoch))?, "01");
            assert_eq!(calendar_unique.day_of_week(Some(epoch))?, "星期四");
            assert_eq!(calendar_unique.month(Some(epoch))?, "01");
            assert_eq!(calendar_unique.month_name(Some(epoch))?, "一月");
            assert_eq!(calendar_unique.year(Some(epoch))?, "1970");
            assert!(calendar_unique.am_pm(Some(epoch)).is_err());
            let mut centuries = (0..21)
                .map(|_| calendar_unique.century())
                .collect::<DatabaseResult<Vec<_>>>()?;
            centuries.sort_unstable();
            assert_eq!(
                centuries,
                [
                    "I", "II", "III", "IV", "IX", "V", "VI", "VII", "VIII", "X", "XI", "XII",
                    "XIII", "XIV", "XIX", "XV", "XVI", "XVII", "XVIII", "XX", "XXI",
                ]
            );
            assert!(matches!(
                calendar_unique.century(),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));

            assert!(matches!(
                faker.date(Some("%"), Some(epoch)),
                Err(DatabaseError::InvalidQuery("Faker 日期格式无效"))
            ));
            let unique = faker.unique(false);
            assert_eq!(unique.unix_time(Some(epoch))?, 0);
            assert_eq!(unique.date_time(Some(epoch))?, epoch);
            assert_eq!(unique.date(None, Some(epoch))?, "1970-01-01");
            assert_eq!(unique.time(None, Some(epoch))?, "00:00:00");
            assert_eq!(unique.iso8601(Some(epoch))?, "1970-01-01T00:00:00+0000");
            assert!(matches!(
                unique.iso8601(Some(epoch)),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            for _ in 0..20 {
                assert!((-2..=3).contains(&faker.number_between(3, -2)));
                assert!(faker.random_digit() <= 9);
                assert!((1..=9).contains(&faker.random_digit_not_null()));
                assert!((start..=end).contains(&faker.date_time_between(start, end)?));
            }
            assert!(matches!(
                faker.unique(false).date_time_between(end, start),
                Err(DatabaseError::InvalidQuery(
                    "Faker start date exceeds end date"
                ))
            ));
            for (expression, expected) in [
                ("2024-03-31 12:34:00 -1 month", "2024-03-02T04:34:00Z"),
                (
                    "2024-04-10 02:30 America/New_York -1 month",
                    "2024-03-10T07:30:00Z",
                ),
                (
                    "2024-11-27 02:30 Europe/Paris -1 month",
                    "2024-10-27T00:30:00Z",
                ),
                ("2100-07-15 12:00 America/New_York", "2100-07-15T16:00:00Z"),
                ("2024-01-15 12:00 next Monday", "2024-01-21T16:00:00Z"),
                ("@-1", "1969-12-31T23:59:59Z"),
            ] {
                let expected = expected.parse::<jiff::Timestamp>().unwrap();
                let upper = expected.as_second().to_string();
                assert_eq!(
                    faker.date_time_between_expressions(expression, &upper)?,
                    expected
                );
            }
            assert_eq!(faker.unix_time_expression("bad date")?, 0);
            assert_eq!(faker.date_time_between_expressions("bad date", "0")?, epoch);
            assert_eq!(
                faker.date_time_between_expressions("@0", "bad date")?,
                epoch
            );
            assert!(matches!(
                faker.date_time_between_expressions("@1", "bad date"),
                Err(DatabaseError::InvalidQuery(
                    "Faker start date exceeds end date"
                ))
            ));
            assert!(
                faker
                    .date_time_in_interval_expressions(Some("bad date"), Some("1 day"))
                    .is_err()
            );
            assert!(matches!(
                faker.unix_time_expression(&" ".repeat(20 * 1024 * 1024 + 1)),
                Err(DatabaseError::InvalidQuery(
                    "Faker date expression exceeds parser limits"
                ))
            ));
            assert!(matches!(
                faker.date_time_between_expressions("tomorrow", "today"),
                Err(DatabaseError::InvalidQuery(
                    "Faker start date exceeds end date"
                ))
            ));
            assert_eq!(faker.unique(false).date_time_between(start, start)?, start);
            assert!(matches!(
                faker
                    .unique(false)
                    .date_time_between_expressions("@-1", "-1"),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            let expression_interval = faker.date_time_in_interval_expressions(
                Some("2024-03-09 12:00 America/New_York"),
                Some("+1 day"),
            )?;
            let spring_start = "2024-03-09T17:00:00Z".parse::<jiff::Timestamp>().unwrap();
            let spring_end = "2024-03-10T16:00:00Z".parse::<jiff::Timestamp>().unwrap();
            assert!(
                (spring_start.as_second()..=spring_end.as_second())
                    .contains(&expression_interval.as_second())
            );
            assert_eq!(
                faker
                    .unique(false)
                    .date_time_in_interval_expressions(Some("@0"), Some("today"))?,
                epoch
            );
            assert!(matches!(
                faker
                    .unique(false)
                    .date_time_in_interval(Some(epoch), Some(jiff::SignedDuration::ZERO)),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            faker.unique(true);
            assert!(matches!(
                faker.date_time_in_interval_expressions(Some("now"), Some("1 day UTC")),
                Err(DatabaseError::InvalidQuery(
                    "invalid Faker date or interval expression"
                ))
            ));
            let second = jiff::SignedDuration::from_secs(1);
            for interval in [second, -second] {
                let other = epoch + interval;
                let sampled = faker.date_time_in_interval(Some(epoch), Some(interval))?;
                assert!((epoch.min(other)..=epoch.max(other)).contains(&sampled));
            }
            assert!(matches!(
                faker.date_time_in_interval(Some(jiff::Timestamp::MAX), Some(second)),
                Err(DatabaseError::InvalidQuery(
                    "Faker timestamp is out of range"
                ))
            ));
            assert_eq!(
                unique.date_time_in_interval(Some(epoch), Some(jiff::SignedDuration::ZERO))?,
                epoch
            );
            assert!(matches!(
                unique.date_time_in_interval(Some(epoch), Some(jiff::SignedDuration::ZERO)),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            for date in [
                unique.date_time_this_century(Some(epoch))?,
                unique.date_time_this_decade(None)?,
                unique.date_time_this_year(None)?,
                unique.date_time_this_month(None)?,
                faker.date_time_in_interval(None, None)?,
            ] {
                assert!(date <= jiff::Timestamp::now());
                assert_eq!(date.subsec_nanosecond(), 0);
            }
            let year_start = jiff::civil::Date::new(
                jiff::Timestamp::now().to_zoned(taipei.clone()).year(),
                1,
                1,
            )
            .unwrap()
            .at(0, 0, 0, 0)
            .to_zoned(taipei.clone())
            .unwrap()
            .timestamp();
            assert_eq!(faker.date_time_this_year(Some(year_start))?, year_start);
            for date in [
                faker.date_time_this_century(Some(jiff::Timestamp::MIN)),
                faker.date_time_this_decade(Some(epoch)),
                faker.date_time_this_year(Some(epoch)),
                faker.date_time_this_month(Some(epoch)),
            ] {
                assert!(matches!(
                    date,
                    Err(DatabaseError::InvalidQuery(
                        "Faker start date exceeds end date"
                    ))
                ));
            }
            assert_eq!(
                faker.unique(false).number_between(i64::MIN, i64::MIN)?,
                i64::MIN
            );
            assert_eq!(
                faker.unique(false).number_between(i64::MAX, i64::MAX)?,
                i64::MAX
            );
            assert!(matches!(
                faker.unique(false).number_between(i64::MIN, i64::MIN),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert!(faker.unique(false).random_digit()? <= 9);
            assert!((1..=9).contains(&faker.unique(false).random_digit_not_null()?));
            assert_ne!(faker.unique(false).random_digit_not(4)?, 4);
            assert_eq!(faker.random_float(Some(2), 1.005, Some(1.005))?, 1.01);
            assert_eq!(faker.random_float(Some(2), -1.005, Some(-1.005))?, -1.01);
            assert_eq!(faker.random_float(Some(-1), -25.0, Some(-25.0))?, -30.0);
            assert_eq!(
                faker.random_float(Some(0), 1_000_000_000.0, None)?,
                1_000_000_000.0
            );
            for _ in 0..20 {
                assert!((-2.0..=2.0).contains(&faker.random_float(None, 2.0, Some(-2.0))?));
            }
            assert!(faker.random_float(Some(2), f64::NAN, None).is_err());
            assert_eq!(
                faker
                    .unique(false)
                    .random_float(Some(0), -0.0, Some(-0.0))?,
                0.0
            );
            assert!(matches!(
                faker.unique(false).random_float(Some(0), 0.0, Some(0.0)),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert!((100_000_000..=999_999_999).contains(&faker.random_number(Some(9), true)?));
            assert!((0..=999_999_999).contains(&faker.random_number(None, false)?));
            assert!(matches!(
                faker.random_number(Some(10), false),
                Err(DatabaseError::InvalidQuery(
                    "Faker random number exceeds digit limit"
                ))
            ));
            assert!(matches!(
                faker.random_number(Some(0), true),
                Err(DatabaseError::InvalidQuery(
                    "Faker strict digit count is zero"
                ))
            ));
            assert_eq!(faker.unique(false).random_number(Some(0), false)?, 0);
            assert!(matches!(
                faker.unique(false).random_number(Some(0), false),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));

            assert_eq!(faker.unique(false).sentence(0..1)?, ".");
            assert!(matches!(
                faker.unique(false).sentence(0..1),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert_eq!(faker.unique(true).sentence(0..1)?, ".");

            let exhausted = Factory::<FactoryChild>::try_with_faker(|faker| {
                Ok(Attributes::from([(
                    "generated_name".into(),
                    faker.unique(false).sentence(0..1)?.into(),
                )]))
            })
            .count(2);
            for _ in 0..2 {
                assert!(matches!(
                    exhausted.create_on(connection).await,
                    Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
                ));
                assert_eq!(
                    connection.table("seed_factory_children").count("*").await?,
                    0
                );
            }
            let same_locale = fake(Some("ZH-hant"))?;
            assert!(Arc::ptr_eq(&faker, &same_locale));
            assert!(!Arc::ptr_eq(&faker, &other));
            assert!(matches!(
                same_locale.unique(false).sentence(0..1),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert_eq!(other.unique(true).sentence(0..1)?, ".");
            faker.unique(true);
            faker.set_default_timezone(None);
            let owners = || Factory::<FactoryOwner>::new(Attributes::new);
            let children = || Factory::<FactoryChild>::new(Attributes::new);
            let pivot = |label: &str| Attributes::from([("label".into(), label.into())]);
            let pivot_calls = Arc::new(AtomicUsize::new(0));
            owners()
                .has_attached_named::<_, MorphToMany<FactoryOwner, FactoryChild>>(
                    "attached",
                    children().count(3),
                    vec![pivot("first"), pivot("second")],
                )
                .has_attached_named::<_, MorphToMany<FactoryOwner, FactoryChild>>(
                    "attached",
                    children().count(2),
                    FactoryPivot::resolve({
                        let pivot_calls = Arc::clone(&pivot_calls);
                        move |owner: &FactoryOwner| {
                            assert!(owner.state().exists());
                            assert!(owner.get_key().is_some());
                            let index = pivot_calls.fetch_add(1, Ordering::Relaxed);
                            Ok(Attributes::from([(
                                "label".into(),
                                format!("resolved-{index}").into(),
                            )]))
                        }
                    }),
                )
                .create_on(connection)
                .await?;
            let labels = connection
                .table("seed_factory_links")
                .order_by("child_id", "asc")
                .pluck("label")
                .await?;
            assert_eq!(
                labels,
                ["first", "second", "first", "resolved-0", "resolved-1"].map(Value::from)
            );
            assert_eq!(pivot_calls.load(Ordering::Relaxed), 2);
            assert_eq!(connection.table("seed_factory_links").delete().await?, 5);
            assert_eq!(connection.table("seed_factory_children").delete().await?, 5);
            assert_eq!(connection.table("seed_factory_owners").delete().await?, 1);
            Box::pin(verify_parent_attributes(connection)).await?;
            Box::pin(verify_existing_attachments(connection, &sqlite)).await?;
            let localized = Factory::<FactoryChild>::try_with_faker(move |faker| {
                assert_eq!(faker.locale(), "zh-Hant");
                assert_eq!(
                    faker
                        .date_time_in_timezone(Some(epoch), None)
                        .time_zone()
                        .clone(),
                    taipei
                );
                let name = faker.unique(false).first_name()?;
                assert!(
                    name.chars()
                        .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
                );
                Ok(Attributes::from([
                    ("generated_name".into(), name.into()),
                    (
                        "generated_score".into(),
                        faker.unique(false).number_between(10, 11)?.into(),
                    ),
                    (
                        "generated_at".into(),
                        faker
                            .date_time_between(start, start)?
                            .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
                            .to_string()
                            .into(),
                    ),
                ]))
            })
            .count(2)
            .create_on(connection)
            .await?;
            assert!(localized.iter().all(|model| model.state().exists()));
            assert_ne!(
                localized[0].get_attribute("generated_name"),
                localized[1].get_attribute("generated_name")
            );
            let subject = || {
                MorphTo::<FactoryChild>::new("owner_type", "owner_id")
                    .target::<FactoryOwner>("seed-owner", "id")
            };
            for name in ["missing", "owner"] {
                let invalid = children().for_named::<FactoryOwner, openrust::database::eloquent::relations::BelongsTo<FactoryChild, FactoryOwner>>(name, owners());
                assert!(matches!(
                    invalid.make_on(connection).await,
                    Err(DatabaseError::InvalidQuery(_))
                ));
            }
            assert_eq!(connection.table("seed_factory_owners").count("*").await?, 0);
            let parented = children()
                .for_named::<_, MorphTo<FactoryChild>>("owner", owners())
                .count(2);
            let first = parented.create_on(connection).await?;
            let second = parented.create_on(connection).await?;
            assert_eq!(
                first[0].get_attribute("owner_id"),
                first[1].get_attribute("owner_id")
            );
            assert_eq!(
                first[0].get_attribute("owner_id"),
                second[0].get_attribute("owner_id")
            );
            assert_eq!(
                second[0].get_attribute("owner_id"),
                second[1].get_attribute("owner_id")
            );
            assert_eq!(
                first[0].get_attribute("owner_type"),
                Some(&"seed-owner".into())
            );

            let foreign = Root::new();
            let url = std::env::var("SEED_FIXTURE_MYSQL").unwrap();
            // 验证同库的独立连接，不另建隔离数据库。
            let foreign_connection = foreign.connect(json!({"url":url}));
            let foreign_owner = FactoryOwner::query_on(&foreign_connection)
                .find_or_fail(first[0].get_attribute("owner_id").unwrap().clone())
                .await?;
            let independent = children()
                .r#for(subject(), owners())
                .recycle(foreign_owner)
                .make_on(connection)
                .await?;
            assert_ne!(
                independent.get_attribute("owner_id"),
                first[0].get_attribute("owner_id")
            );

            let owner_pool = FactoryOwner::query_on(connection).get().await?;
            let reusable_ids = owner_pool.model_keys();
            let drafts = children()
                .r#for(subject(), owners())
                .count(3)
                .recycle_many(owner_pool)
                .make_on(connection)
                .await?;
            let reused = drafts[0].get_attribute("owner_id").unwrap();
            assert!(reusable_ids.contains(reused));
            assert!(
                drafts.iter().all(|draft| !draft.state().exists()
                    && draft.get_attribute("owner_id") == Some(reused))
            );

            let recycled = FactoryOwner::query_on(connection)
                .find_or_fail(reusable_ids[0].clone())
                .await?;
            let recycled_id = recycled.get_key().unwrap().clone();
            let owner = owners()
                .recycle(recycled)
                .has_named::<_, MorphMany<FactoryOwner, FactoryChild>>(
                    "children",
                    children().count(2),
                )
                .has_attached_named::<_, MorphToMany<FactoryOwner, FactoryChild>>(
                    "attached",
                    children().r#for(subject(), owners()).count(1),
                    Attributes::from([("label".into(), "attached".into())]),
                )
                .create_on(connection)
                .await?;
            let attached = connection
                .table("seed_factory_links")
                .pluck("child_id")
                .await?;
            let linked = FactoryChild::query_on(connection)
                .find_or_fail(attached[0].clone())
                .await?;
            assert_eq!(linked.get_attribute("owner_id"), Some(&recycled_id));
            let draft = children()
                .for_existing(subject(), owner)
                .make_on(connection)
                .await?;
            assert_eq!(
                draft.get_attribute("owner_type"),
                Some(&"seed-owner".into())
            );
            assert!(!draft.state().exists());
            Ok(())
        })
    }
}

struct SeederEventFlow;
struct MutedSeeder;
struct EventModelSeeder;
struct FailingEventSeeder;
struct FailedSeederEventFlow;

impl Seeder for EventModelSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            Factory::<FactoryChild>::new(|| {
                Attributes::from([("generated_name".into(), "quiet-fixture".into())])
            })
            .create_on(context.connection())
            .await?;
            Ok(())
        })
    }
}

impl Seeder for MutedSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.call(&["EventModelSeeder"]))
    }
}

impl Seeder for FailingEventSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            context.call(&["EventModelSeeder"]).await?;
            Err(DatabaseError::InvalidSeeder("muted fixture failure"))
        })
    }
}

impl Seeder for SeederEventFlow {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            assert!(context.call(&["EventModelSeeder"]).await.is_err());
            context.call(&["MutedSeeder"]).await?;
            assert!(context.call(&["EventModelSeeder"]).await.is_err());
            let error = context.call(&["FailingEventSeeder"]).await.unwrap_err();
            assert!(
                error.to_string().contains("muted fixture failure"),
                "{error}"
            );
            assert!(context.call(&["EventModelSeeder"]).await.is_err());
            Ok(())
        })
    }
}

impl Seeder for FailedSeederEventFlow {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(context.call(&["FailingEventSeeder"]))
    }
}

struct ParentCreationSeeder;

impl Seeder for ParentCreationSeeder {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(verify_parent_creation(context.connection()))
    }
}

async fn verify_parent_creation(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("seed_factory_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("seed_factory_children", |table| {
            table.id();
            table.unsigned_big_integer("owner_id").nullable();
            table.string("generated_name", 80);
        })
        .await?;
    let parent = Factory::<FactoryOwner>::new(Attributes::new)
        .create_on(connection)
        .await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let factory = || {
        let calls = Arc::clone(&calls);
        Factory::<FactoryChild>::new(Attributes::new)
            .state_with_parent::<FactoryOwner>(|_, parent| {
                let key = parent.and_then(Model::get_key).unwrap();
                Attributes::from([("generated_name".into(), format!("parent-{key:?}").into())])
            })
            .after_creating_with_parent::<FactoryOwner>(move |model, parent| {
                assert!(model.state().exists());
                assert!(parent.and_then(Model::get_key).is_some());
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
    };
    let final_state = |attributes: &Attributes, model: &FactoryOwner| {
        let key = model.get_key().unwrap();
        assert_eq!(model.get_key(), parent.get_key());
        assert_eq!(
            attributes["generated_name"],
            format!("parent-{key:?}").into()
        );
        Attributes::from([("generated_name".into(), "computed".into())])
    };
    let child = factory()
        .create_with_parent_state_on(connection, &parent, final_state)
        .await?;
    assert!(child.get_attribute("owner_id").is_none());
    assert_eq!(
        child.get_attribute("generated_name"),
        Some(&"computed".into())
    );
    assert_eq!(
        connection
            .table("seed_factory_children")
            .pluck("generated_name")
            .await?,
        ["computed".into()]
    );
    let cancelled = || Attributes::from([("generated_name".into(), "quiet-fixture".into())]);
    assert!(
        factory()
            .create_with_parent_on(connection, &parent, cancelled())
            .await
            .is_err()
    );
    let quiet_state = |attributes: &Attributes, owner: &FactoryOwner| {
        final_state(attributes, owner);
        cancelled()
    };
    factory()
        .create_quietly_with_parent_state_on(connection, &parent, quiet_state)
        .await?;
    let batch = factory().count(2);
    let mut quiet_calls = 0;
    batch
        .create_quietly_with_parent_state_on(connection, &parent, |attributes, owner| {
            quiet_calls += 1;
            quiet_state(attributes, owner)
        })
        .await?;
    assert_eq!(quiet_calls, 2);
    assert!(
        batch
            .create_with_parent_on(connection, &parent, cancelled())
            .await
            .is_err()
    );
    let single = factory();
    let lazy_one = single.lazy_with_parent(&parent, Attributes::new());
    let lazy_batch = batch.lazy_with_parent(&parent, Attributes::new());
    assert_eq!(
        connection.table("seed_factory_children").count("*").await?,
        4
    );
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    assert!(lazy_one().await?.state().exists());
    assert_eq!(lazy_batch().await?.len(), 2);
    assert_eq!(lazy_batch().await?.len(), 2);
    assert_eq!(calls.load(Ordering::Relaxed), 9);
    assert_eq!(
        connection
            .table("seed_factory_children")
            .where_null("owner_id")
            .count("*")
            .await?,
        9
    );
    schema.drop("seed_factory_children").await?;
    schema.drop("seed_factory_owners").await?;
    Ok(())
}

struct Provider;
impl ServiceProvider for Provider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
                "database/migrations/0001_create_seed_items.rs",
                &Create,
            )];
            static SEEDERS: &[SeederEntry] = &[
                SeederEntry::new("DatabaseSeeder", &DatabaseSeeder),
                SeederEntry::new("SeederCalls", &SeederCalls),
                SeederEntry::new("RetrySeeder", &RetrySeeder),
                SeederEntry::new("UnreachedSeeder", &Insert("unreached")),
                SeederEntry::new("FirstSeeder", &Insert("first")),
                SeederEntry::new("SecondSeeder", &Insert("second")),
                SeederEntry::new("SilentSeeder", &Insert("silent")),
                SeederEntry::new("ParameterSeeder", &Insert("fallback")),
                SeederEntry::new("FailingSeeder", &Failing),
                SeederEntry::new("RecursiveSeeder", &Recursive),
                SeederEntry::new("SeederEventFlow", &SeederEventFlow),
                SeederEntry::new("MutedSeeder", &MutedSeeder).without_model_events(),
                SeederEntry::new("EventModelSeeder", &EventModelSeeder),
                SeederEntry::new("FailingEventSeeder", &FailingEventSeeder).without_model_events(),
                SeederEntry::new("FailedSeederEventFlow", &FailedSeederEventFlow),
                SeederEntry::new("ParentCreationSeeder", &ParentCreationSeeder),
                SeederEntry::new("PolymorphicFactories", &PolymorphicFactories),
                SeederEntry::new("FakerApplicationScope", &FakerApplicationScope),
            ];
            FactoryOwner::resolve_relation_using(app, "children", |_| {
                MorphMany::<FactoryOwner, FactoryChild>::new(
                    "id",
                    "owner_id",
                    "owner_type",
                    "seed-owner",
                )
            })?;
            FactoryChild::register_model_event(app, ModelEvent::Creating, CancelFactoryChild)?;
            BulkSeed::register_model_event(app, ModelEvent::Creating, CancelFactoryChild)?;
            MorphMap::morph_map(
                app,
                [MorphMapEntry::model::<FactoryOwner>("seed-owner")],
                true,
            )?;
            app.container()
                .instance(Arc::new(MigrationRegistry(MIGRATIONS)))?;
            app.container()
                .instance(Arc::new(SeederRegistry(SEEDERS)))?;
            Ok(())
        })
    }
}

fn load(_: &Path, _: &Environment) -> Result<BTreeMap<String, Json>, ConfigError> {
    let alternate = std::env::var("SEED_FIXTURE_MYSQL").map_or_else(
        |_| json!({"driver":"sqlite","database":"database/alternate.sqlite"}),
        |url| json!({"url":url}),
    );
    let environment = std::env::var("SEED_FIXTURE_ENV").unwrap_or_else(|_| "local".into());
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":environment,"faker_locale":"zh-hant","timezone":"Asia/Taipei"}),
        ),
        (
            "database".into(),
            json!({"default":"sqlite","migrations":{"table":"seed_migrations"},"connections":{
                "sqlite":{"driver":"sqlite","database":"database/database.sqlite"},
                "alternate":alternate
            }}),
        ),
    ]))
}

#[tokio::test]
async fn seeder_command_fixture() {
    let Some(root) = std::env::var_os("SEED_FIXTURE_ROOT") else {
        return;
    };
    let args: Vec<String> =
        serde_json::from_str(&std::env::var("SEED_FIXTURE_ARGS").unwrap()).unwrap();
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let mut app = Application::configure(root.into())
        .with_config(load)
        .with_providers(vec![Box::new(Provider)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().get::<DatabaseManager>("db").unwrap();
    let status = app.handle_command(&args).await.unwrap();
    assert!(matches!(
        SeedItem::new_instance(Attributes::from([("name".into(), "outside".into())])),
        Err(DatabaseError::MassAssignment(name)) if name == "name"
    ));
    assert_eq!(manager.get_default_connection().unwrap(), "sqlite");
    std::process::exit(status);
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Root(PathBuf, OnceLock<Mysql>);

struct Mysql {
    source: String,
    url: String,
    connection: Connection,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Mysql {
    fn new(source: &str) -> Self {
        let (ready, receiver) = std::sync::mpsc::sync_channel(1);
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let source_owned = source.to_owned();
        // 同步 Artisan 用例和异步工厂用例共用既有隔离夹具；独立线程避免嵌套运行时。
        let worker = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let mut app = TestApp::mysql(&source_owned, |builder| builder)
                        .await
                        .expect("创建 Seeder 隔离数据库失败");
                    let connection = app.database().await.unwrap();
                    let url = app
                        .app()
                        .config()
                        .unwrap()
                        .get("database.connections.mysql.url")
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .unwrap();
                    let _ = ready.send((url, connection));
                    let _ = stopped.await;
                    app.close().await.expect("清理 Seeder 隔离数据库失败");
                });
        });
        let (url, connection) = receiver.recv().expect("Seeder 隔离数据库未就绪");
        Self {
            source: source.to_owned(),
            url,
            connection,
            stop: Some(stop),
            worker: Some(worker),
        }
    }
}

impl Drop for Mysql {
    fn drop(&mut self) {
        let _ = self.stop.take().unwrap().send(());
        if self.worker.take().unwrap().join().is_err() {
            if std::thread::panicking() {
                eprintln!("Seeder 隔离数据库清理失败，请核对验收日志");
            } else {
                panic!("Seeder 隔离数据库清理失败");
            }
        }
    }
}

impl Root {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "openrust-seeding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("database")).unwrap();
        for file in ["database.sqlite", "alternate.sqlite"] {
            fs::File::create(root.join("database").join(file)).unwrap();
        }
        Self(root, OnceLock::new())
    }
    fn mysql(&self, source: &str) -> &Mysql {
        let mysql = self.1.get_or_init(|| Mysql::new(source));
        assert!(
            mysql.source == source,
            "同一 Seeder 夹具不能切换 MySQL 实例"
        );
        mysql
    }
    fn fixture(&self, environment: &str, mysql: Option<&str>, args: &[&str]) -> Output {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "seeder_command_fixture", "--nocapture"])
            // The polymorphic fixture runs many factory combinations in one Debug future.
            .env("RUST_MIN_STACK", "8388608")
            .env("SEED_FIXTURE_ROOT", &self.0)
            .env("SEED_FIXTURE_ENV", environment)
            .env("SEED_FIXTURE_ARGS", serde_json::to_string(args).unwrap());
        if let Some(url) = mysql {
            command.env("SEED_FIXTURE_MYSQL", &self.mysql(url).url);
        } else {
            command.env_remove("SEED_FIXTURE_MYSQL");
        }
        command.output().unwrap()
    }
    fn connection(&self, mysql: Option<&str>, file: &str) -> Connection {
        match mysql {
            Some(url) => self.mysql(url).connection.clone(),
            None => self.connect(json!({"driver":"sqlite","database":file})),
        }
    }
    fn connect(&self, config: Json) -> Connection {
        let manager = DatabaseManager::new(
            Arc::new(Repository::new(BTreeMap::from([(
                "database".into(),
                json!({"default":"verify","connections":{"verify":config}}),
            )]))),
            &self.0,
        );
        manager.connection(None).unwrap()
    }
    async fn close(mut self) {
        if let Some(mysql) = self.1.take() {
            // 归还连接期间让调用者的运行时继续处理连接池关闭。
            tokio::task::spawn_blocking(move || drop(mysql))
                .await
                .unwrap();
        }
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        drop(self.1.take());
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

async fn verify(mysql: Option<&str>, allow_fresh: bool) {
    let root = Root::new();
    let db = root.connection(mysql, "database/alternate.sqlite");
    for table in ["seed_items", "seed_migrations"] {
        db.get_schema_builder().drop_if_exists(table).await.unwrap();
    }
    let run = |args: &[&str]| root.fixture("local", mysql, args);
    let pretend = success(run(&[
        "migrate",
        "--database=alternate",
        "--pretend",
        "--seed",
    ]));
    assert!(!pretend.contains("Seeding database"));
    assert!(
        !db.get_schema_builder()
            .has_table("seed_items")
            .await
            .unwrap()
    );
    let seeded = success(run(&["migrate", "--database=alternate", "--seed"]));
    assert!(seeded.contains("FirstSeeder DONE") && seeded.contains("ParameterSeeder DONE"));
    assert!(!seeded.contains("SilentSeeder"));
    let rows = || db.table("seed_items").order_by("id", "asc");
    assert_eq!(
        rows().pluck("name").await.unwrap(),
        ["first", "second", "silent", "parameter"].map(Value::from)
    );
    let default = root.connection(None, "database/database.sqlite");
    assert!(
        !default
            .get_schema_builder()
            .has_table("seed_items")
            .await
            .unwrap()
    );

    let refused = root.fixture(
        "production",
        mysql,
        &["db:seed", "--database=alternate", "--no-interaction"],
    );
    assert_eq!(refused.status.code(), Some(1));
    assert_eq!(rows().count("*").await.unwrap(), 4);
    success(root.fixture(
        "production",
        mysql,
        &[
            "db:seed",
            "FirstSeeder",
            "--class=UnknownSeeder",
            "--database=alternate",
            "--force",
        ],
    ));
    assert_eq!(rows().count("*").await.unwrap(), 5);

    for (name, expected) in [
        ("UnknownSeeder", "not registered"),
        ("FailingSeeder", "fixture failure"),
        ("RecursiveSeeder", "recursive call"),
    ] {
        let output = run(&["db:seed", "--database=alternate", "--class", name]);
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    }

    let expected = ["shared", "shared", "quiet", "once", "recovered", "first"];
    for invocation in 0..2 {
        let output = success(run(&[
            "db:seed",
            "--database=alternate",
            "--class=SeederCalls",
        ]));
        assert!(
            output.find("FirstSeeder DONE").unwrap() < output.find("SecondSeeder DONE").unwrap()
        );
        assert!(!output.contains("SilentSeeder") && !output.contains("ParameterSeeder"));
        assert_eq!(output.matches("RetrySeeder RUNNING").count(), 1);
        assert_eq!(output.matches("RetrySeeder DONE").count(), 1);
        let written = rows().pluck("name").await.unwrap();
        let offset = 5 + invocation * expected.len();
        assert_eq!(&written[offset..], &expected.map(Value::from));
    }

    success(run(&[
        "migrate:refresh",
        "--database=alternate",
        "--seeder=Database\\Seeders\\SecondSeeder",
    ]));
    assert_eq!(rows().pluck("name").await.unwrap(), ["second".into()]);
    if allow_fresh {
        success(run(&[
            "migrate:fresh",
            "--database=alternate",
            "--seed",
            "--seeder=FirstSeeder",
        ]));
        assert_eq!(rows().pluck("name").await.unwrap(), ["first".into()]);
    }
    for table in ["seed_items", "seed_migrations"] {
        db.get_schema_builder().drop(table).await.unwrap();
    }
    root.close().await;
}

#[tokio::test]
async fn artisan_seeders_use_the_selected_sqlite_database() {
    verify(None, true).await;
}

#[tokio::test]
async fn artisan_seeders_write_to_real_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    verify(Some(&url), false).await;
}

#[tokio::test]
async fn mysql_seed_fixtures_isolate_tables_and_cleanup_owned_databases() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let first = Root::new();
    let second = Root::new();
    let first_db = first.connection(Some(&url), "");
    let second_db = second.connection(Some(&url), "");
    assert_ne!(first_db.get_database_name(), second_db.get_database_name());
    let second_name = second_db.get_database_name().to_owned();
    let migrate = ["migrate", "--database=alternate", "--seed"];
    success(first.fixture("local", Some(&url), &migrate));
    success(second.fixture("local", Some(&url), &migrate));
    first_db.table("seed_items").delete().await.unwrap();
    assert_eq!(first_db.table("seed_items").count("*").await.unwrap(), 0);
    assert!(second_db.table("seed_items").count("*").await.unwrap() > 0);
    success(first.fixture(
        "local",
        Some(&url),
        &["db:seed", "--database=alternate", "--class=FirstSeeder"],
    ));
    assert_eq!(first_db.table("seed_items").count("*").await.unwrap(), 1);
    drop(second_db);
    second.close().await;
    assert!(
        !first_db
            .table("information_schema.SCHEMATA")
            .r#where("SCHEMA_NAME", "=", second_name)
            .exists()
            .await
            .unwrap()
    );
    assert_eq!(first_db.table("seed_items").count("*").await.unwrap(), 1);
    first.close().await;
}

#[tokio::test]
async fn artisan_seeder_persists_polymorphic_factory_relationships_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = root.connection(Some(&url), "");
    let mut schema = db.get_schema_builder();
    for table in [
        "seed_bulk",
        "seed_factory_links",
        "seed_factory_children",
        "seed_factory_owners",
    ] {
        schema.drop_if_exists(table).await.unwrap();
    }
    for sql in [
        "CREATE TABLE seed_bulk (id BIGINT AUTO_INCREMENT PRIMARY KEY, external_id VARCHAR(36) NOT NULL, label VARCHAR(80) NOT NULL UNIQUE, score BIGINT NOT NULL, options JSON, created_at VARCHAR(40) NOT NULL, updated_at VARCHAR(40) NOT NULL)",
        "CREATE TABLE seed_factory_owners (id BIGINT AUTO_INCREMENT PRIMARY KEY)",
        "CREATE TABLE seed_factory_children (id BIGINT AUTO_INCREMENT PRIMARY KEY, owner_id BIGINT, owner_type VARCHAR(40), generated_name VARCHAR(120), generated_score BIGINT, generated_at VARCHAR(40))",
        "CREATE TABLE seed_factory_links (owner_id BIGINT NOT NULL, child_id BIGINT NOT NULL, owner_type VARCHAR(40) NOT NULL, label VARCHAR(40) NOT NULL)",
    ] {
        db.statement(sql, &[]).await.unwrap();
    }
    success(root.fixture(
        "production",
        Some(&url),
        &[
            "db:seed",
            "--database=alternate",
            "--class=PolymorphicFactories",
            "--force",
        ],
    ));
    assert_eq!(db.table("seed_factory_owners").count("*").await.unwrap(), 3);
    assert_eq!(
        db.table("seed_factory_children").count("*").await.unwrap(),
        9
    );
    assert_eq!(
        db.table("seed_factory_children")
            .r#where("owner_type", "=", "seed-owner")
            .count("*")
            .await
            .unwrap(),
        7
    );
    assert_eq!(
        db.table("seed_factory_links")
            .r#where("owner_type", "=", "seed-owner")
            .pluck("label")
            .await
            .unwrap(),
        ["attached".into()]
    );
    let names = db
        .table("seed_factory_children")
        .pluck("generated_name")
        .await
        .unwrap();
    assert_eq!(
        names
            .iter()
            .filter(|name| matches!(name, Value::Text(value) if !value.is_empty()))
            .count(),
        2
    );
    let generated = db.table("seed_factory_children");
    let scores = generated.pluck("generated_score").await.unwrap();
    assert!(scores.contains(&10_i64.into()));
    assert!(scores.contains(&11_i64.into()));
    assert_eq!(
        db.table("seed_factory_children")
            .r#where("generated_at", "=", "1969-12-31T23:59:59+00:00")
            .count("*")
            .await
            .unwrap(),
        2
    );
    for table in [
        "seed_bulk",
        "seed_factory_links",
        "seed_factory_children",
        "seed_factory_owners",
    ] {
        schema.drop(table).await.unwrap();
    }
    root.close().await;
}

#[tokio::test]
async fn mysql_fresh_seeds_the_selected_database() {
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_WIPE_URL") {
        verify(Some(&url), true).await;
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
}

#[test]
fn seed_generator_preserves_existing_files_until_forced() {
    let root = Root::new();
    let command = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .env("APP_BASE_PATH", &root.0)
            .env("APP_ENV", "local")
            .env("DB_CONNECTION", "sqlite")
            .env_remove("DB_URL")
            .args(["artisan"])
            .args(args)
            .output()
            .unwrap()
    };
    success(command(&["migrate", "--seed"]));
    let args = ["make:seeder", "Admin/APIUserSeeder"];
    assert!(success(command(&args)).contains("database/seeders/admin/api_user_seeder.rs"));
    let file = root.0.join("database/seeders/admin/api_user_seeder.rs");
    let generated = fs::read_to_string(&file).unwrap();
    fs::write(&file, "// existing\n").unwrap();
    assert_eq!(command(&args).status.code(), Some(1));
    assert_eq!(fs::read_to_string(&file).unwrap(), "// existing\n");
    success(command(&["make:seeder", "Admin/APIUserSeeder", "--force"]));
    assert_eq!(fs::read_to_string(file).unwrap(), generated);
    assert_eq!(
        command(&["make:seeder", "../Outside"]).status.code(),
        Some(1)
    );
}

#[test]
fn faker_float_rounding_matches_upstream_decimal_boundaries() -> DatabaseResult<()> {
    let faker = openrust::database::eloquent::factories::Faker::new("en-US")?;
    for (value, decimals, expected) in [
        (5.0e-310, 309, 5.0e-310),
        (1.4239704891618352e-200, 200, 1.0e-200),
        (1.75e308, -308, 1.75e308),
        (1.0e308, -309, 0.0),
        (0.285, 10, 0.285),
    ] {
        assert_eq!(
            faker.random_float(Some(decimals), value, Some(value))?,
            expected
        );
    }
    Ok(())
}

#[test]
fn faker_lorem_preserves_counts_byte_limits_and_unique_history() -> DatabaseResult<()> {
    let faker = openrust::database::eloquent::factories::Faker::new("en-US")?;
    assert!(faker.words(0).is_empty());
    assert!(faker.sentence_with_word_count(0, true).is_empty());
    assert!(faker.paragraph(0, true).is_empty());
    assert_eq!(faker.words(4).len(), 4);
    assert_eq!(faker.words_as_text(4).split_whitespace().count(), 4);
    let sentence = faker.sentence_with_word_count(4, false);
    assert_eq!(sentence.split_whitespace().count(), 4);
    assert!(sentence.starts_with(char::is_uppercase));
    assert!(sentence.ends_with('.'));
    assert_eq!(faker.paragraph(4, false).matches('.').count(), 4);
    assert_eq!(faker.sentences(4).len(), 4);
    assert_eq!(faker.sentences_as_text(4).matches('.').count(), 4);
    assert_eq!(faker.paragraphs(4).len(), 4);
    assert_eq!(faker.paragraphs_as_text(4).split("\n\n").count(), 4);
    for _ in 0..16 {
        assert!(
            (7..=15).contains(
                &faker
                    .sentence_with_word_count(10, true)
                    .split_whitespace()
                    .count()
            )
        );
        for max in [5, 24, 25, 99, 100, 200] {
            let text = faker.text(max)?;
            assert!(!text.is_empty() && text.len() <= max && text.ends_with('.'));
        }
    }
    assert!(faker.text(4).is_err());
    let unique = faker.unique(false);
    assert!(unique.words(0)?.is_empty());
    assert!(unique.words(0).is_err());
    assert!(unique.words_as_text(0)?.is_empty());
    assert!(unique.words_as_text(0).is_err());
    assert!(unique.paragraphs(0)?.is_empty());
    assert!(unique.paragraphs(0).is_err());
    assert!(faker.unique(true).words(0)?.is_empty());
    let localized = openrust::database::eloquent::factories::Faker::new("zh-Hans")?;
    let text = localized.text(200)?;
    assert!(text.len() <= 200 && !text.is_ascii());
    Ok(())
}

#[test]
fn faker_placeholders_preserve_classes_literals_and_unique_history() -> DatabaseResult<()> {
    let faker = openrust::database::eloquent::factories::Faker::new("zh-Hant")?;
    for _ in 0..64 {
        let digits = faker.numerify(Some("##########%%%%-樣品?*"));
        assert!(digits[..10].bytes().all(|byte| byte.is_ascii_digit()));
        assert!(
            digits[10..14]
                .bytes()
                .all(|byte| (b'1'..=b'9').contains(&byte))
        );
        assert_eq!(&digits[14..], "-樣品?*");
        let letters = faker.lexify(Some("????-樣品#%*"));
        assert!(letters[..4].bytes().all(|byte| byte.is_ascii_lowercase()));
        assert_eq!(&letters[4..], "-樣品#%*");
        let mixed = faker.bothify(Some("#%?****************-樣品"));
        let bytes = mixed.as_bytes();
        assert!(bytes[0].is_ascii_digit());
        assert!((b'1'..=b'9').contains(&bytes[1]));
        assert!(bytes[2].is_ascii_lowercase());
        assert!(
            bytes[3..19]
                .iter()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        );
        assert_eq!(&mixed[19..], "-樣品");
        let ascii = faker.asciify(Some("****-樣品#%?"));
        assert!(ascii[..4].bytes().all(|byte| byte.is_ascii_graphic()));
        assert_eq!(&ascii[4..], "-樣品#%?");
        assert!(faker.random_letter().is_ascii_lowercase());
        assert!(faker.random_ascii().is_ascii_graphic());
    }
    assert_eq!(faker.numerify(None).len(), 3);
    assert_eq!(faker.lexify(None).len(), 4);
    assert_eq!(faker.bothify(None).as_bytes()[2], b' ');
    assert_eq!(faker.asciify(None).len(), 4);
    assert_eq!(faker.bothify(Some("")), "");

    let unique = faker.unique(false);
    assert_eq!(unique.bothify(Some("樣品"))?, "樣品");
    assert!(unique.bothify(Some("樣品")).is_err());
    assert_eq!(unique.numerify(Some("樣品"))?, "樣品");
    assert_eq!(faker.unique(true).bothify(Some("樣品"))?, "樣品");
    Ok(())
}

#[test]
fn faker_sampling_and_shuffle_follow_collection_boundaries() -> DatabaseResult<()> {
    let faker = openrust::database::eloquent::factories::Faker::new("en-US")?;
    let values = [4, 4, 9];
    assert_eq!(faker.random_element::<i32>(&[]), None);
    assert_eq!(faker.random_key::<i32>(&[]), None);
    assert!(
        faker
            .random_elements::<i32>(&[], Some(0), false)?
            .is_empty()
    );
    assert!(faker.random_elements::<i32>(&[], None, false).is_err());
    assert!(faker.random_elements::<i32>(&[], Some(1), true).is_err());
    assert!(faker.random_elements(&values, Some(4), false).is_err());
    for _ in 0..64 {
        let mut selected = faker.random_elements(&values, Some(3), false)?;
        selected.sort_unstable();
        assert_eq!(selected, [&4, &4, &9]);
        assert!((1..=3).contains(&faker.random_elements(&values, None, false)?.len()));
        assert_eq!(
            faker.random_elements(&values[..1], Some(5), true)?,
            vec![&4; 5]
        );
        assert!(
            faker
                .random_key(&values)
                .is_some_and(|index| index < values.len())
        );
        assert!(
            faker
                .random_element(&values)
                .is_some_and(|item| values.contains(item))
        );
        assert!(!faker.boolean(Some(0)));
        assert!(faker.boolean(Some(100)));
    }
    let mut shuffled = faker.shuffle_array(&values);
    shuffled.sort_unstable();
    assert_eq!(shuffled, values);
    let mut shuffled: Vec<_> = faker.shuffle_string("中a😊中").chars().collect();
    shuffled.sort_unstable();
    assert_eq!(shuffled, ['a', '中', '中', '😊']);
    assert_eq!(faker.shuffle_string(""), "");
    let unique = faker.unique(false);
    assert!(!unique.boolean(Some(0))?);
    assert!(unique.boolean(Some(0)).is_err());
    assert_eq!(unique.random_key(&values[..1])?, Some(0));
    assert!(unique.random_key(&values[..1]).is_err());
    assert_eq!(unique.random_element(&values[..1])?, Some(&4));
    assert!(unique.random_element(&values[1..2]).is_err());
    assert_eq!(unique.random_elements(&values[..1], Some(1), false)?, [&4]);
    assert!(
        unique
            .random_elements(&values[1..2], Some(1), false)
            .is_err()
    );
    assert_eq!(unique.shuffle_array(&values[..1])?, [4]);
    assert!(unique.shuffle_array(&values[..1]).is_err());
    assert_eq!(unique.shuffle_string("中")?, "中");
    assert!(unique.shuffle_string("中").is_err());
    Ok(())
}

#[test]
fn faker_miscellaneous_codes_and_emoji_preserve_the_configured_locale() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    let faker = Faker::new("zh-Hant-TW")?;
    let unique = faker.unique(false);
    let mut language_codes = std::collections::HashSet::new();
    for _ in 0..184 {
        let language = unique.language_code()?;
        assert_eq!(language.len(), 2);
        assert!(language.bytes().all(|byte| byte.is_ascii_lowercase()));
        assert!(language_codes.insert(language));
    }
    assert!(language_codes.contains("en"));
    assert!(language_codes.contains("zh"));
    assert!(unique.language_code().is_err());

    let mut locales = std::collections::HashSet::new();
    // 固定 provider 的 255 项经 CLDR 规范化后有 252 个不同标签：
    // sh_CS／sh_YU 合并，sr_CS／sr_RS／sr_YU 合并。
    for _ in 0..252 {
        let locale = unique.random_locale()?;
        assert_eq!(
            openrust::localization::canonical_locale(locale)
                .as_deref()
                .ok(),
            Some(locale)
        );
        assert!(locales.insert(locale));
    }
    for locale in ["en-GB", "en-US", "zh-CN", "zh-HK", "zh-TW"] {
        assert!(locales.contains(locale));
    }
    assert!(unique.random_locale().is_err());

    fn check_codes(
        next: impl Fn() -> DatabaseResult<&'static str>,
        count: usize,
        length: usize,
    ) -> DatabaseResult<()> {
        let mut values = std::collections::HashSet::new();
        for _ in 0..count {
            let value = next()?;
            assert_eq!(value.len(), length);
            assert!(value.bytes().all(|byte| byte.is_ascii_uppercase()));
            assert!(values.insert(value));
        }
        assert!(next().is_err());
        Ok(())
    }

    check_codes(|| unique.country_code(), 247, 2)?;
    check_codes(|| unique.country_iso_alpha3(), 249, 3)?;
    check_codes(|| unique.currency_code(), 154, 3)?;
    let mut emoji = (0..56)
        .map(|_| unique.emoji())
        .collect::<DatabaseResult<Vec<_>>>()?;
    emoji.sort_unstable();
    assert_eq!(emoji.first(), Some(&"😀"));
    assert_eq!(emoji.last(), Some(&"😷"));
    assert!(unique.emoji().is_err());
    assert_eq!(faker.locale(), "zh-Hant-TW");
    assert!(faker.unique(true).emoji().is_ok());
    Ok(())
}

#[test]
fn faker_digest_samples_are_lowercase_hex_and_support_unique() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    fn check_samples(
        next: impl Fn() -> DatabaseResult<String>,
        length: usize,
    ) -> DatabaseResult<()> {
        let mut samples = std::collections::HashSet::new();
        for _ in 0..32 {
            let sample = next()?;
            assert_eq!(sample.len(), length);
            assert!(
                sample
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            );
            assert!(samples.insert(sample));
        }
        Ok(())
    }

    let faker = Faker::new("en-US")?;
    let unique = faker.unique(false);
    check_samples(|| unique.md5(), 32)?;
    check_samples(|| unique.sha1(), 40)?;
    check_samples(|| unique.sha256(), 64)?;
    assert_eq!(faker.unique(true).sha256()?.len(), 64);
    Ok(())
}

#[test]
fn faker_network_addresses_follow_expected_formats_and_ranges() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;
    use std::net::{Ipv4Addr, Ipv6Addr};

    let faker = Faker::new("en-US")?;
    let unique = faker.unique(false);
    let mut addresses = std::collections::HashSet::new();
    for _ in 0..256 {
        let sample = unique.ipv4()?;
        let address = u32::from(sample.parse::<Ipv4Addr>().unwrap());
        assert!((0x0100_0000..=0xffff_fffe).contains(&address));
        assert!(addresses.insert(sample));

        let sample = unique.local_ipv4()?;
        assert!(sample.parse::<Ipv4Addr>().unwrap().is_private());
        assert!(addresses.insert(format!("private:{sample}")));

        let sample = unique.ipv6()?;
        assert!(sample.parse::<Ipv6Addr>().is_ok());
        let groups: Vec<_> = sample.split(':').collect();
        assert_eq!(groups.len(), 8);
        for group in groups {
            assert_eq!(
                group,
                format!("{:x}", u16::from_str_radix(group, 16).unwrap())
            );
        }
        assert!(addresses.insert(sample));

        let sample = unique.mac_address()?;
        let octets: Vec<_> = sample.split(':').collect();
        assert_eq!(octets.len(), 6);
        for octet in octets {
            assert_eq!(
                octet,
                format!("{:02X}", u8::from_str_radix(octet, 16).unwrap())
            );
        }
        assert!(addresses.insert(sample));
    }
    assert!(faker.unique(true).ipv4().is_ok());
    Ok(())
}

#[test]
fn faker_internet_formats_use_localized_suffixes_and_upstream_slug_counts() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    let faker = Faker::new("en-US")?;
    assert!(faker.slug(Some(0), true).is_empty());
    for _ in 0..64 {
        assert_eq!(faker.slug(Some(3), false).split('-').count(), 3);
        assert!((4..=9).contains(&faker.slug(None, true).split('-').count()));
        let domain = faker.domain_name()?;
        let (name, suffix) = domain.rsplit_once('.').unwrap();
        assert!(!name.is_empty());
        assert!(
            name.bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.'))
        );
        assert!(["com", "biz", "info", "net", "org"].contains(&suffix));
        let address = faker.url()?;
        let (scheme, host_path) = address.split_once("://").unwrap();
        let (host, path) = host_path.split_once('/').unwrap();
        assert!(matches!(scheme, "http" | "https"));
        assert!(!host.is_empty() && !host.contains(char::is_whitespace));
        if !path.is_empty() {
            let slug = path.strip_suffix(".html").unwrap_or(path);
            assert!((4..=9).contains(&slug.split('-').count()));
        }
    }

    for (locale, expected) in [
        (
            "zh-Hans",
            &[
                "biz", "biz.cn", "cn", "com", "com.cn", "edu.cn", "gov.cn", "info", "net",
                "net.cn", "org", "org.cn",
            ][..],
        ),
        ("zh-Hant", &["biz", "com", "info", "net", "org"][..]),
        ("fr-FR", &["com", "fr", "net", "org"][..]),
    ] {
        let faker = Faker::new(locale)?;
        let unique = faker.unique(false);
        let mut suffixes = (0..expected.len())
            .map(|_| unique.tld())
            .collect::<DatabaseResult<Vec<_>>>()?;
        suffixes.sort_unstable();
        assert_eq!(suffixes, expected);
        assert!(unique.tld().is_err());
        assert!(faker.unique(true).tld().is_ok());
    }
    let unique = faker.unique(false);
    assert_eq!(unique.slug(Some(0), false)?, "");
    assert!(unique.slug(Some(0), true).is_err());
    assert!(unique.domain_word().is_ok());
    assert!(unique.domain_name().is_ok());
    assert!(unique.url().is_ok());
    Ok(())
}

#[test]
fn faker_mail_and_password_follow_internet_formatter_contracts() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    let faker = Faker::new("zh-Hans-CN")?;
    let free_domains = [
        "gmail.com",
        "yahoo.com",
        "hotmail.com",
        "126.com",
        "163.com",
        "qq.com",
        "sohu.com",
        "sina.com",
    ];
    for _ in 0..64 {
        let username = faker.user_name();
        assert!(!username.trim_matches(['.', '_']).is_empty());
        assert!(
            username
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_'))
        );
        for sample in [faker.free_email(), faker.email()?] {
            let (name, domain) = sample.split_once('@').unwrap();
            assert!(!name.is_empty());
            assert!(free_domains.contains(&domain));
        }
        for sample in [faker.safe_email(), faker.company_email()?] {
            assert_eq!(sample.matches('@').count(), 1);
            assert!(!sample.contains(char::is_whitespace));
        }
        assert!(matches!(
            faker.safe_email_domain(),
            "example.com" | "example.org" | "example.net"
        ));
        let password = faker.password(None, None)?;
        assert!((6..=20).contains(&password.len()));
        assert!(password.bytes().all(|byte| (b'!'..=b'~').contains(&byte)));
        assert!((3..=5).contains(&faker.password(Some(5), Some(3))?.len()));
    }
    assert_eq!(faker.password(Some(0), Some(0))?, "");
    assert_eq!(faker.password(Some(8), Some(8))?.len(), 8);
    assert!(faker.password(Some(usize::MAX), Some(usize::MAX)).is_err());

    let unique = faker.unique(false);
    let mut sampled = (0..free_domains.len())
        .map(|_| unique.free_email_domain())
        .collect::<DatabaseResult<Vec<_>>>()?;
    sampled.sort_unstable();
    let mut expected = free_domains;
    expected.sort_unstable();
    assert_eq!(sampled, expected);
    assert!(unique.free_email_domain().is_err());
    assert!(unique.email().is_ok());
    assert!(unique.company_email().is_ok());
    assert!(unique.safe_email().is_ok());
    assert!(unique.free_email().is_ok());
    assert!(unique.user_name().is_ok());
    assert_eq!(unique.password(Some(0), Some(0))?, "");
    assert!(unique.password(Some(0), Some(0)).is_err());
    assert!(faker.unique(true).password(Some(0), Some(0)).is_ok());
    Ok(())
}

#[test]
fn faker_phone_numbers_and_imei_follow_pinned_formats() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    let cn = Faker::new("zh-Hans-CN")?;
    let tw = Faker::new("zh-Hant-TW")?;
    for _ in 0..128 {
        let number = cn.phone_number();
        assert_eq!(number.len(), 11);
        assert!(number.starts_with("1") && number.bytes().all(|byte| byte.is_ascii_digit()));
        let number = tw.phone_number();
        assert!(number.starts_with("0") || number.starts_with("(0") || number.starts_with("+886"));
        assert!(
            number
                .bytes()
                .all(|byte| byte.is_ascii_digit() || b"+()-".contains(&byte))
        );
        let imei = cn.imei();
        assert_eq!(imei.len(), 15);
        assert!(imei.bytes().all(|byte| byte.is_ascii_digit()));
        let sum: u32 = imei
            .bytes()
            .rev()
            .enumerate()
            .map(|(index, byte)| {
                let digit = u32::from(byte - b'0') * (1 + (index % 2) as u32);
                if digit > 9 { digit - 9 } else { digit }
            })
            .sum();
        assert_eq!(sum % 10, 0);
    }

    for (locale, prefix, lengths) in [
        ("en-US", "+1", 12..=12),
        ("fr-FR", "+33", 12..=12),
        ("de-DE", "+49", 12..=15),
        ("en-GB", "+44", 13..=13),
        ("tr-TR", "+90", 13..=13),
    ] {
        let faker = Faker::new(locale)?;
        let unique = faker.unique(false);
        let mut numbers = std::collections::HashSet::new();
        for _ in 0..128 {
            let number = unique.e164_phone_number()?;
            assert!(number.starts_with(prefix));
            assert!(lengths.contains(&number.len()), "{locale}: {number}");
            assert!(number[1..].bytes().all(|byte| byte.is_ascii_digit()));
            assert!(numbers.insert(number));
        }
    }
    assert!(tw.unique(false).phone_number().is_ok());
    assert!(tw.unique(true).imei().is_ok());
    Ok(())
}

#[test]
fn faker_address_and_title_formats_follow_locale_contracts() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    let us = Faker::new("en-US")?;
    let cn = Faker::new("zh-Hans-CN")?;
    let tw = Faker::new("zh-Hant-TW")?;
    let fr = Faker::new("fr-FR")?;
    for _ in 0..64 {
        let address = us.address();
        let (street, locality) = address.split_once('\n').unwrap();
        assert!(street.starts_with(|character: char| character.is_ascii_digit()));
        let (_, state_zip) = locality.rsplit_once(", ").unwrap();
        let (state, zip) = state_zip.split_once(' ').unwrap();
        assert_eq!(state.len(), 2);
        assert!(matches!(zip.len(), 5 | 10));
        assert!(
            zip.bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-')
        );
        assert!(matches!(us.building_number().len(), 3..=5));
        assert!(us.secondary_address().starts_with(['A', 'S']));

        let address = cn.address();
        assert!(address.ends_with('区') && !address.contains('\n'));
        let postcode = cn.postcode();
        assert_eq!(postcode.len(), 6);
        assert!(postcode.ends_with("00"));
        assert!((1..=85).contains(&postcode[..2].parse::<u8>().unwrap()));
        assert!((10..=88).contains(&postcode[2..4].parse::<u8>().unwrap()));

        let street = tw.street_address();
        assert!(street.contains('號'));
        assert!(street.ends_with(['號', '樓']));
        assert!(tw.address().contains('號'));
        assert!(tw.secondary_address().ends_with(['室', '房']));

        let address = fr.address();
        let (_, locality) = address.split_once('\n').unwrap();
        let (postcode, _) = locality.split_once(' ').unwrap();
        assert_eq!(postcode.len(), 5);
        assert!(postcode.bytes().all(|byte| byte.is_ascii_digit()));

        assert!(["先生", "博士", "教授"].contains(&tw.title(Some("male"))));
        assert!(["小姐", "太太", "博士", "教授"].contains(&tw.title_female()));
        assert!(
            [
                "女士", "小姐", "夫人", "太太", "医生", "老师", "教授", "律师", "经理"
            ]
            .contains(&cn.title(Some("female")))
        );
        assert!(!us.country().is_empty());
        assert!(!us.city_prefix().is_empty());
        assert!(!us.city_suffix().is_empty());
        assert!(!us.street_suffix().is_empty());
        assert!(!us.state().is_empty());
    }
    let unique = tw.unique(false);
    let mut titles = (0..3)
        .map(|_| unique.title_male())
        .collect::<DatabaseResult<Vec<_>>>()?;
    titles.sort_unstable();
    assert_eq!(titles, ["先生", "博士", "教授"]);
    assert!(unique.title_male().is_err());
    assert!(unique.title(None).is_ok());
    assert!(unique.address().is_ok());
    assert!(tw.unique(true).title_male().is_ok());
    Ok(())
}

#[test]
fn faker_gendered_names_follow_catalogs_and_share_unique_history() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    for locale in [
        "ar-SA", "de-DE", "fa-IR", "fr-FR", "it-IT", "ja-JP", "nl-NL", "pt-BR", "pt-PT", "tr-TR",
        "zh-Hans", "zh-Hant", "en-US", "cy-GB",
    ] {
        let faker = Faker::new(locale)?;
        for _ in 0..64 {
            assert!(!faker.first_name_male().is_empty());
            assert!(!faker.first_name_female().is_empty());
            for gender in [Some("male"), Some("female"), None, Some("other")] {
                let name = faker.name_with_gender(gender);
                assert!(!name.trim().is_empty());
                if matches!(locale, "zh-Hans" | "zh-Hant") {
                    assert!(name.chars().all(|character| !character.is_ascii()));
                }
            }
        }
    }

    let faker = Faker::new("zh-Hant-TW")?;
    let unique = faker.unique(false);
    let names = (0..64)
        .map(|_| unique.first_name_with_gender(Some("female")))
        .collect::<DatabaseResult<std::collections::HashSet<_>>>()?;
    assert_eq!(names.len(), 64);
    assert!(
        names
            .iter()
            .all(|name| (1..=2).contains(&name.chars().count()))
    );
    assert!(unique.first_name_male().is_ok());
    assert!(unique.first_name_female().is_ok());
    assert!(unique.name_with_gender(Some("male")).is_ok());
    assert!(unique.name().is_ok());
    assert!(faker.unique(true).first_name_with_gender(None).is_ok());
    Ok(())
}

#[test]
fn faker_coordinates_preserve_bounds_precision_and_unique_history() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    let global = Faker::new("en-US")?;
    let taiwan = Faker::new("zh-Hant-TW")?;
    let hungary = Faker::new("hu-HU")?;
    let montenegro = Faker::new("me-ME")?;
    for _ in 0..64 {
        for (faker, latitude, longitude) in [
            (&global, -90.0..=90.0, -180.0..=180.0),
            (&taiwan, -90.0..=90.0, -180.0..=180.0),
            (&hungary, 46.262740..=47.564721, 17.077949..=20.604560),
            (&montenegro, 42.43..=42.45, 19.16..=19.27),
        ] {
            let sample = faker.local_coordinates()?;
            assert!(latitude.contains(&sample.latitude));
            assert!(longitude.contains(&sample.longitude));
            for coordinate in [sample.latitude, sample.longitude] {
                assert_eq!(
                    coordinate,
                    format!("{coordinate:.6}").parse::<f64>().unwrap()
                );
            }
        }
        assert!((22.0..=25.0).contains(&taiwan.local_latitude()?));
        assert!((120.0..=122.0).contains(&taiwan.local_longitude()?));
        assert!((10.0..=12.0).contains(&global.latitude(Some(12.0), Some(10.0))?));
    }
    assert_eq!(
        global.longitude(Some(0.1234567), Some(0.1234567))?,
        0.123457
    );
    assert!(global.latitude(Some(f64::NAN), None).is_err());
    assert!(global.longitude(None, Some(f64::INFINITY)).is_err());
    let unique = global.unique(false);
    assert_eq!(unique.latitude(Some(-0.0), Some(-0.0))?, 0.0);
    assert!(unique.latitude(Some(0.0), Some(0.0)).is_err());
    assert_eq!(unique.longitude(Some(0.0), Some(0.0))?, 0.0);
    assert!(unique.local_coordinates().is_ok());
    assert!(taiwan.unique(false).local_latitude().is_ok());
    assert!(taiwan.unique(false).local_longitude().is_ok());
    Ok(())
}

struct TrashedProbe(ModelState);

impl Model for TrashedProbe {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("trashed_factory_probe")
            .timestamps(None, None)
            .soft_deletes("removed_at");
        &METADATA
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

async fn verify_trashed_factory(db: &Connection) -> DatabaseResult<()> {
    db.get_schema_builder()
        .create("trashed_factory_probe", |table| {
            table.id();
            table.string("name", 60);
            table.date_time("removed_at", 0).nullable();
        })
        .await?;
    let definition = || Attributes::from([("name".into(), "sample".into())]);
    let now = jiff::Timestamp::now();
    let batch = Factory::<TrashedProbe>::new(definition)
        .count(2)
        .trashed(None)?;
    let previews = batch.make_on(db).await?;
    assert_eq!(previews.len(), 2);
    let mut previous = None;
    for model in previews.iter() {
        assert!(model.trashed());
        let Some(Value::Text(value)) = model.get_attribute_value("removed_at")? else {
            panic!("expected date text");
        };
        let raw = value.parse::<jiff::Timestamp>().unwrap();
        assert!((86_399..=86_401).contains(&now.duration_since(raw).as_secs()));
        if let Some(previous) = &previous {
            assert_eq!(&value, previous);
        }
        previous = Some(value);
    }
    assert!(
        TrashedProbe::query_on(db)
            .with_trashed()
            .get()
            .await?
            .is_empty()
    );
    assert_eq!(batch.create_on(db).await?.len(), 2);

    let explicit = "2020-02-03 04:05:06";
    let model = Factory::<TrashedProbe>::new(definition)
        .trashed(Some(explicit.into()))?
        .create_on(db)
        .await?;
    assert!(model.trashed());
    assert!(
        db.table("trashed_factory_probe")
            .pluck("removed_at")
            .await?
            .contains(&explicit.into())
    );

    let restored = Factory::<TrashedProbe>::new(definition)
        .trashed(Some(Value::Null))?
        .set("removed_at", Value::Null)
        .create_on(db)
        .await?;
    assert!(!restored.trashed());
    assert_eq!(TrashedProbe::query_on(db).get().await?.len(), 1);
    assert_eq!(
        TrashedProbe::query_on(db).only_trashed().get().await?.len(),
        3
    );
    db.get_schema_builder()
        .drop("trashed_factory_probe")
        .await?;
    Ok(())
}

#[tokio::test]
async fn factory_trashed_state_respects_scopes_on_sqlite_and_mysql() {
    let root = Root::new();
    verify_trashed_factory(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_trashed_factory(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    assert!(matches!(
        Factory::<FactoryChild>::new(Attributes::new).trashed(None),
        Err(DatabaseError::InvalidQuery(
            "soft deletes are disabled for this model"
        ))
    ));
    root.close().await;
}

struct ConditionalProbe(ModelState);

impl Model for ConditionalProbe {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata =
            ModelMetadata::new("conditional_factory_probe").timestamps(None, None);
        &METADATA
    }
    fn from_state(state: ModelState) -> Self {
        Self(state)
    }
    fn state(&self) -> &ModelState {
        &self.0
    }
    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

async fn verify_conditional_factory(db: &Connection) -> DatabaseResult<()> {
    db.get_schema_builder()
        .create("conditional_factory_probe", |table| {
            table.id();
            table.string("name", 60);
        })
        .await?;
    let definition = || Attributes::from([("name".into(), "default".into())]);
    let single = Factory::<ConditionalProbe>::new(definition)
        .when(false, |_| panic!("false when callback"))
        .unless(true, |_| panic!("true unless callback"))
        .when(true, |factory| factory.set("name", "when"))
        .unless_else(
            true,
            |_| panic!("true unless callback"),
            |factory| {
                factory.state(|attributes| {
                    assert_eq!(attributes.get("name"), Some(&"when".into()));
                    Attributes::from([("name".into(), "unless fallback".into())])
                })
            },
        );
    assert_eq!(
        single.raw_on(db).await?.get("name"),
        Some(&"unless fallback".into())
    );
    single.create_on(db).await?;

    let batch = Factory::<ConditionalProbe>::new(definition)
        .count(2)
        .when_else(
            false,
            |_| panic!("false when callback"),
            |batch| batch.set("name", "when fallback"),
        )
        .unless(false, |batch| {
            batch.state(|attributes| {
                assert_eq!(attributes.get("name"), Some(&"when fallback".into()));
                Attributes::from([("name".into(), "unless".into())])
            })
        })
        .when(true, |batch| batch.set("name", "batch"));
    assert!(
        batch
            .make_on(db)
            .await?
            .iter()
            .all(|model| model.get_attribute("name") == Some(&"batch".into()))
    );
    assert_eq!(batch.create_on(db).await?.len(), 2);
    assert_eq!(
        db.table("conditional_factory_probe")
            .order_by("id", "asc")
            .pluck("name")
            .await?,
        ["unless fallback".into(), "batch".into(), "batch".into()]
    );
    db.get_schema_builder()
        .drop("conditional_factory_probe")
        .await?;
    Ok(())
}

#[tokio::test]
async fn factory_conditionals_select_only_one_branch_and_keep_batch_count() {
    let root = Root::new();
    verify_conditional_factory(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_conditional_factory(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

struct BoundFactory;

impl FactoryDefinition for BoundFactory {
    type Model = FactoryChild;

    fn definition() -> Attributes {
        Attributes::from([("label".into(), "defined".into())])
    }

    fn configure(factory: Factory<Self::Model>) -> Factory<Self::Model> {
        factory.state(|attributes| {
            assert_eq!(attributes.get("label"), Some(&"defined".into()));
            Attributes::from([("label".into(), "configured".into())])
        })
    }
}

impl HasFactory for FactoryChild {
    fn factory() -> Factory<Self> {
        BoundFactory::factory()
    }
}

#[tokio::test]
async fn model_factory_binding_configures_each_fresh_builder() {
    let root = Root::new();
    let connection = root.connection(None, "database/database.sqlite");

    let configured = FactoryChild::factory();
    let overridden = FactoryChild::factory().set("label", "explicit");
    assert_eq!(
        configured.raw_on(&connection).await.unwrap().get("label"),
        Some(&"configured".into())
    );
    assert_eq!(
        overridden.raw_on(&connection).await.unwrap().get("label"),
        Some(&"explicit".into())
    );
    assert_eq!(
        BoundFactory::factory()
            .raw_on(&connection)
            .await
            .unwrap()
            .get("label"),
        Some(&"configured".into())
    );
}
struct ParentProbe(ModelState);
struct ChildProbe(ModelState);

macro_rules! probe_model {
    ($model:ident, $table:literal) => {
        impl Model for $model {
            fn metadata() -> &'static ModelMetadata {
                static METADATA: ModelMetadata = ModelMetadata::new($table).timestamps(None, None);
                &METADATA
            }
            fn from_state(state: ModelState) -> Self {
                Self(state)
            }
            fn state(&self) -> &ModelState {
                &self.0
            }
            fn state_mut(&mut self) -> &mut ModelState {
                &mut self.0
            }
        }
    };
}

probe_model!(ParentProbe, "factory_parent_probe");
probe_model!(ChildProbe, "factory_child_probe");

async fn verify_relation_parent(db: &Connection) -> DatabaseResult<()> {
    let mut schema = db.get_schema_builder();
    schema
        .create("factory_parent_probe", |table| {
            table.id();
            table.string("name", 60);
        })
        .await?;
    schema
        .create("factory_child_probe", |table| {
            table.id();
            table.unsigned_big_integer("parent_id").nullable();
            table.string("name", 60);
        })
        .await?;
    schema
        .create("factory_parent_links", |table| {
            table.unsigned_big_integer("parent_id");
            table.unsigned_big_integer("child_id");
        })
        .await?;

    let standalone =
        Factory::<ChildProbe>::new(|| Attributes::from([("name".into(), "standalone".into())]))
            .state_with_parent::<ParentProbe>(|_, parent| {
                assert!(parent.is_none());
                Attributes::new()
            })
            .after_creating_with_parent::<ParentProbe>(|_, parent| {
                assert!(parent.is_none());
                Ok(())
            });
    standalone.create_on(db).await?;

    let children = |attached| {
        Factory::<ChildProbe>::new(|| Attributes::from([("name".into(), "base".into())]))
            .state_with_parent::<ChildProbe>(|_, parent| {
                assert!(parent.is_none());
                Attributes::new()
            })
            .count(2)
            .state_with_parent::<ParentProbe>(move |attributes, parent| {
                assert_eq!(attributes.get("name"), Some(&"base".into()));
                let parent = parent.unwrap();
                assert!(parent.get_attribute("id").is_some());
                let name = if attached { "attached" } else { "direct" };
                assert_eq!(parent.get_attribute("name"), Some(&"owner".into()));
                Attributes::from([("name".into(), name.into())])
            })
            .sequence_with_parent::<ParentProbe>(|index, attributes, parent| {
                assert!(parent.is_some_and(|model| model.state().exists()));
                let Value::Text(name) = &attributes["name"] else {
                    panic!("earlier state did not set the child name");
                };
                Attributes::from([("name".into(), format!("{name}-{index}").into())])
            })
            .after_creating_with_parent::<ParentProbe>(move |child, parent| {
                let parent = parent.unwrap();
                assert!(child.get_attribute("id").is_some());
                if !attached {
                    assert_eq!(child.get_attribute("parent_id"), parent.get_attribute("id"));
                }
                assert_eq!(parent.get_attribute("name"), Some(&"owner".into()));
                Ok(())
            })
    };

    Factory::<ParentProbe>::new(|| Attributes::from([("name".into(), "owner".into())]))
        .has(
            openrust::database::eloquent::relations::HasMany::<ParentProbe, ChildProbe>::new(
                "id",
                "parent_id",
            ),
            children(false),
        )
        .has_attached(
            openrust::database::eloquent::relations::BelongsToMany::<ParentProbe, ChildProbe>::new(
                "factory_parent_links",
                "parent_id",
                "child_id",
                "id",
                "id",
            ),
            children(true),
            Attributes::new(),
        )
        .create_on(db)
        .await?;
    assert_eq!(
        db.table("factory_child_probe")
            .order_by("id", "asc")
            .pluck("name")
            .await?,
        [
            "standalone",
            "direct-0",
            "direct-1",
            "attached-0",
            "attached-1"
        ]
        .map(Value::from)
    );
    assert_eq!(db.table("factory_parent_links").count("*").await?, 2);
    for table in [
        "factory_parent_links",
        "factory_child_probe",
        "factory_parent_probe",
    ] {
        schema.drop(table).await?;
    }
    Ok(())
}

#[tokio::test]
async fn relation_factories_supply_parent_to_states_and_create_callbacks() {
    let root = Root::new();
    verify_relation_parent(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_relation_parent(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

struct ExpansionOwner(ModelState);
struct ExpansionChild(ModelState);

factory_model!(
    ExpansionOwner,
    "expansion_owners",
    NamedRelations::default()
);
factory_model!(
    ExpansionChild,
    "expansion_children",
    NamedRelations::default()
);

async fn verify_expansion(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_children", |table| {
            table.id();
            table.unsigned_big_integer("owner_id").nullable();
        })
        .await?;

    let factory = || {
        Factory::<ExpansionChild>::new(Attributes::new)
            .parent_attribute("owner_id", Factory::<ExpansionOwner>::new(Attributes::new))
    };
    let before = factory();
    Factory::<ExpansionOwner>::dont_expand_relationships_by_default();
    let disabled = factory()
        .count(2)
        .without_parents(vec![FactoryParentSelector::attribute("other")]);
    let enabled = {
        Factory::<ExpansionChild>::expand_relationships_by_default();
        factory()
    };
    let owner = Factory::<ExpansionOwner>::new(Attributes::new)
        .create_on(connection)
        .await?;
    let key = owner.get_key().unwrap().clone();
    assert!(
        disabled
            .recycle(owner)
            .raw_on(connection)
            .await?
            .iter()
            .all(|row| row["owner_id"] == Value::Null)
    );
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    Factory::<ExpansionOwner>::dont_expand_relationships_by_default();
    let overridden = factory()
        .set("owner_id", key.clone())
        .create_on(connection)
        .await?;
    assert_eq!(overridden.get_attribute("owner_id"), Some(&key));
    let relation = BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id");
    let owner = ExpansionOwner::query_on(connection)
        .get()
        .await?
        .into_iter()
        .next()
        .unwrap();
    let stored_key = owner.get_key().unwrap().clone();
    let explicit = Factory::<ExpansionChild>::new(Attributes::new)
        .for_existing(relation, owner)
        .create_on(connection)
        .await?;
    assert_eq!(explicit.get_attribute("owner_id"), Some(&stored_key));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    let suppressed = enabled
        .without_parents(vec![])
        .without_parents(vec![FactoryParentSelector::attribute("other")]);
    assert_eq!(
        suppressed.raw_on(connection).await?["owner_id"],
        Value::Null
    );
    assert_ne!(
        before
            .create_on(connection)
            .await?
            .get_attribute("owner_id"),
        Some(&Value::Null)
    );
    Factory::<ExpansionChild>::flush_state();
    assert_ne!(
        factory()
            .create_on(connection)
            .await?
            .get_attribute("owner_id"),
        Some(&Value::Null)
    );
    assert_eq!(connection.table("expansion_owners").count("*").await?, 3);
    assert_eq!(connection.table("expansion_children").count("*").await?, 4);
    schema.drop("expansion_children").await?;
    schema.drop("expansion_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_relationship_expansion_defaults_are_snapshotted() {
    if std::env::var_os("EXPANSION_TEST_CHILD").is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "factory_relationship_expansion_defaults_are_snapshotted",
                "--nocapture",
            ])
            .env("EXPANSION_TEST_CHILD", "1")
            .output()
            .unwrap();
        success(output);
        return;
    }
    let root = Root::new();
    verify_expansion(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_expansion(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

async fn verify_deferred_factory_owner(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_children", |table| {
            table.id();
            table.unsigned_big_integer("owner_id").nullable();
            table.string("owner_type", 120).nullable();
        })
        .await?;

    let owners = || Factory::<ExpansionOwner>::new(Attributes::new);
    let children = || {
        Factory::<ExpansionChild>::new(|| Attributes::from([("owner_id".into(), 98_i64.into())]))
    };
    let relation =
        || BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id").named("owner");

    let skipped = children()
        .r#for(relation(), owners().connection(Some("missing")))
        .set("owner_id", Value::Null)
        .raw_on(connection)
        .await?;
    assert_eq!(skipped["owner_id"], Value::Null);

    let overridden = children()
        .r#for(relation(), owners())
        .make_one_on(
            connection,
            Attributes::from([("owner_id".into(), 5_i64.into())]),
        )
        .await?;
    assert_eq!(overridden.get_attribute("owner_id"), Some(&5_i64.into()));
    assert!(!overridden.relation_loaded("owner"));

    let morph_class = ExpansionOwner::from_state(ModelState::default()).get_morph_class()?;
    let morph = MorphTo::<ExpansionChild>::new("owner_type", "owner_id")
        .target::<ExpansionOwner>(morph_class, "id");
    let morph_preview = children()
        .r#for(morph, owners())
        .state(move |attributes| {
            assert_eq!(attributes["owner_type"], morph_class.into());
            Attributes::from([("owner_id".into(), Value::Null)])
        })
        .raw_on(connection)
        .await?;
    assert_eq!(morph_preview["owner_id"], Value::Null);
    assert_eq!(morph_preview["owner_type"], morph_class.into());
    assert_eq!(connection.table("expansion_owners").count("*").await?, 0);

    let records = children()
        .parent_attribute("owner_id", owners().connection(Some("missing")))
        .r#for(relation(), owners().connection(Some("missing")))
        .r#for(relation(), owners())
        .sequence(|index| {
            if index == 0 {
                Attributes::from([("owner_id".into(), 5_i64.into())])
            } else {
                Attributes::new()
            }
        })
        .count(3)
        .create_on(connection)
        .await?;
    assert_eq!(records[0].get_attribute("owner_id"), Some(&5_i64.into()));
    assert!(!records[0].relation_loaded("owner"));
    let generated_key = records[1].get_attribute("owner_id").unwrap();
    assert_ne!(*generated_key, 98_i64.into());
    assert_eq!(Some(generated_key), records[2].get_attribute("owner_id"));
    assert!(records[1].relation_loaded("owner"));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    let owner = ExpansionOwner::query_on(connection)
        .find_or_fail(generated_key.clone())
        .await?;
    let replaced = children()
        .for_existing(relation(), owner)
        .set("owner_id", Value::Null)
        .create_on(connection)
        .await?;
    assert_eq!(replaced.get_attribute("owner_id"), Some(&Value::Null));
    assert!(!replaced.relation_loaded("owner"));
    assert_eq!(connection.table("expansion_children").count("*").await?, 4);
    schema.drop("expansion_children").await?;
    schema.drop("expansion_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_owner_resolution_skips_overwritten_keys() {
    let root = Root::new();
    verify_deferred_factory_owner(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_deferred_factory_owner(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

async fn verify_factory_parent_cache(
    connection: &Connection,
    independent: &Connection,
) -> DatabaseResult<()> {
    let owners = || Factory::<ExpansionOwner>::new(Attributes::new);
    let factory = || {
        Factory::<ExpansionChild>::new(Attributes::new).r#for(
            BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id"),
            owners(),
        )
    };

    // A failed asynchronous resolution must be retryable on the same builder.
    let factory = factory();
    assert!(factory.raw_on(connection).await.is_err());
    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    let (raw, model) = tokio::join!(factory.raw_on(connection), factory.make_on(connection));
    let raw = raw?;
    let model = model?;
    assert_eq!(Some(&raw["owner_id"]), model.get_attribute("owner_id"));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    let batch = factory.count(2);
    assert!(
        batch
            .raw_on(connection)
            .await?
            .iter()
            .all(|row| row["owner_id"] == raw["owner_id"])
    );
    assert!(batch.raw_on(independent).await.is_err());
    let mut independent_schema = independent.get_schema_builder();
    independent_schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    independent
        .table("expansion_owners")
        .insert(&[Attributes::from([("id".into(), 100_i64.into())])])
        .await?;
    let independent_result = batch.raw_on(independent).await?;
    assert_ne!(independent_result[0]["owner_id"], raw["owner_id"]);
    assert_eq!(
        independent_result[0]["owner_id"],
        independent_result[1]["owner_id"]
    );
    assert_eq!(independent.table("expansion_owners").count("*").await?, 2);
    assert_eq!(
        batch.raw_on(connection).await?[0]["owner_id"],
        raw["owner_id"]
    );

    let local_owner = ExpansionOwner::query_on(connection)
        .find_or_fail(raw["owner_id"].clone())
        .await?;
    let local_key = local_owner.get_key().unwrap().clone();
    let remote_owner = ExpansionOwner::query_on(independent)
        .find_or_fail(independent_result[0]["owner_id"].clone())
        .await?;
    let remote_key = remote_owner.get_key().unwrap().clone();
    let recycled = Factory::<ExpansionChild>::new(Attributes::new);
    assert!(
        recycled
            .get_random_recycled_model::<ExpansionOwner>()
            .is_none()
    );
    let recycled = recycled.recycle(ExpansionOwner::from_state(ModelState::default()));
    assert!(
        recycled
            .get_random_recycled_model::<ExpansionOwner>()
            .is_none()
    );
    let recycled = recycled
        .recycle(local_owner)
        .recycle(remote_owner)
        .r#for(
            BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id"),
            owners(),
        )
        .count(2);
    assert!(
        recycled
            .get_random_recycled_model::<ExpansionChild>()
            .is_none()
    );
    let keys = [&local_key, &remote_key];
    assert!(
        keys.contains(
            &recycled
                .get_random_recycled_model::<ExpansionOwner>()
                .unwrap()
                .get_key()
                .unwrap()
        )
    );
    assert!(
        recycled
            .raw_on(connection)
            .await?
            .iter()
            .all(|row| row["owner_id"] == local_key)
    );
    assert!(
        recycled
            .raw_on(independent)
            .await?
            .iter()
            .all(|row| row["owner_id"] == remote_key)
    );
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);
    assert_eq!(independent.table("expansion_owners").count("*").await?, 2);
    independent_schema.drop("expansion_owners").await?;
    schema.drop("expansion_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_parent_cache_survives_calls_and_respects_connections() {
    let root = Root::new();
    let sqlite = root.connection(None, "database/database.sqlite");
    let other = root.connection(None, "database/alternate.sqlite");
    verify_factory_parent_cache(&sqlite, &other).await.unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        let mysql = root.connection(Some(&url), "");
        verify_factory_parent_cache(&mysql, &other).await.unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

struct ShortcutModel(ModelState);

impl Model for ShortcutModel {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("factory_shortcuts")
            .timestamps(None, None)
            .fillable(&["label"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }
}

struct ShortcutFactory;

impl FactoryDefinition for ShortcutFactory {
    type Model = ShortcutModel;

    fn definition() -> Attributes {
        Attributes::from([("label".into(), "definition".into())])
    }

    fn configure(factory: Factory<Self::Model>) -> Factory<Self::Model> {
        factory.state(|attributes| {
            let label = match &attributes["label"] {
                Value::Text(label) => format!("{label}-configured"),
                other => panic!("unexpected label {other:?}"),
            };
            Attributes::from([("label".into(), label.into())])
        })
    }
}

async fn verify_factory_shortcuts(connection: &Connection) -> DatabaseResult<()> {
    let initial = ShortcutFactory::new(Attributes::from([("label".into(), "initial".into())]));
    assert_eq!(initial.model_name(), std::any::type_name::<ShortcutModel>());
    let attributes = Attributes::from([("label".into(), "direct".into())]);
    let unsaved = initial.new_model(attributes.clone())?;
    assert!(!unsaved.state().exists());
    assert_eq!(unsaved.get_attribute("label"), Some(&"direct".into()));
    assert!(
        initial
            .new_model(Attributes::new())?
            .get_attribute("label")
            .is_none()
    );
    let guarded = initial.new_model(Attributes::from([
        ("label".into(), "direct".into()),
        ("id".into(), 9_i64.into()),
    ]))?;
    assert!(guarded.get_attribute("id").is_none());
    let computed = ShortcutFactory::new_with_state(|attributes| {
        assert_eq!(attributes["label"], "definition".into());
        Attributes::from([("label".into(), "computed".into())])
    });
    assert_eq!(
        computed.raw_on(connection).await?["label"],
        "computed-configured".into()
    );

    assert_eq!(
        initial.raw_on(connection).await?["label"],
        "initial-configured".into()
    );
    assert_eq!(
        initial.set("label", "override").raw_on(connection).await?["label"],
        "override".into()
    );

    let batch = ShortcutFactory::times(2);
    assert_eq!(batch.model_name(), std::any::type_name::<ShortcutModel>());
    assert_eq!(
        batch.new_model(attributes)?.get_attribute("label"),
        Some(&"direct".into())
    );
    let previews = batch.raw_on(connection).await?;
    assert_eq!(previews.len(), 2);
    assert!(
        previews
            .iter()
            .all(|row| row["label"] == "definition-configured".into())
    );

    let mut schema = connection.get_schema_builder();
    schema
        .create("factory_shortcuts", |table| {
            table.id();
            table.string("label", 100);
        })
        .await?;
    assert!(
        ShortcutFactory::times(0)
            .create_on(connection)
            .await?
            .is_empty()
    );
    assert_eq!(connection.table("factory_shortcuts").count("*").await?, 0);
    assert_eq!(batch.create_on(connection).await?.len(), 2);
    assert_eq!(
        connection
            .table("factory_shortcuts")
            .order_by("id", "asc")
            .pluck("label")
            .await?,
        [
            "definition-configured".into(),
            "definition-configured".into()
        ]
    );
    schema.drop("factory_shortcuts").await?;
    Ok(())
}

#[tokio::test]
async fn named_factory_shortcuts_preserve_configuration_and_model_construction() {
    let root = Root::new();
    verify_factory_shortcuts(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_factory_shortcuts(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

async fn verify_factory_raw_overrides(connection: &Connection) -> DatabaseResult<()> {
    let factory = Factory::<FactoryChild>::factory_for_model();
    assert_eq!(
        factory.raw_on(connection).await?["label"],
        "configured".into()
    );
    let overrides = Attributes::from([("label".into(), "temporary".into())]);
    assert_eq!(
        factory.raw_with_on(connection, overrides.clone()).await?["label"],
        "temporary".into()
    );
    let raw = factory
        .raw_with_state_on(connection, |attributes| {
            assert_eq!(attributes["label"], "configured".into());
            overrides.clone()
        })
        .await?;
    assert_eq!(raw["label"], "temporary".into());
    assert_eq!(
        factory.raw_on(connection).await?["label"],
        "configured".into()
    );

    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    let batch = Factory::<ExpansionChild>::new(Attributes::new)
        .r#for(
            BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id"),
            Factory::<ExpansionOwner>::new(Attributes::new),
        )
        .after_making(|_| panic!("raw invoked afterMaking"))
        .sequence(|index| Attributes::from([("position".into(), index.to_string().into())]))
        .count(2);
    let rows = batch
        .raw_with_state_on(connection, |attributes| {
            assert!(attributes.contains_key("position"));
            Attributes::from([("owner_id".into(), Value::Null)])
        })
        .await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["position"], "0".into());
    assert_eq!(rows[1]["position"], "1".into());
    assert!(rows.iter().all(|row| row["owner_id"] == Value::Null));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 0);

    let rows = batch.raw_with_on(connection, overrides).await?;
    assert_eq!(rows[0]["owner_id"], rows[1]["owner_id"]);
    assert!(rows.iter().all(|row| row["label"] == "temporary".into()));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);
    assert_eq!(batch.raw_on(connection).await?[0].get("label"), None);
    let empty = Factory::<ExpansionChild>::new(|| panic!("empty batch evaluated definition"))
        .count(0)
        .raw_with_state_on(connection, |_| panic!("empty batch evaluated state"))
        .await?;
    assert!(empty.is_empty());
    schema.drop("expansion_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_raw_overrides_are_temporary_and_resolve_parents_after_state() {
    let root = Root::new();
    verify_factory_raw_overrides(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_factory_raw_overrides(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

async fn verify_raw_parent_context(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    let owners = || Factory::<ExpansionOwner>::new(Attributes::new);
    let parent = owners().create_on(connection).await?;
    let key = parent.get_key().unwrap().clone();
    let children = || {
        Factory::<ExpansionChild>::new(Attributes::new).state_with_parent::<ExpansionOwner>(
            |_, parent| {
                Attributes::from([(
                    "context_key".into(),
                    parent
                        .and_then(Model::get_key)
                        .cloned()
                        .unwrap_or(Value::Null),
                )])
            },
        )
    };
    let only_context = children()
        .raw_with_parent_on(connection, &parent, |_, _| Attributes::new())
        .await?;
    assert_eq!(only_context["context_key"], key);
    assert!(!only_context.contains_key("owner_id"));

    let factory = children().r#for(
        BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id"),
        owners(),
    );
    let attributes = factory
        .raw_with_parent_on(connection, &parent, |attributes, model| {
            assert!(std::ptr::eq(model, &parent));
            assert_eq!(attributes["context_key"], key);
            Attributes::from([("owner_id".into(), Value::Null)])
        })
        .await?;
    assert_eq!(attributes["owner_id"], Value::Null);
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    let batch = factory.count(2);
    let mut calls = 0;
    let rows = batch
        .raw_with_parent_on(connection, &parent, |attributes, model| {
            assert!(std::ptr::eq(model, &parent));
            assert_eq!(attributes["context_key"], key);
            calls += 1;
            Attributes::from([("owner_id".into(), key.clone())])
        })
        .await?;
    assert_eq!(calls, 2);
    assert!(rows.iter().all(|row| row["owner_id"] == key));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);
    let without_parent = batch.raw_on(connection).await?;
    assert_eq!(without_parent[0]["context_key"], Value::Null);
    assert_ne!(without_parent[0]["owner_id"], key);
    assert_eq!(connection.table("expansion_owners").count("*").await?, 2);
    schema.drop("expansion_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_raw_parent_is_state_context_without_automatic_association() {
    let root = Root::new();
    verify_raw_parent_context(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_raw_parent_context(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

async fn verify_make_and_insert_parent(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_children", |table| {
            table.id();
            table.unsigned_big_integer("owner_id").nullable();
            table.unsigned_big_integer("context_key");
            table.string("label", 80);
        })
        .await?;
    let owners = || Factory::<ExpansionOwner>::new(Attributes::new);
    let parent = owners().create_on(connection).await?;
    let key = parent.get_key().unwrap().clone();
    let factory = || {
        Factory::<ExpansionChild>::new(Attributes::new)
            .state_with_parent::<ExpansionOwner>(|_, parent| {
                Attributes::from([(
                    "context_key".into(),
                    parent
                        .and_then(Model::get_key)
                        .cloned()
                        .unwrap_or(Value::Null),
                )])
            })
            .r#for(
                BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id"),
                owners(),
            )
            .after_making(|model| {
                assert!(!model.state().exists());
                model.set_attribute("label", "made");
                Ok(())
            })
            .after_creating(|_| panic!("insert ran afterCreating"))
    };
    let overrides = || Attributes::from([("owner_id".into(), Value::Null)]);
    let final_state = |attributes: &Attributes, parent: &ExpansionOwner| {
        assert_eq!(parent.get_key(), Some(&key));
        assert_eq!(attributes["context_key"], key);
        overrides()
    };
    let single = factory();
    let model = single
        .make_with_parent_state_on(connection, &parent, final_state)
        .await?;
    assert_eq!(model.get_attribute("context_key"), Some(&key));
    assert_eq!(model.get_attribute("owner_id"), Some(&Value::Null));
    assert_eq!(model.get_attribute("label"), Some(&"made".into()));
    let batch = factory().count(2);
    let drafts = batch
        .make_with_parent_state_on(connection, &parent, final_state)
        .await?;
    assert_eq!(drafts.len(), 2);
    assert!(
        drafts
            .iter()
            .all(|model| model.get_attribute("context_key") == Some(&key))
    );
    assert_eq!(connection.table("expansion_children").count("*").await?, 0);

    Factory::<ExpansionChild>::new(|| panic!("empty batch evaluated definition"))
        .count(0)
        .insert_with_parent_on(connection, &parent, overrides())
        .await?;
    assert_eq!(connection.table("expansion_children").count("*").await?, 0);
    single
        .insert_with_parent_on(connection, &parent, overrides())
        .await?;
    batch
        .insert_with_parent_on(connection, &parent, overrides())
        .await?;
    let rows = connection.table("expansion_children");
    assert_eq!(rows.count("*").await?, 3);
    let rows = ExpansionChild::query_on(connection).get().await?;
    assert!(rows.iter().all(|model| {
        matches!(
            model.get_attribute("context_key"),
            Some(Value::Integer(1) | Value::Unsigned(1))
        ) && model.get_attribute("owner_id") == Some(&Value::Null)
            && model.get_attribute("label") == Some(&"made".into())
    }));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);
    let without_parent = single.make_one_on(connection, overrides()).await?;
    assert_eq!(
        without_parent.get_attribute("context_key"),
        Some(&Value::Null)
    );
    schema.drop("expansion_children").await?;
    schema.drop("expansion_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_make_and_insert_share_parent_state_without_associating_it() {
    let root = Root::new();
    verify_make_and_insert_parent(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_make_and_insert_parent(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

async fn verify_attachment_pivot_count(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_children", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_links", |table| {
            table.unsigned_big_integer("owner_id");
            table.unsigned_big_integer("child_id");
            table.string("label", 40).nullable();
        })
        .await?;

    let relation = || {
        openrust::database::eloquent::relations::BelongsToMany::<ExpansionOwner, ExpansionChild>::new(
        "expansion_links", "owner_id", "child_id", "id", "id",
    )
    };
    let owners = || Factory::<ExpansionOwner>::new(Attributes::new);
    let children = || Factory::<ExpansionChild>::new(Attributes::new);
    let pivots = || {
        vec![
            Attributes::from([("label".into(), "first".into())]),
            Attributes::from([("label".into(), "second".into())]),
        ]
    };
    let inferred = owners()
        .has_attached(relation(), children(), pivots())
        .create_on(connection)
        .await?;
    let explicit = owners()
        .has_attached(relation(), children().count(3), pivots())
        .create_on(connection)
        .await?;
    let empty = owners()
        .has_attached(relation(), children().count(0), pivots())
        .create_on(connection)
        .await?;
    let default = owners()
        .has_attached(relation(), children(), Vec::<Attributes>::new())
        .create_on(connection)
        .await?;
    let shared = owners()
        .has_attached(
            relation(),
            children(),
            Attributes::from([("label".into(), "shared".into())]),
        )
        .create_on(connection)
        .await?;
    let labels = |owner: &ExpansionOwner| {
        connection
            .table("expansion_links")
            .r#where("owner_id", "=", owner.get_key().unwrap().clone())
            .order_by("child_id", "asc")
    };
    assert_eq!(
        labels(&inferred).pluck("label").await?,
        ["first".into(), "second".into()]
    );
    assert_eq!(
        labels(&explicit).pluck("label").await?,
        ["first".into(), "second".into(), "first".into()]
    );
    assert!(labels(&empty).pluck("label").await?.is_empty());
    assert_eq!(labels(&default).pluck("label").await?, [Value::Null]);
    assert_eq!(labels(&shared).pluck("label").await?, ["shared".into()]);
    assert_eq!(connection.table("expansion_children").count("*").await?, 7);
    schema.drop("expansion_links").await?;
    schema.drop("expansion_children").await?;
    schema.drop("expansion_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_attachment_infers_count_only_for_uncounted_factories() {
    let root = Root::new();
    verify_attachment_pivot_count(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_attachment_pivot_count(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

async fn verify_seeder_events(root: &Root, mysql: Option<&str>) {
    let connection = root.connection(mysql, "database/alternate.sqlite");
    let mut schema = connection.get_schema_builder();
    schema
        .create("seed_factory_children", |table| {
            table.id();
            table.string("generated_name", 80);
        })
        .await
        .unwrap();
    let run = |name| {
        root.fixture(
            "production",
            mysql,
            &[
                "db:seed",
                "--database=alternate",
                "--force",
                "--class",
                name,
            ],
        )
    };
    let completed = success(run("SeederEventFlow"));
    assert!(completed.contains("MutedSeeder DONE") && completed.contains("EventModelSeeder DONE"));
    let failed = run("FailedSeederEventFlow");
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("muted fixture failure"));
    let rows = connection
        .table("seed_factory_children")
        .pluck("generated_name")
        .await
        .unwrap();
    assert_eq!(
        rows,
        [
            "quiet-fixture".into(),
            "quiet-fixture".into(),
            "quiet-fixture".into()
        ]
    );
    schema.drop("seed_factory_children").await.unwrap();
}

#[tokio::test]
async fn seeder_event_muting_covers_children_and_restores_after_errors() {
    let root = Root::new();
    verify_seeder_events(&root, None).await;
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_seeder_events(&root, Some(&url)).await;
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

#[test]
fn faker_barcodes_match_checksums_and_preserve_unique_history() -> DatabaseResult<()> {
    let faker = fake(Some("zh-Hant"))?;
    for (length, generate) in [
        (
            8,
            openrust::database::eloquent::factories::Faker::ean8 as fn(_) -> _,
        ),
        (13, openrust::database::eloquent::factories::Faker::ean13),
        (13, openrust::database::eloquent::factories::Faker::isbn13),
    ] {
        for _ in 0..64 {
            let code: String = generate(&faker);
            assert_eq!(code.len(), length);
            assert!(code.bytes().all(|digit| digit.is_ascii_digit()));
            let sum: u32 = code
                .bytes()
                .rev()
                .enumerate()
                .map(|(index, digit)| u32::from(digit - b'0') * if index % 2 == 0 { 1 } else { 3 })
                .sum();
            assert_eq!(sum % 10, 0);
        }
    }
    for _ in 0..64 {
        let code = faker.isbn13();
        assert!(code.starts_with("978") || code.starts_with("979"));
        let code = faker.isbn10();
        assert_eq!(code.len(), 10);
        assert!(code[..9].bytes().all(|digit| digit.is_ascii_digit()));
        let sum: u32 = code
            .bytes()
            .zip((1..=10).rev())
            .map(|(digit, weight)| {
                weight
                    * if digit == b'X' {
                        10
                    } else {
                        u32::from(digit - b'0')
                    }
            })
            .sum();
        assert_eq!(sum % 11, 0);
    }
    let unique = faker.unique(true);
    for samples in [
        (0..128)
            .map(|_| unique.ean8())
            .collect::<DatabaseResult<Vec<_>>>()?,
        (0..128)
            .map(|_| unique.ean13())
            .collect::<DatabaseResult<Vec<_>>>()?,
        (0..128)
            .map(|_| unique.isbn10())
            .collect::<DatabaseResult<Vec<_>>>()?,
        (0..128)
            .map(|_| unique.isbn13())
            .collect::<DatabaseResult<Vec<_>>>()?,
    ] {
        assert_eq!(
            samples
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            samples.len()
        );
    }
    Ok(())
}

async fn verify_default_application_seeder(root: &Root, mysql: Option<&str>) {
    use openrust::{
        contracts::hashing::Hasher,
        hashing::{BcryptConfig, BcryptHasher, HashOptions},
    };

    let database = if mysql.is_some() { "mysql" } else { "sqlite" };
    let run = |args: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
        command
            .env("APP_BASE_PATH", &root.0)
            .env("APP_ENV", "local")
            .env("DB_CONNECTION", database)
            .env("HASH_DRIVER", "bcrypt")
            .env_remove("DB_URL")
            .env_remove("DB_DATABASE")
            .args(["artisan"])
            .args(args);
        if let Some(url) = mysql {
            command.env("DB_URL", &root.mysql(url).url);
        }
        command.output().unwrap()
    };

    success(run(&["migrate", "--force"]));
    success(run(&["db:seed", "--database", database, "--force"]));
    let connection = root.connection(mysql, "database/database.sqlite");
    let user = connection.table("users").first().await.unwrap().unwrap();
    assert_eq!(user.get("name"), Some(&"Test User".into()));
    assert_eq!(user.get("email"), Some(&"test@example.com".into()));
    assert!(
        user.get("email_verified_at")
            .is_some_and(|value| *value != Value::Null)
    );
    let Some(Value::Text(token)) = user.get("remember_token") else {
        panic!("missing remember token");
    };
    assert_eq!(token.len(), 10);
    assert!(token.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    let Some(Value::Text(password)) = user.get("password") else {
        panic!("missing password");
    };
    assert!(
        BcryptHasher::new(BcryptConfig::default())
            .check(b"password", password, &HashOptions::new())
            .await
            .unwrap()
    );
    let duplicate = run(&["db:seed", "--database", database, "--force"]);
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("DatabaseSeeder"));
    assert_eq!(connection.table("users").count("*").await.unwrap(), 1);
}

#[tokio::test]
async fn application_default_seeder_creates_the_verified_example_user_on_sqlite() {
    verify_default_application_seeder(&Root::new(), None).await;
}

#[tokio::test]
async fn application_default_seeder_creates_the_verified_example_user_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_DEFAULT_SEEDER_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    verify_default_application_seeder(&root, Some(&url)).await;
    root.close().await;
}

async fn verify_sequence_lifecycle(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_children", |table| {
            table.id();
            table.string("label", 40).nullable();
        })
        .await?;
    schema
        .create("expansion_links", |table| {
            table.unsigned_big_integer("owner_id");
            table.unsigned_big_integer("child_id");
            table.string("label", 40);
        })
        .await?;
    let label = |value: &str| Attributes::from([("label".into(), value.into())]);
    let factory = || {
        Factory::<ExpansionChild>::new(Attributes::new)
            .sequence(|index| Attributes::from([("label".into(), index.to_string().into())]))
    };
    let builder = factory();
    assert_eq!(builder.raw_on(connection).await?["label"], "0".into());
    let batch = builder.count(2);
    assert_eq!(
        batch.make_on(connection).await?[1].get_attribute("label"),
        Some(&"2".into())
    );
    batch.insert_on(connection, Attributes::new()).await?;
    assert_eq!(
        batch.create_on(connection).await?[0].get_attribute("label"),
        Some(&"5".into())
    );
    assert_eq!(factory().raw_on(connection).await?["label"], "0".into());
    let each = Factory::<ExpansionChild>::new(Attributes::new)
        .for_each_sequence(vec![label("first"), label("second")]);
    assert_eq!(
        each.make_one_on(connection, Attributes::new())
            .await?
            .get_attribute("label"),
        Some(&"first".into())
    );
    assert_eq!(each.raw_on(connection).await?[0]["label"], "second".into());
    let product = Factory::<ExpansionChild>::new(Attributes::new)
        .cross_join_sequence(vec![vec![label("x"), label("y")]])?;
    assert_eq!(product.raw_on(connection).await?["label"], "x".into());
    assert_eq!(product.raw_on(connection).await?["label"], "y".into());

    let parent = Factory::<ExpansionOwner>::new(Attributes::new)
        .create_on(connection)
        .await?;
    let contextual =
        Factory::<ExpansionChild>::new(|| Attributes::from([("label".into(), "base".into())]))
            .set("label", "earlier")
            .count(2)
            .sequence_with_state(|index, attributes| {
                assert_eq!(attributes["label"], "earlier".into());
                Attributes::from([("label".into(), format!("step-{index}").into())])
            })
            .sequence_with_parent::<ExpansionOwner>(|index, attributes, parent| {
                assert_eq!(attributes["label"], format!("step-{index}").into());
                let suffix = if parent.is_some_and(|model| model.state().exists()) {
                    "saved"
                } else {
                    "none"
                };
                Attributes::from([("label".into(), format!("step-{index}-{suffix}").into())])
            });
    let draft = contextual
        .make_one_on(connection, Attributes::new())
        .await?;
    assert_eq!(draft.get_attribute("label"), Some(&"step-0-none".into()));
    let created = contextual
        .create_with_parent_on(connection, &parent, Attributes::new())
        .await?;
    assert_eq!(created.len(), 2);
    assert_eq!(
        created[0].get_attribute("label"),
        Some(&"step-1-saved".into())
    );
    assert_eq!(
        created[1].get_attribute("label"),
        Some(&"step-2-saved".into())
    );
    let preview = contextual
        .make_one_on(connection, label("override"))
        .await?;
    assert_eq!(preview.get_attribute("label"), Some(&"override".into()));
    assert_eq!(
        contextual
            .make_one_on(connection, Attributes::new())
            .await?
            .get_attribute("label"),
        Some(&"step-4-none".into())
    );

    let mixed = Factory::<ExpansionChild>::new(move || label("earlier"))
        .count(9)
        .for_each_sequence_entries(vec![
            label("fixed").into(),
            FactorySequence::resolve::<ExpansionOwner>(|position, attributes, parent| {
                assert_eq!(position.count, 2);
                assert_eq!(attributes["label"], "earlier".into());
                let owner = if parent.is_some_and(|model| model.state().exists()) {
                    "saved"
                } else {
                    "none"
                };
                Attributes::from([(
                    "label".into(),
                    format!("mixed-{}-{owner}", position.index).into(),
                )])
            }),
        ])
        .sequence_entries(vec![]);
    assert_eq!(
        mixed
            .make_one_on(connection, Attributes::new())
            .await?
            .get_attribute("label"),
        Some(&"fixed".into())
    );
    let models = mixed
        .create_with_parent_on(connection, &parent, Attributes::new())
        .await?;
    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0].get_attribute("label"),
        Some(&"mixed-1-saved".into())
    );
    assert_eq!(models[1].get_attribute("label"), Some(&"fixed".into()));
    let rows = mixed.raw_on(connection).await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["label"], "mixed-3-none".into());
    assert_eq!(rows[1]["label"], "fixed".into());

    let empty = Factory::<ExpansionChild>::new(|| panic!("zero count evaluated the definition"))
        .count(3)
        .for_each_sequence_entries(vec![]);
    assert!(empty.raw_on(connection).await?.is_empty());

    let relation = openrust::database::eloquent::relations::BelongsToMany::<
        ExpansionOwner,
        ExpansionChild,
    >::new("expansion_links", "owner_id", "child_id", "id", "id");
    let parents = Factory::<ExpansionOwner>::new(Attributes::new).has_attached(
        relation,
        Factory::<ExpansionChild>::new(Attributes::new).count(1),
        vec![label("first"), label("second")],
    );
    parents.create_on(connection).await?;
    parents.create_on(connection).await?;
    assert_eq!(
        connection
            .table("expansion_links")
            .order_by("owner_id", "asc")
            .pluck("label")
            .await?,
        ["first".into(), "second".into()]
    );
    assert_eq!(
        connection
            .table("expansion_children")
            .order_by("id", "asc")
            .pluck("label")
            .await?,
        [
            "3".into(),
            "4".into(),
            "5".into(),
            "6".into(),
            "step-1-saved".into(),
            "step-2-saved".into(),
            "mixed-1-saved".into(),
            "fixed".into(),
            Value::Null,
            Value::Null
        ]
    );
    schema.drop("expansion_links").await?;
    schema.drop("expansion_children").await?;
    schema.drop("expansion_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_sequences_continue_across_operations_and_attachments() {
    let root = Root::new();
    verify_sequence_lifecycle(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_sequence_lifecycle(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

async fn verify_factory_many_count(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("expansion_children", |table| {
            table.id();
        })
        .await?;
    let factory = || Factory::<ExpansionChild>::new(Attributes::new).count(3);
    let batch = factory();
    assert_eq!(batch.raw_on(connection).await?.len(), 3);
    assert_eq!(
        batch
            .make_many_on(connection, FactoryRecords::Default)
            .await?
            .len(),
        3
    );
    assert_eq!(
        batch
            .make_many_on(connection, FactoryRecords::Default)
            .await?
            .len(),
        1
    );
    assert_eq!(batch.make_on(connection).await?.len(), 1);
    assert_eq!(batch.raw_on(connection).await?.len(), 1);
    assert_eq!(connection.table("expansion_children").count("*").await?, 0);

    let explicit = factory();
    assert!(
        explicit
            .make_many_on(connection, FactoryRecords::Count(0))
            .await?
            .is_empty()
    );
    assert_eq!(explicit.raw_on(connection).await?.len(), 1);

    let create = factory();
    assert_eq!(
        create
            .create_many_quietly_on(connection, FactoryRecords::Default)
            .await?
            .len(),
        3
    );
    assert_eq!(create.create_on(connection).await?.len(), 1);
    let failed = Factory::<ExpansionChild>::new(Attributes::new)
        .after_making(|_| Err(DatabaseError::InvalidQuery("factory count fixture error")))
        .count(3);
    assert!(
        failed
            .make_many_on(connection, FactoryRecords::Default)
            .await
            .is_err()
    );
    assert_eq!(failed.raw_on(connection).await?.len(), 1);
    assert_eq!(connection.table("expansion_children").count("*").await?, 4);
    schema.drop("expansion_children").await?;
    Ok(())
}

#[tokio::test]
async fn factory_many_operations_consume_the_configured_count() {
    let root = Root::new();
    verify_factory_many_count(&root.connection(None, "database/database.sqlite"))
        .await
        .unwrap();
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        verify_factory_many_count(&root.connection(Some(&url), ""))
            .await
            .unwrap();
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
    root.close().await;
}

#[test]
fn factory_parent_creation_runs_through_artisan_on_sqlite_and_mysql() {
    let root = Root::new();
    let args = [
        "db:seed",
        "--class=ParentCreationSeeder",
        "--database=alternate",
        "--force",
    ];
    success(root.fixture("local", None, &args));
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") {
        success(root.fixture("local", Some(&url), &args));
    } else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
    }
}

struct InsertProjection(ModelState);

impl Model for InsertProjection {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("factory_insert_projection")
            .timestamps(None, None)
            .hidden(&["secret"])
            .appends(&["computed"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        Self(state)
    }

    fn state(&self) -> &ModelState {
        &self.0
    }

    fn state_mut(&mut self) -> &mut ModelState {
        &mut self.0
    }

    fn attribute(name: &str) -> Option<Attribute> {
        match name {
            "label" => Some(Attribute::make(
                Some(|value, _| match value {
                    Some(Value::Text(value)) => Ok(format!("read-{value}").into()),
                    _ => Err(DatabaseError::InvalidQuery("projection label")),
                }),
                Some(|value, _| match value {
                    Value::Text(value) => Ok(Attributes::from([(
                        "label".into(),
                        format!("write-{value}").into(),
                    )])),
                    _ => Err(DatabaseError::InvalidQuery("projection input")),
                }),
            )),
            "computed" => Some(Attribute::get(|_, _| panic!("insert evaluated an append"))),
            _ => None,
        }
    }

    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[
            ("options", AttributeCast::Json),
            ("published_at", AttributeCast::DateTime),
        ]
    }
}

async fn verify_factory_insert_projection(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("factory_insert_projection", |table| {
            table.id();
            table.string("label", 80).unique();
            table.string("secret", 80);
            table.json("options");
            table.timestamp("published_at", 0);
        })
        .await?;
    let factory = || {
        Factory::<InsertProjection>::new(|| {
            Attributes::from([
                ("label".into(), "sample".into()),
                ("secret".into(), "retained".into()),
                ("options".into(), Value::Json(json!({"ok":true}))),
                ("published_at".into(), "2020-01-02T03:04:05Z".into()),
            ])
        })
    };
    let batch = factory().count(2);
    assert!(
        batch
            .insert_on(connection, Attributes::new())
            .await
            .is_err()
    );
    assert_eq!(
        connection
            .table("factory_insert_projection")
            .count("*")
            .await?,
        0
    );
    factory().insert_on(connection, Attributes::new()).await?;
    let row = connection
        .table("factory_insert_projection")
        .first()
        .await?
        .unwrap();
    assert_eq!(row.get("label"), Some(&"write-read-write-sample".into()));
    assert_eq!(row.get("secret"), Some(&"retained".into()));
    let model = InsertProjection::query_on(connection)
        .get()
        .await?
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        model.get_attribute_value("options")?,
        Some(Value::Json(json!({"ok":true})))
    );
    assert!(
        matches!(model.get_attribute_value("published_at")?, Some(Value::Text(date)) if date.starts_with("2020-01-02T03:04:05"))
    );
    schema.drop("factory_insert_projection").await?;
    Ok(())
}

#[tokio::test]
async fn factory_insert_projects_attributes_before_writing_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    verify_factory_insert_projection(&root.connection(Some(&url), ""))
        .await
        .unwrap();
    root.close().await;
}

fn assert_test_iban(iban: &str) {
    let rotated = format!("{}{}", &iban[4..], &iban[..4]);
    let decimal: String = rotated
        .bytes()
        .map(|byte| {
            if byte.is_ascii_digit() {
                char::from(byte).to_string()
            } else {
                (byte - b'A' + 10).to_string()
            }
        })
        .collect();
    let remainder = decimal.bytes().fold(0_u32, |value, digit| {
        (10 * value + u32::from(digit - b'0')) % 97
    });
    assert_eq!(remainder, 1);
}

#[test]
fn faker_bank_identifiers_follow_fixed_layouts_and_checksums() -> DatabaseResult<()> {
    let faker = openrust::database::eloquent::factories::fake(Some("en"))?;
    let de = faker.iban(Some("de"), "0012", Some(1))?;
    assert!(de.starts_with("DE"));
    assert_eq!(de.len(), 22);
    assert_eq!(&de[4..8], "0012");
    assert!(de[4..].bytes().all(|byte| byte.is_ascii_digit()));
    assert_test_iban(&de);

    let gb = faker.iban(Some("GB"), "BANK", None)?;
    assert_eq!(gb.len(), 22);
    assert_eq!(&gb[4..8], "BANK");
    assert!(gb[8..].bytes().all(|byte| byte.is_ascii_digit()));
    assert_test_iban(&gb);
    let fr = faker.iban(Some("FR"), "", None)?;
    assert_eq!(fr.len(), 27);
    assert!(fr[4..14].bytes().all(|byte| byte.is_ascii_digit()));
    assert!(fr[14..25].bytes().all(|byte| byte.is_ascii_alphanumeric()));
    assert!(fr[25..].bytes().all(|byte| byte.is_ascii_digit()));
    assert_test_iban(&fr);

    let fallback = faker.iban(Some("ZZ"), "", None)?;
    assert_eq!(fallback.len(), 28);
    assert!(fallback[4..].bytes().all(|byte| byte.is_ascii_digit()));
    assert_test_iban(&fallback);
    let short = faker.iban(Some("ZZ"), "003", Some(5))?;
    assert_eq!(short.len(), 9);
    assert_eq!(&short[4..7], "003");
    assert_test_iban(&short);
    assert!(faker.iban(Some("USA"), "", None).is_err());
    assert!(faker.iban(Some("GB"), "bad-", None).is_err());
    assert!(faker.iban(Some("ZZ"), "", Some(31)).is_err());
    assert!(faker.iban(Some("GB"), &"A".repeat(31), None).is_err());

    let unique = faker.unique(true);
    let mut ibans = std::collections::HashSet::new();
    let mut bics = std::collections::HashSet::new();
    for _ in 0..128 {
        let iban = unique.iban(None, "", None)?;
        assert_test_iban(&iban);
        assert!(ibans.insert(iban));
        let bic = unique.swift_bic_number()?;
        assert!(matches!(bic.len(), 8 | 11));
        assert!(bic[..6].bytes().all(|byte| byte.is_ascii_uppercase()));
        assert!(bic[6..].bytes().all(|byte| byte.is_ascii_alphanumeric()));
        assert!(bics.insert(bic));
    }
    Ok(())
}

#[test]
fn faker_credit_cards_follow_provider_masks_and_expiration_contract() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::fake;

    let faker = fake(Some("en"))?;
    let brazil = fake(Some("pt-BR"))?;
    for (generator, vendor, lengths, prefixes) in [
        (&faker, "Visa", &[16][..], &["4"][..]),
        (&faker, "Visa Retired", &[13][..], &["4"][..]),
        (&faker, "MasterCard", &[16][..], &["2", "5"][..]),
        (&faker, "American Express", &[15][..], &["34", "37"][..]),
        (&faker, "Discover Card", &[16][..], &["6011"][..]),
        (&faker, "JCB", &[16][..], &["3528", "3589"][..]),
        (&brazil, "Diners", &[14, 16][..], &["30", "36", "38"][..]),
        (&brazil, "Elo", &[16][..], &["4", "5", "6"][..]),
        (&brazil, "Hipercard", &[16][..], &["38", "60"][..]),
        (&brazil, "Aura", &[16][..], &["50"][..]),
    ] {
        for _ in 0..16 {
            let formatted = generator.credit_card_number(Some(vendor), true, Some("::"))?;
            let groups: Vec<_> = formatted.split("::").collect();
            assert_eq!(groups.len(), 4);
            assert!(groups[..3].iter().all(|group| group.len() == 4));
            let number = groups.concat();
            assert!(lengths.contains(&number.len()));
            assert!(prefixes.iter().any(|prefix| number.starts_with(prefix)));
            assert!(number.bytes().all(|byte| byte.is_ascii_digit()));
            let sum: u32 = number
                .bytes()
                .rev()
                .enumerate()
                .map(|(index, byte)| {
                    let value = u32::from(byte - b'0');
                    if index % 2 == 0 {
                        value
                    } else {
                        value * 2 - u32::from(value > 4) * 9
                    }
                })
                .sum();
            assert_eq!(sum % 10, 0);
        }
    }
    assert!(faker.credit_card_number(Some("Elo"), false, None).is_err());
    assert!(brazil.credit_card_number(Some("JCB"), false, None).is_err());
    assert!(
        faker
            .credit_card_number(Some("bogus"), false, None)
            .is_err()
    );
    let start = jiff::Timestamp::now().as_second();
    let upper = faker.resolve_date_max("+36 months")?.as_second() + 1;
    let lower = faker.resolve_date_max("-36 months")?.as_second() - 1;
    for _ in 0..16 {
        assert!(
            (start..=upper).contains(
                &faker
                    .credit_card_expiration_date(true)?
                    .timestamp()
                    .as_second()
            )
        );
        assert!(
            (lower..=upper).contains(
                &faker
                    .credit_card_expiration_date(false)?
                    .timestamp()
                    .as_second()
            )
        );
    }
    let default_date = faker.credit_card_expiration_date_string(true, None)?;
    assert!(jiff::civil::Date::strptime("%d/%m/%y", format!("01/{default_date}")).is_ok());
    assert!(
        faker
            .credit_card_expiration_date_string(true, Some("%"))
            .is_err()
    );
    let unique = faker.unique(true);
    let mut vendors = std::collections::HashSet::new();
    for _ in 0..6 {
        assert!(vendors.insert(unique.credit_card_type()?));
    }
    assert!(unique.credit_card_type().is_err());
    let details = unique.credit_card_details(true)?;
    assert!(vendors.contains(details.r#type));
    assert!(!details.name.is_empty());
    assert!(!details.number.contains('-'));
    assert_eq!(
        serde_json::to_value(&details).unwrap()["expirationDate"],
        details.expiration_date
    );
    assert!(
        unique
            .credit_card_expiration_date(true)?
            .timestamp()
            .as_second()
            >= start
    );
    assert_eq!(
        unique
            .credit_card_expiration_date_string(true, Some("%Y-%m"))?
            .len(),
        7
    );
    let first = unique.credit_card_number(None, false, None)?;
    assert_ne!(unique.credit_card_number(None, false, None)?, first);
    Ok(())
}

#[test]
fn faker_regional_bank_accounts_and_routing_follow_provider_contract() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::{Faker, fake};

    for (locale, country) in [
        ("ar-SA", "SA"),
        ("bg-BG", "BG"),
        ("cs-CZ", "CZ"),
        ("da-DK", "DK"),
        ("at-AT", "AT"),
        ("de-AT", "AT"),
        ("de-CH", "CH"),
        ("de-DE", "DE"),
        ("el-CY", "CY"),
        ("el-GR", "GR"),
        ("en-GB", "GB"),
        ("es-ES", "ES"),
        ("fi-FI", "FI"),
        ("fr-BE", "BE"),
        ("fr-CH", "CH"),
        ("fr-FR", "FR"),
        ("he-IL", "IL"),
        ("hr-HR", "HR"),
        ("hu-HU", "HU"),
        ("is-IS", "IS"),
        ("it-CH", "CH"),
        ("it-IT", "IT"),
        ("ka-GE", "GE"),
        ("kk-KZ", "KZ"),
        ("lt-LT", "LT"),
        ("lv-LV", "LV"),
        ("me-ME", "ME"),
        ("nb-NO", "NO"),
        ("nl-BE", "BE"),
        ("nl-NL", "NL"),
        ("pl-PL", "PL"),
        ("pt-BR", "BR"),
        ("pt-PT", "PT"),
        ("ro-MD", "MD"),
        ("ro-RO", "RO"),
        ("sk-SK", "SK"),
        ("sl-SI", "SI"),
        ("sr-Cyrl-RS", "RS"),
        ("sr-Latn-RS", "RS"),
        ("sr-RS", "RS"),
        ("sv-SE", "SE"),
        ("tr-TR", "TR"),
        ("ar-EG", "EG"),
    ] {
        let account = fake(Some(locale))?.bank_account_number("", None, None)?;
        assert!(account.starts_with(country), "{locale}: {account}");
        assert_test_iban(&account);
    }
    let uk = fake(Some("en-GB"))?;
    let gb = uk.bank_account_number("BANK", None, None)?;
    assert_eq!(&gb[4..8], "BANK");
    assert_eq!(gb.len(), 22);
    let override_account = uk.bank_account_number("00", Some("ZZ"), Some(7))?;
    assert_eq!(override_account.len(), 11);
    assert_eq!(&override_account[4..6], "00");
    assert_eq!(
        fake(Some("me-ME"))?
            .bank_account_number("", Some("ZZ"), None)?
            .len(),
        22
    );

    let usa = fake(Some("en-US"))?;
    let malaysia = fake(Some("ms-MY"))?;
    let nepal = fake(Some("ne-NP"))?;
    let unique = usa.unique(true);
    let mut routes = std::collections::HashSet::new();
    for _ in 0..128 {
        let account = usa.bank_account_number("", None, None)?;
        assert!((5..=17).contains(&account.len()));
        assert!(account.bytes().all(|byte| byte.is_ascii_digit()));
        let account = malaysia.bank_account_number("", None, None)?;
        assert!((10..=16).contains(&account.len()));
        assert!(account.bytes().all(|byte| byte.is_ascii_digit()));
        let account = nepal.bank_account_number("", None, None)?;
        let digits = if let Some(digits) = account.strip_prefix("00") {
            digits
        } else {
            assert!(account.as_bytes()[0].is_ascii_uppercase());
            &account[1..]
        };
        assert!((9..=20).contains(&account.len()));
        assert!(digits.bytes().all(|byte| (b'1'..=b'9').contains(&byte)));
        let route = unique.bank_routing_number()?;
        assert_eq!(route.len(), 9);
        let district: u8 = route[..2].parse().unwrap();
        assert!(matches!(district, 1..=12 | 21..=32 | 61..=72));
        assert_ne!(route.as_bytes()[2], b'0');
        assert_ne!(route.as_bytes()[4], b'0');
        let checksum: u32 = route
            .bytes()
            .zip([3, 7, 1].into_iter().cycle())
            .map(|(digit, weight)| u32::from(digit - b'0') * weight)
            .sum();
        assert_eq!(checksum % 10, 0);
        assert!(routes.insert(route));
    }
    assert_eq!(Faker::calculate_routing_number_checksum("02100002")?, 1);
    assert_eq!(Faker::calculate_routing_number_checksum("021000021")?, 1);
    assert!(Faker::calculate_routing_number_checksum("0210000x").is_err());
    assert!(Faker::calculate_routing_number_checksum("123").is_err());
    assert!(usa.bank_account_number("00", None, None).is_err());
    for locale in ["en-CA", "ja-JP", "zz-ZZ"] {
        let fallback = fake(Some(locale))?;
        let account = fallback.bank_account_number("", None, None)?;
        assert!((5..=17).contains(&account.len()));
        assert!(account.bytes().all(|byte| byte.is_ascii_digit()));
        let route = fallback.unique(true).bank_routing_number()?;
        let check = Faker::calculate_routing_number_checksum(&route)?;
        assert_eq!(route.len(), 9);
        assert_eq!(route.as_bytes()[8] - b'0', check);
        assert!(fallback.bank_account_number("00", None, None).is_err());
    }
    let alias = fake(Some("at-AT"))?;
    assert!(alias.vat(Some(false))?.starts_with("ATU"));
    assert!(alias.bank_routing_number().is_err());
    assert!(uk.bank_routing_number().is_err());
    assert!(
        fake(Some("zh-Hant"))?
            .bank_account_number("", None, None)
            .is_err()
    );
    assert_ne!(
        unique.bank_account_number("", None, None)?,
        unique.bank_account_number("", None, None)?
    );
    Ok(())
}

#[test]
fn faker_institutions_and_local_swift_follow_provider_contract() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::fake;

    for (locale, distinct, representative) in [
        ("de-DE", 60, "Bank 1 Saar"),
        ("el-CY", 12, "Τράπεζα Κύπρου"),
        ("ka-GE", 19, "ბანკი რესპუბლიკა"),
        ("kk-KZ", 2, "Қазкоммерцбанк"),
        ("pl-PL", 68, "Narodowy Bank Polski"),
        (
            "pt-BR",
            59,
            "BADESUL Desenvolvimento S.A. – Agência de Fomento/RS",
        ),
        ("ru-RU", 788, "Новый Промышленный Банк"),
        ("th-TH", 22, "ธนาคารแห่งประเทศไทย"),
        ("uk-UA", 89, "А-Банк"),
        ("zh-Hans", 19, "渤海银行"),
    ] {
        let faker = fake(Some(locale))?;
        let unique = faker.unique(true);
        let names = (0..distinct)
            .map(|_| unique.bank())
            .collect::<DatabaseResult<std::collections::HashSet<_>>>()?;
        assert_eq!(names.len(), distinct);
        assert!(names.contains(representative));
        assert!(unique.bank().is_err());
    }
    let malaysia = fake(Some("ms-MY"))?;
    let unique = malaysia.unique(true);
    assert_eq!(unique.currency_symbol()?, "RM");
    assert!(unique.currency_symbol().is_err());
    assert!(!unique.insurance()?.is_empty());
    let nepali = fake(Some("ne-NP"))?;
    for _ in 0..64 {
        assert!(!malaysia.bank()?.is_empty());
        assert!(!malaysia.local_bank()?.is_empty());
        assert!(!malaysia.foreign_bank()?.is_empty());
        assert!(!malaysia.government_bank()?.is_empty());
        let swift = malaysia.swift_code()?;
        assert_eq!(swift.len(), 11);
        assert_eq!(&swift[4..6], "MY");
        let swift = nepali.swift_code()?;
        assert!(matches!(swift.len(), 8 | 11));
        assert_eq!(&swift[4..6], "NP");
        assert!(
            nepali
                .micro_finance()?
                .ends_with(" Laghubitta Bittiya Sanstha")
        );
        assert!(!nepali.commercial_bank()?.is_empty());
        assert!(!nepali.development_bank()?.is_empty());
        assert!(!nepali.finance_company()?.is_empty());
        assert!(!nepali.digital_wallet()?.is_empty());
    }
    assert_ne!(unique.swift_code()?, unique.swift_code()?);
    assert!(malaysia.commercial_bank().is_err());
    assert!(nepali.local_bank().is_err());
    let traditional = fake(Some("zh-Hant"))?;
    assert!(traditional.bank().is_err());
    assert!(traditional.swift_code().is_err());
    Ok(())
}

#[test]
fn faker_vat_samples_match_regional_provider_contracts() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::{Faker, fake};

    let french = fake(Some("fr-FR"))?;
    let belgian = fake(Some("nl-BE"))?;
    let uk = fake(Some("en-GB"))?;
    let italian = fake(Some("it-IT"))?;
    for locale in [
        "bg-BG", "de-AT", "fr-BE", "nl-BE", "fr-FR", "es-ES", "it-IT", "nl-NL", "en-GB",
    ] {
        let faker = fake(Some(locale))?;
        for spaced in [None, Some(false)] {
            for _ in 0..32 {
                let vat = faker.vat(spaced)?;
                let groups: Vec<_> = vat.split(' ').map(str::len).collect();
                let compact = vat.replace(' ', "");
                let spaced = spaced.unwrap_or(true);
                let number = match locale {
                    "bg-BG" => {
                        assert!(compact.starts_with("BG"));
                        assert!(matches!(compact.len(), 11 | 12));
                        assert_ne!(compact.as_bytes()[7], b'0');
                        assert_eq!(groups.len(), if spaced { 2 } else { 1 });
                        &compact[2..]
                    }
                    "de-AT" => {
                        assert!(vat.starts_with(if spaced { "AT U" } else { "ATU" }));
                        assert_eq!(compact.len(), 11);
                        &compact[3..]
                    }
                    "fr-BE" | "nl-BE" => {
                        assert!(vat.starts_with(if spaced { "BE 0" } else { "BE0" }));
                        assert_eq!(compact.len(), 12);
                        &compact[3..]
                    }
                    "fr-FR" => {
                        assert!(compact.starts_with("FR"));
                        assert_eq!(compact.len(), 13);
                        assert_eq!(
                            groups,
                            if spaced {
                                vec![2, 2, 3, 3, 3]
                            } else {
                                vec![13]
                            }
                        );
                        &compact[2..]
                    }
                    "es-ES" => {
                        assert_eq!(vat.len(), 9);
                        assert!(b"ABCDEFGHJNPQRSUVW".contains(&vat.as_bytes()[0]));
                        &vat[1..]
                    }
                    "it-IT" => {
                        assert!(vat.starts_with("IT"));
                        assert_eq!(vat.len(), 13);
                        &vat[2..]
                    }
                    "nl-NL" => {
                        assert!(vat.starts_with("NL"));
                        assert_eq!(vat.len(), 14);
                        assert_eq!(&vat[11..12], "B");
                        assert!(vat[12..].bytes().all(|byte| byte.is_ascii_digit()));
                        &vat[2..11]
                    }
                    "en-GB" => {
                        assert!(compact.starts_with("GB"));
                        assert!(matches!(groups.as_slice(), [5, 4, 1 | 2]));
                        &compact[2..]
                    }
                    _ => unreachable!(),
                };
                assert!(
                    number.bytes().all(|byte| byte.is_ascii_digit()),
                    "{locale}: {vat}"
                );
            }
        }
    }
    let verify_luhn = |digits: &str| {
        let sum: u32 = digits
            .bytes()
            .rev()
            .enumerate()
            .map(|(index, byte)| {
                let digit = u32::from(byte - b'0');
                if index % 2 == 0 {
                    digit
                } else {
                    digit * 2 - u32::from(digit > 4) * 9
                }
            })
            .sum();
        assert_eq!(sum % 10, 0);
    };
    for _ in 0..64 {
        let fr = french.vat(Some(false))?;
        verify_luhn(&fr[4..]);
        let siren: u32 = fr[4..].parse().unwrap();
        assert_eq!(
            fr[2..4].parse::<u32>().unwrap(),
            (12 + 3 * (siren % 97)) % 97
        );
        assert_eq!(french.siren(true)?.split(' ').count(), 3);
        verify_luhn(&italian.unique(false).vat_id()?[2..]);
        let be = belgian.vat(Some(false))?;
        assert_eq!(
            be[10..].parse::<u32>().unwrap(),
            97 - be[2..10].parse::<u32>().unwrap() % 97
        );
        let branch = uk.vat_with_type(Some("branch"))?;
        let blocks: Vec<_> = branch[2..].split(' ').collect();
        assert_eq!(blocks.len(), 4);
        let body = [blocks[0], blocks[1]].concat();
        let weighted: u16 = body
            .bytes()
            .zip((2..=8).rev())
            .map(|(digit, weight)| u16::from(digit - b'0') * weight)
            .sum();
        assert_eq!((weighted + 55 + blocks[2].parse::<u16>().unwrap()) % 97, 0);
        assert_eq!(blocks[3].len(), 3);
        let government = uk.vat_with_type(Some("gov"))?;
        assert!(government.starts_with("GBGD"));
        assert_eq!(government.len(), 7);
        assert!(government[4..].parse::<u16>().unwrap() < 500);
        let health = uk.vat_with_type(Some("health"))?;
        assert!(health.starts_with("GBHA"));
        assert!((500..=999).contains(&health[4..].parse::<u16>().unwrap()));
    }
    assert_eq!(Faker::calculate_modulus97("0000000", false)?, 0);
    assert_eq!(Faker::calculate_modulus97("0000000", true)?, 42);
    assert!(Faker::calculate_modulus97("123456x", true).is_err());
    assert!(Faker::calculate_modulus97("123", true).is_err());
    assert!(
        uk.unique(true)
            .vat_with_type(Some("branch"))?
            .starts_with("GB")
    );
    assert!(fake(Some("nl-NL"))?.unique(true).btw()?.starts_with("NL"));
    let taiwanese = fake(Some("zh-Hant"))?.vat(None)?;
    assert_eq!(taiwanese.len(), 8);
    assert!(taiwanese.bytes().all(|digit| digit.is_ascii_digit()));
    assert!(fake(Some("en-US"))?.vat(None).is_err());
    assert!(french.vat_with_type(Some("gov")).is_err());
    Ok(())
}
async fn verify_deferred_factory_attributes(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.get_schema_builder();
    schema
        .create("seed_factory_owners", |table| {
            table.id();
            table.string("generated_name", 120);
        })
        .await?;
    schema
        .create("seed_factory_children", |table| {
            table.id();
            table.string("generated_name", 120);
            table.string("derived_name", 120);
            table.unsigned_big_integer("owner_id").nullable();
        })
        .await?;

    let factory = Factory::<FactoryChild>::new(Attributes::new)
        .set("generated_name", "default")
        .attribute("derived_name", |values| {
            let Some(Value::Text(name)) = values.get("generated_name") else {
                return Err(DatabaseError::InvalidQuery("missing factory name"));
            };
            Ok(format!("{name}-derived").into())
        })
        .parent_attribute_with("owner_id", |values| {
            let name = values["derived_name"].clone();
            Factory::<FactoryOwner>::new(move || {
                Attributes::from([("generated_name".into(), name.clone())])
            })
        });
    let preview = factory
        .raw_with_on(
            connection,
            Attributes::from([
                ("generated_name".into(), "preview".into()),
                ("owner_id".into(), Value::Null),
            ]),
        )
        .await?;
    assert_eq!(preview["derived_name"], "preview-derived".into());
    assert_eq!(connection.table("seed_factory_owners").count("*").await?, 0);
    let made = factory
        .create_one_on(
            connection,
            Attributes::from([("generated_name".into(), "saved".into())]),
        )
        .await?;
    assert_eq!(
        made.get_attribute("derived_name"),
        Some(&"saved-derived".into())
    );
    assert_eq!(
        connection
            .table("seed_factory_owners")
            .pluck("generated_name")
            .await?,
        ["saved-derived".into()]
    );

    let batch = factory.count(2);
    let made = batch
        .create_with_parent_on(
            connection,
            &FactoryOwner::from_state(Default::default()),
            Attributes::from([
                ("derived_name".into(), "override".into()),
                ("owner_id".into(), Value::Null),
            ]),
        )
        .await?;
    assert!(
        made.iter()
            .all(|model| model.get_attribute("derived_name") == Some(&"override".into()))
    );
    assert_eq!(
        connection.table("seed_factory_children").count("*").await?,
        3
    );
    assert_eq!(connection.table("seed_factory_owners").count("*").await?, 1);

    let disabled = Factory::<FactoryChild>::new(Attributes::new)
        .attribute("computed", |_| Ok("kept".into()))
        .parent_attribute_with::<FactoryOwner>("owner_id", |_| {
            panic!("excluded owner factory was called")
        })
        .without_parents(Vec::new());
    let row = disabled.raw_on(connection).await?;
    assert_eq!(row["computed"], "kept".into());
    assert_eq!(row["owner_id"], Value::Null);

    let failed = Factory::<FactoryChild>::new(Attributes::new)
        .attribute("computed", |_| {
            Err(DatabaseError::InvalidQuery("probe failed"))
        })
        .parent_attribute_with::<FactoryOwner>("owner_id", |_| panic!("later resolver was called"));
    assert!(matches!(
        failed.create_on(connection).await,
        Err(DatabaseError::InvalidQuery("probe failed"))
    ));
    assert_eq!(
        connection.table("seed_factory_children").count("*").await?,
        3
    );
    assert_eq!(connection.table("seed_factory_owners").count("*").await?, 1);
    schema.drop("seed_factory_children").await?;
    schema.drop("seed_factory_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_deferred_attributes_resolve_in_order_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = root.connection(Some(&url), "");
    verify_deferred_factory_attributes(&db).await.unwrap();
    root.close().await;
}

#[tokio::test]
async fn factory_model_keys_and_relation_cache_follow_lifecycle_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = root.connection(Some(&url), "");
    verify_factory_relation_cache(&db).await.unwrap();
    root.close().await;
}
async fn verify_factory_relation_cache(connection: &Connection) -> DatabaseResult<()> {
    use openrust::database::eloquent::{
        RelatedCollection,
        relations::{BelongsTo, HasMany},
    };

    let mut schema = connection.get_schema_builder();
    schema
        .create("seed_factory_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("seed_factory_children", |table| {
            table.id();
            table.unsigned_big_integer("owner_id").nullable();
        })
        .await?;
    let existing = Factory::<FactoryOwner>::new(Attributes::new)
        .create_on(connection)
        .await?;
    let child = Factory::<FactoryChild>::new(Attributes::new)
        .model_attribute("owner_id", &existing)
        .make_on(connection)
        .await?;
    assert_eq!(child.get_attribute("owner_id"), existing.get_key());
    let unsaved = FactoryOwner::from_state(Default::default());
    let raw = Factory::<FactoryChild>::new(Attributes::new)
        .model_attribute("owner_id", &unsaved)
        .raw_on(connection)
        .await?;
    assert_eq!(raw["owner_id"], Value::Null);
    assert_eq!(connection.table("seed_factory_owners").count("*").await?, 1);
    assert_eq!(
        connection.table("seed_factory_children").count("*").await?,
        0
    );

    type Children = HasMany<FactoryOwner, FactoryChild>;
    type Single = BelongsTo<FactoryOwner, FactoryOwner>;
    let known_child = std::sync::Arc::new(child);
    let factory = Factory::<FactoryOwner>::new(Attributes::new)
        .after_making(move |owner| {
            owner.set_relation::<Children>("children", RelatedCollection::from_shared(Vec::new()));
            owner.set_relation::<Children>(
                "known",
                RelatedCollection::from_shared(vec![known_child.clone()]),
            );
            owner.set_relation::<Single>("single", None);
            Ok(())
        })
        .has(
            Children::new("id", "owner_id"),
            Factory::<FactoryChild>::new(Attributes::new)
                .after_making(|child| {
                    type Nested = HasMany<FactoryChild, FactoryChild>;
                    child.set_relation::<Nested>(
                        "nested",
                        RelatedCollection::from_shared(Vec::new()),
                    );
                    Ok(())
                })
                .after_creating(|child| {
                    assert!(!child.relation_loaded("nested"));
                    Ok(())
                })
                .count(2),
        )
        .after_creating(|owner| {
            assert!(!owner.relation_loaded("children"));
            assert!(owner.relation_loaded("known"));
            assert!(owner.relation_loaded("single"));
            Ok(())
        });
    let preview = factory.make_on(connection).await?;
    assert!(preview.relation_loaded("children"));
    let mut owner = factory.create_on(connection).await?;
    let relation = Children::new("id", "owner_id");
    let children = owner.get_relation_or_load("children", &relation).await?;
    assert_eq!(children.len(), 2);
    assert_eq!(connection.table("seed_factory_owners").count("*").await?, 2);
    assert_eq!(
        connection.table("seed_factory_children").count("*").await?,
        2
    );
    schema.drop("seed_factory_children").await?;
    schema.drop("seed_factory_owners").await?;
    Ok(())
}

#[tokio::test]
async fn factory_named_custom_pivot_reloads_touched_members_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    verify_factory_pivot_touch(&root.connection(Some(&url), ""))
        .await
        .unwrap();
    root.close().await;
}
type PivotMembers = openrust::database::eloquent::relations::BelongsToMany<PivotOwner, PivotMember>;
struct PivotOwner(ModelState);
struct PivotMember(ModelState);
struct CustomFactoryPivot(ModelState);

macro_rules! pivot_model {
    ($model:ident, $metadata:expr, $($extra:item),*) => {
        impl Model for $model {
            fn metadata() -> &'static ModelMetadata {
                static METADATA: ModelMetadata = $metadata;
                &METADATA
            }
            fn from_state(state: ModelState) -> Self { Self(state) }
            fn state(&self) -> &ModelState { &self.0 }
            fn state_mut(&mut self) -> &mut ModelState { &mut self.0 }
            $($extra)*
        }
    };
}

fn pivot_members() -> PivotMembers {
    PivotMembers::new("factory_touch_pivot", "owner_id", "member_id", "id", "id")
        .using::<CustomFactoryPivot>()
        .with_pivot(["label", "meta"])
}

pivot_model!(
    PivotOwner,
    ModelMetadata::new("factory_touch_owners").timestamps(None, None),
    fn named_relations() -> NamedRelations<Self> {
        NamedRelations::default().nested("members", pivot_members)
    },
    fn touches() -> openrust::database::eloquent::Touches<Self> {
        openrust::database::eloquent::Touches::default().belongs_to_many(pivot_members())
    }
);
pivot_model!(
    PivotMember,
    ModelMetadata::new("factory_touch_members").timestamps(None, Some("updated_at")),
);
pivot_model!(
    CustomFactoryPivot,
    ModelMetadata::new("unused_factory_pivot")
        .incrementing(false)
        .timestamps(None, None),
    fn casts() -> &'static [(&'static str, AttributeCast)] {
        &[("meta", AttributeCast::Json)]
    }
);

async fn verify_factory_pivot_touch(db: &Connection) -> DatabaseResult<()> {
    use openrust::database::eloquent::{RelatedCollection, relations::Relation as _};

    const OLD: &str = "2000-01-01 00:00:00";
    let mut schema = db.get_schema_builder();
    schema
        .create("factory_touch_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("factory_touch_members", |table| {
            table.id();
            table.timestamp("updated_at", 0);
        })
        .await?;
    schema
        .create("factory_touch_pivot", |table| {
            table.unsigned_big_integer("owner_id");
            table.unsigned_big_integer("member_id");
            table.string("label", 32);
            table.json("meta").nullable();
        })
        .await?;
    let member = || Factory::<PivotMember>::new(Attributes::new).set("updated_at", OLD);
    let first = member().create_on(db).await?;
    let second = member().create_on(db).await?;
    let third = member().create_on(db).await?;
    let untouched = third.get_key().cloned().unwrap();
    let prior = RelatedCollection::from_shared(vec![std::sync::Arc::new(PivotMember::from_state(
        first.state().clone(),
    ))]);
    let owner = Factory::<PivotOwner>::new(Attributes::new)
        .after_making(move |owner| {
            owner.set_relation::<PivotMembers>("members", prior.clone());
            Ok(())
        })
        .has_attached_named::<PivotMember, PivotMembers>(
            "members",
            vec![first, second],
            vec![
                Attributes::from([("label".into(), "first".into())]),
                Attributes::from([("label".into(), "second".into())]),
            ],
        )
        .after_creating(|owner| {
            let members = owner
                .get_relation::<RelatedCollection<PivotMember>>("members")
                .unwrap();
            assert_eq!(members.len(), 2);
            let mut labels = Vec::new();
            for member in members.iter() {
                assert_ne!(member.get_attribute("updated_at"), Some(&OLD.into()));
                let pivot = member.get_relation::<CustomFactoryPivot>("pivot").unwrap();
                labels.push(pivot.get_attribute("label").cloned().unwrap());
            }
            assert!(labels.contains(&"first".into()));
            assert!(labels.contains(&"second".into()));
            Ok(())
        })
        .create_on(db)
        .await?;
    assert!(owner.relation_loaded("members"));
    assert_eq!(db.table("factory_touch_pivot").count("*").await?, 2);
    assert_eq!(
        db.scalar(
            "SELECT updated_at FROM factory_touch_members WHERE id = ?",
            &[untouched]
        )
        .await?,
        OLD.into()
    );
    let appended = Factory::<PivotOwner>::new(Attributes::new)
        .has_attached_named::<PivotMember, PivotMembers>(
            "members",
            member(),
            Attributes::from([
                ("label".into(), "new".into()),
                ("meta".into(), Value::Json(json!({"shared": true}))),
            ]),
        )
        .create_on(db)
        .await?;
    let members = appended
        .get_relation::<RelatedCollection<PivotMember>>("members")
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_ne!(members[0].get_attribute("updated_at"), Some(&OLD.into()));
    let pivot = members[0]
        .get_relation::<CustomFactoryPivot>("pivot")
        .unwrap();
    assert_eq!(
        pivot.get_attribute_value("meta")?,
        Some(Value::Json(json!({"shared": true})))
    );
    let before = db.table("factory_touch_members").count("*").await?;
    let generated = Factory::<PivotOwner>::new(Attributes::new)
        .has_attached_named::<PivotMember, PivotMembers>(
            "members",
            member(),
            vec![
                Attributes::from([
                    ("label".into(), "plain".into()),
                    ("meta".into(), Value::Null),
                ]),
                Attributes::from([
                    ("label".into(), "nested".into()),
                    ("meta".into(), Value::Json(json!({"foo": ["bar", 2]}))),
                ]),
            ],
        )
        .create_on(db)
        .await?;
    let members = pivot_members().get_results(&generated).await?;
    let mut meta = BTreeMap::new();
    for member in members.iter() {
        let pivot = member.get_relation::<CustomFactoryPivot>("pivot").unwrap();
        let Some(Value::Text(label)) = pivot.get_attribute("label") else {
            panic!("expected a pivot label");
        };
        meta.insert(label.clone(), pivot.get_attribute_value("meta")?);
    }
    assert_eq!(meta["plain"], Some(Value::Null));
    assert_eq!(
        meta["nested"],
        Some(Value::Json(json!({"foo": ["bar", 2]})))
    );
    assert_eq!(
        db.table("factory_touch_members").count("*").await?,
        before + 2
    );
    assert_eq!(db.table("factory_touch_pivot").count("*").await?, 5);
    Ok(())
}

#[test]
fn faker_seed_restarts_formats_without_touching_other_generators() -> DatabaseResult<()> {
    let sequence = |faker: &Faker| -> DatabaseResult<Vec<String>> {
        let mut values = Vec::new();
        for _ in 0..4 {
            values.extend([
                faker.name(),
                faker.address(),
                faker.company(),
                faker.sentence(5..6),
                faker.phone_number(),
                faker.safe_email(),
                faker.regexify("([A-Z]{2}|[0-9]{3})")?,
                faker.safe_hex_color(),
                faker.chrome(),
                faker.uuid().to_string(),
                faker.numerify(Some("#%#-###")),
                faker.random_html(Some(2), Some(3))?,
                faker
                    .date_time_between_expressions("2020-01-01", "2020-02-01")?
                    .strftime("%Y-%m-%dT%H:%M:%S%.f%:z")
                    .to_string(),
            ]);
        }
        Ok(values)
    };
    for locale in [
        "en-US", "fr-FR", "nl-NL", "is-IS", "fr-CH", "ms-MY", "zh-CN", "zh-TW", "ro-RO", "ja-JP",
    ] {
        let left = Faker::new(locale)?;
        let right = Faker::new(locale)?;
        left.seed(47);
        right.seed(Some(47));
        let first = sequence(&left)?;
        assert_eq!(sequence(&right)?, first, "{locale}");
        // Draws and reseeding on one handle leave the other handle's sequence intact.
        let next = sequence(&left)?;
        left.seed(48);
        assert_ne!(sequence(&left)?, first, "{locale}");
        assert_eq!(sequence(&right)?, next, "{locale}");
        left.seed(47);
        assert_eq!(sequence(&left)?, first, "{locale}");
        left.seed(None);
    }

    let faker = Faker::new("en-US")?;
    faker.seed(5);
    assert_eq!(faker.unique(false).number_between(1, 1)?, 1);
    faker.seed(5);
    assert!(faker.unique(false).number_between(1, 1).is_err());
    assert_eq!(faker.unique(true).number_between(1, 1)?, 1);
    Ok(())
}

#[test]
fn faker_providers_resolve_latest_case_insensitive_callbacks() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    let independent = Faker::new("en-US")?;
    faker.seed(42);
    independent.seed(42);
    assert_eq!(faker.format("name", &[])?, Value::from(independent.name()));

    faker.add_provider(
        FakerProvider::new()
            .formatter("name", |_, _| Ok("initial".into()))
            .formatter("reference", |faker, arguments| {
                let [Value::Integer(sequence)] = arguments else {
                    return Err(DatabaseError::InvalidQuery("reference expects an integer"));
                };
                Ok(format!("{sequence}:{}", faker.parse("{{ NaMe }}")?).into())
            })
            .formatter("mimeType", |_, _| Ok("overridden".into())),
    );
    let resolved = faker.get_formatter("NAME")?;
    faker.add_provider(FakerProvider::new().formatter("NaMe", |_, _| Ok("latest".into())));
    assert_eq!(resolved(&[])?, Value::from("initial"));
    assert_eq!(faker.format("name", &[])?, Value::from("latest"));
    assert_ne!(faker.format("mimeType", &[])?, Value::from("overridden"));
    assert_eq!(
        faker.format("REFERENCE", &[7.into()])?,
        Value::from("7:latest")
    );
    assert!(matches!(
        faker.format("reference", &["seven".into()]),
        Err(DatabaseError::InvalidQuery("reference expects an integer"))
    ));
    assert!(matches!(
        independent.format("reference", &[]),
        Err(DatabaseError::UnknownFakerFormatter(name)) if name == "reference"
    ));
    assert!(matches!(
        faker.parse("before {{ unknown }} after"),
        Err(DatabaseError::UnknownFakerFormatter(name)) if name == "unknown"
    ));
    assert_eq!(
        faker.parse("left {{  name  }} / {{name}} right")?,
        "left {{  name  }} / latest right"
    );
    Ok(())
}

#[test]
fn faker_taiwanese_company_resolves_provider_and_propagates_errors() -> DatabaseResult<()> {
    let faker = Faker::new("zh-TW")?;
    faker.add_provider(FakerProvider::new().formatter("lastNameEn", |_, _| Ok("Lin".into())));
    assert!(faker.company_en()?.starts_with("Lin"));
    assert!(
        matches!(faker.format("companyEn", &[])?, Value::Text(name) if name.starts_with("Lin"))
    );
    faker.add_provider(FakerProvider::new().formatter("lastNameEn", |_, _| Ok(false.into())));
    assert!(matches!(
        faker.company_en(),
        Err(DatabaseError::InvalidQuery(
            "Faker lastNameEn must return text"
        ))
    ));
    faker.add_provider(FakerProvider::new().formatter("lastNameEn", |_, _| {
        Err(DatabaseError::UnknownFakerFormatter("upstreamName".into()))
    }));
    assert!(matches!(
        faker.company_en(),
        Err(DatabaseError::UnknownFakerFormatter(name)) if name == "upstreamName"
    ));
    Ok(())
}

#[test]
fn faker_named_numeric_and_pattern_arguments_use_builtin_contracts() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    faker.add_provider(
        FakerProvider::new()
            .formatter("numberBetween", |_, _| Ok((-1).into()))
            .formatter("bothify", |_, _| Ok("custom".into())),
    );
    for _ in 0..16 {
        assert!(
            matches!(faker.format("numberBetween", &[])?, Value::Integer(n) if (0..=i64::from(i32::MAX)).contains(&n))
        );
        assert!(matches!(
            faker.format("numberBetween", &[9.into(), 7.into()])?,
            Value::Integer(7..=9)
        ));
        assert!(matches!(
            faker.format("randomNumber", &[2.into(), true.into()])?,
            Value::Integer(10..=99)
        ));
        assert!(matches!(
            faker.format("randomDigitNot", &[0.into()])?,
            Value::Integer(1..=9)
        ));
        assert!(matches!(
            faker.format("randomDigitNotZero", &[])?,
            Value::Integer(1..=9)
        ));
    }
    assert_eq!(
        faker.format("NUMBERBETWEEN", &[7.into(), 7.into()])?,
        7.into()
    );
    assert_eq!(
        faker.format("numberBetween", &[Value::Null, Value::Null])?,
        0.into()
    );
    assert_eq!(
        faker.format(
            "randomFloat",
            &[2.into(), Value::Real(1.25), Value::Real(1.25)]
        )?,
        Value::Real(1.25)
    );
    assert_eq!(
        faker.format("latitude", &[Value::Null, 0.into()])?,
        Value::Real(0.0)
    );
    assert_eq!(
        faker.format("longitude", &[10.into(), 10.into()])?,
        Value::Real(10.0)
    );
    assert_eq!(faker.format("boolean", &[Value::Null])?, false.into());
    assert_eq!(faker.format("boolean", &[100.into()])?, true.into());
    assert_eq!(faker.format("bothify", &["a##".into()])?, "custom".into());
    assert_eq!(faker.format("numerify", &[Value::Null])?, Value::Null);
    assert_eq!(
        faker.format("lexify", &["literal".into()])?,
        "literal".into()
    );
    assert_eq!(faker.format("asciify", &[Value::Null])?, "".into());
    assert_eq!(faker.format("regexify", &["^X{2}$".into()])?, "XX".into());
    assert_eq!(faker.format("regexify", &[])?, "".into());
    let regex = faker.get_formatter("regexify")?;
    assert_eq!(regex(&["^x$".into()])?, "x".into());

    assert!(faker.format("randomDigitNot", &[]).is_err());
    assert!(faker.format("numberBetween", &["5".into()]).is_err());
    assert!(
        faker
            .format("numberBetween", &[Value::Unsigned(u64::MAX)])
            .is_err()
    );
    assert!(
        faker
            .format("numberBetween", &[1.into(), 1.into(), 1.into()])
            .is_err()
    );
    assert!(
        faker
            .format("randomFloat", &[0.into(), Value::Real(f64::NAN)])
            .is_err()
    );
    assert!(
        faker
            .format("randomNumber", &[10.into(), true.into()])
            .is_err()
    );
    assert!(faker.format("regexify", &["[".into()]).is_err());
    Ok(())
}

#[test]
fn faker_named_colors_keep_local_provider_and_core_catalogs_distinct() -> DatabaseResult<()> {
    const COLOR: &str = r"Faker\Extension\ColorExtension";
    let chinese = Faker::new("zh-Hant-TW")?;
    let english = Faker::new("en-US")?;
    let core = chinese.ext(COLOR)?;
    for (method, typed) in [
        ("colorName", Faker::color_name as fn(&Faker) -> &'static str),
        ("safeColorName", Faker::safe_color_name),
    ] {
        chinese.seed(94);
        let expected = typed(&chinese);
        assert!(!expected.is_ascii());
        chinese.seed(94);
        assert_eq!(chinese.format(method, &[])?, expected.into());
        english.seed(94);
        let default = typed(&english);
        assert!(default.is_ascii());
        chinese.seed(94);
        assert_eq!(core.format(method, &[])?, default.into());
    }
    for (method, typed) in [
        ("hexColor", Faker::hex_color as fn(&Faker) -> String),
        ("safeHexColor", Faker::safe_hex_color),
        ("rgbColor", Faker::rgb_color),
        ("rgbCssColor", Faker::rgb_css_color),
        ("rgbaCssColor", Faker::rgba_css_color),
        ("hslColor", Faker::hsl_color),
    ] {
        chinese.seed(94);
        let expected = typed(&chinese);
        chinese.seed(94);
        assert_eq!(chinese.format(method, &[])?, expected.clone().into());
        chinese.seed(94);
        assert_eq!(core.format(method, &[])?, expected.into());
    }
    chinese.seed(94);
    let rgb = Value::Json(json!(chinese.rgb_color_as_array()));
    chinese.seed(94);
    assert_eq!(chinese.format("rgbColorAsArray", &[])?, rgb);
    chinese.seed(94);
    let hsl = Value::Json(json!(chinese.hsl_color_as_array()));
    chinese.seed(94);
    assert_eq!(chinese.format("hslColorAsArray", &[])?, hsl);
    chinese.seed(94);
    assert_eq!(core.format("hslColorAsArray", &[])?, hsl);

    chinese.add_provider(FakerProvider::new().formatter("colorName", |_, _| Ok("override".into())));
    assert_eq!(chinese.parse("{{colorName}}")?, "override");
    assert_ne!(core.format("colorName", &[])?, "override".into());
    assert!(core.format("unknownColor", &[]).is_err());
    Ok(())
}

#[test]
fn faker_named_dates_preserve_provider_locales_and_instance_zones() -> DatabaseResult<()> {
    let epoch = [0_i64.into()];
    let czech = Faker::new("cs-CZ")?;
    let other = Faker::new("en-US")?;
    assert_eq!(czech.format("getDefaultTimezone", &[])?, Value::Null);
    assert_eq!(czech.format("monthNameGenitive", &epoch)?, "ledna".into());
    assert_eq!(czech.format("monthName", &epoch)?, "leden".into());
    assert_eq!(czech.format("dayOfMonth", &epoch)?, "1".into());
    assert_eq!(czech.format("dayOfWeek", &epoch)?, "čtvrtek".into());
    assert_eq!(other.format("dayOfWeek", &epoch)?, "Thursday".into());
    let core = czech.ext(r"Faker\Extension\DateTimeExtension")?;
    assert_eq!(core.format("monthName", &epoch)?, "January".into());
    assert_eq!(core.format("dayOfMonth", &epoch)?, "01".into());
    assert!(other.format("formattedDate", &[]).is_err());

    czech.add_provider(FakerProvider::new().formatter("dayOfMonth", |_, _| Ok("7".into())));
    assert!(
        matches!(czech.format("formattedDate", &[])?, Value::Text(value) if value.starts_with("7. "))
    );
    assert_eq!(
        czech.format("setDefaultTimezone", &["Asia/Tokyo".into()])?,
        Value::Null
    );
    assert_eq!(
        czech.format("getDefaultTimezone", &[])?,
        "Asia/Tokyo".into()
    );
    assert_eq!(other.format("getDefaultTimezone", &[])?, Value::Null);
    assert_eq!(
        czech.format("dateTime", &epoch)?,
        "1970-01-01T09:00:00+09:00".into()
    );
    assert_eq!(
        czech.format("dateTime", &[0_i64.into(), "UTC".into()])?,
        "1970-01-01T00:00:00+00:00".into()
    );
    assert!(
        czech
            .format("setDefaultTimezone", &["No/Zone".into()])
            .is_err()
    );
    assert_eq!(
        czech.format("getDefaultTimezone", &[])?,
        "Asia/Tokyo".into()
    );
    czech.format("setDefaultTimezone", &[Value::Null])?;
    assert_eq!(czech.format("getDefaultTimezone", &[])?, Value::Null);
    assert_eq!(
        czech.format("dateTimeBetween", &[0_i64.into(), 0_i64.into()])?,
        "1970-01-01T00:00:00+00:00".into()
    );

    let now = jiff::Timestamp::now();
    let old = (now - jiff::SignedDuration::from_hours(480)).as_second();
    assert!(other.format("dateTimeThisMonth", &[old.into()]).is_ok());
    Ok(())
}

#[test]
fn faker_named_miscellaneous_and_base_match_typed_values() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    for (method, sample) in [
        ("locale", Faker::random_locale as fn(&Faker) -> &'static str),
        ("languageCode", Faker::language_code),
        ("countryCode", Faker::country_code),
        ("countryISOAlpha3", Faker::country_iso_alpha3),
        ("currencyCode", Faker::currency_code),
        ("emoji", Faker::emoji),
    ] {
        faker.seed(94);
        let expected = sample(&faker);
        faker.seed(94);
        assert_eq!(faker.format(method, &[])?, expected.into());
    }
    assert_eq!(faker.locale(), "en-US");

    assert_eq!(faker.format("toLower", &["ÉCOLE".into()])?, "école".into());
    assert_eq!(
        faker.format("toUpper", &["Straße".into()])?,
        "STRASSE".into()
    );
    assert_eq!(faker.format("toLower", &[])?, "".into());
    assert!(faker.format("toUpper", &[7_i64.into()]).is_err());

    faker.seed(94);
    let expected = faker.biased_number_between(Some(-5), Some(5), None)?;
    faker.seed(94);
    assert_eq!(
        faker.format(
            "biasedNumberBetween",
            &[(-5_i64).into(), 5_i64.into(), "sqrt".into()]
        )?,
        expected.into()
    );
    assert!(matches!(
        faker.unique(false).format("biasedNumberBetween", &[])?,
        Value::Integer(0..=100)
    ));
    assert!(
        faker
            .format("biasedNumberBetween", &[5_i64.into(), (-5_i64).into()])
            .is_err()
    );
    assert!(
        faker
            .format(
                "biasedNumberBetween",
                &[Value::Null, Value::Null, "unknown".into()]
            )
            .is_err()
    );
    Ok(())
}

#[test]
fn faker_named_company_and_person_reuse_regional_generators() -> DatabaseResult<()> {
    let malaysia = Faker::new("ms-MY")?;
    malaysia.seed(94);
    let name = malaysia.first_name_male_chinese()?;
    malaysia.seed(94);
    assert_eq!(malaysia.format("firstNameMaleChinese", &[])?, name.into());
    malaysia.seed(94);
    let industry = malaysia.industry()?;
    malaysia.seed(94);
    assert_eq!(malaysia.format("industry", &[])?, industry.into());
    assert!(matches!(malaysia.format("companyName", &[])?, Value::Text(name) if !name.is_empty()));
    assert!(
        matches!(malaysia.format("hajjah", &[])?, Value::Text(value) if ["", "Hajjah ", "Hjh "].contains(&value.as_str()))
    );

    let mongolia = Faker::new("mn-MN")?;
    assert!(
        matches!(mongolia.format("namePrefix", &[])?, Value::Text(value) if value.chars().count() == 1)
    );
    let france = Faker::new("fr-FR")?;
    france.seed(94);
    let slogan = france.catch_phrase()?;
    france.seed(94);
    assert_eq!(france.format("catchPhrase", &[])?, slogan.into());
    assert!(
        matches!(france.format("catchPhraseNoun", &[])?, Value::Text(noun) if !noun.is_empty())
    );
    let armenia = Faker::new("hy-AM")?;
    assert_eq!(armenia.format("bs", &[])?, "".into());

    let taiwan = Faker::new("zh-Hant-TW")?;
    let pool = Value::Json(json!({"name":"明"}));
    assert_eq!(
        taiwan.format("randomName", &[pool.clone(), 2_i64.into()])?,
        "明明".into()
    );
    assert_eq!(
        taiwan.format("randomName", &[pool.clone(), (-1_i64).into()])?,
        "".into()
    );
    assert_eq!(
        taiwan.format("randomName", &[Value::Json(json!([])), 4_i64.into()])?,
        "".into()
    );
    assert!(
        taiwan
            .format("randomName", &[pool, i64::MAX.into()])
            .is_err()
    );
    assert!(
        taiwan
            .format("randomName", &[Value::Json(json!([3])), 2_i64.into()])
            .is_err()
    );
    assert!(france.format("firstNameMaleChinese", &[]).is_err());

    let singapore = Faker::new("en-SG")?;
    let id = singapore.format("singaporeId", &["2002-02-01".into(), true.into()])?;
    assert!(matches!(id, Value::Text(id) if id.starts_with('G') && id.len() == 9));
    assert!(
        singapore
            .format("singaporeId", &["2002-02-30".into()])
            .is_err()
    );
    assert!(
        singapore
            .format("singaporeId", &[Value::Null, Value::Null])
            .is_err()
    );
    Ok(())
}

#[test]
fn faker_named_address_and_plate_components_follow_provider_contracts() -> DatabaseResult<()> {
    let taiwan = Faker::new("zh-Hant-TW")?;
    assert!(
        matches!(taiwan.format("localLatitude", &[])?, Value::Real(value) if (22.0..=25.0).contains(&value))
    );
    assert!(
        matches!(taiwan.format("localLongitude", &[])?, Value::Real(value) if (120.0..=122.0).contains(&value))
    );
    assert!(matches!(
        taiwan.format("randomNumber2", &[])?,
        Value::Integer(1..=100)
    ));
    assert!(matches!(
        taiwan.format("randomNumber3", &[])?,
        Value::Integer(1..=1000)
    ));
    let Value::Text(digit) = taiwan.format("randomChineseNumber", &[])? else {
        panic!("expected a textual digit");
    };
    assert_eq!(digit.chars().count(), 1);
    assert!("一二三四五六七八九".contains(&digit));

    let poland = Faker::new("pl-PL")?;
    poland.seed(94);
    let default = poland.license_plate()?;
    poland.seed(94);
    assert_eq!(poland.format("licensePlate", &[])?, default.clone().into());
    poland.seed(94);
    assert_eq!(
        poland.format(
            "licensePlate",
            &[false.into(), Value::Null, Value::Json(json!(["missing"]))]
        )?,
        default.into()
    );

    for province in [json!(["dolnośląskie"]), json!({"region":"dolnośląskie"})] {
        let args = [
            false.into(),
            Value::Json(province),
            Value::Json(json!(["Wrocław"])),
        ];
        let Value::Text(plate) = poland.format("licensePlate", &args)? else {
            panic!("expected a textual plate");
        };
        let (prefix, _) = plate.split_once(' ').expect("plate separator");
        assert!(matches!(prefix, "DW" | "DX" | "VW" | "VX"));
    }
    let army = [true.into(), Value::Json(json!(["army"]))];
    assert!(
        matches!(poland.unique(false).format("licensePlate", &army)?, Value::Text(plate) if plate.starts_with('U'))
    );
    assert!(poland.format("licensePlate", &[Value::Null]).is_err());
    assert!(
        poland
            .format("licensePlate", &[false.into(), "army".into()])
            .is_err()
    );
    assert!(
        poland
            .format("licensePlate", &[false.into(), Value::Json(json!([42]))])
            .is_err()
    );

    let malaysia = Faker::new("ms-MY")?;
    for (name, generate) in [
        (
            "peninsularPrefix",
            Faker::peninsular_prefix as fn(&Faker) -> DatabaseResult<&'static str>,
        ),
        ("sabahPrefix", Faker::sabah_prefix),
        ("sarawakPrefix", Faker::sarawak_prefix),
        ("specialPrefix", Faker::special_prefix),
        ("validAlphabet", Faker::valid_alphabet),
    ] {
        malaysia.seed(94);
        let expected = generate(&malaysia)?;
        malaysia.seed(94);
        assert_eq!(malaysia.format(name, &[])?, expected.into());
    }
    malaysia.seed(94);
    let expected = malaysia.jpj_number_plate()?;
    malaysia.seed(94);
    assert_eq!(malaysia.format("jpjNumberPlate", &[])?, expected.into());
    assert!(matches!(
        malaysia.format("numberSequence", &[])?,
        Value::Integer(1..=9999)
    ));

    let us = Faker::new("en-US")?;
    for name in [
        "localLatitude",
        "localLongitude",
        "randomChineseNumber",
        "licensePlate",
        "jpjNumberPlate",
    ] {
        assert!(us.format(name, &[]).is_err());
    }
    Ok(())
}

#[test]
fn faker_named_phone_formatters_preserve_regional_signatures() -> DatabaseResult<()> {
    let malaysia = Faker::new("ms-MY")?;
    for (name, default, with_options) in [
        (
            "mobileNumber",
            Faker::mobile_number as fn(&Faker) -> DatabaseResult<String>,
            Faker::mobile_number_with_options as fn(&Faker, bool, bool) -> DatabaseResult<String>,
        ),
        (
            "fixedLineNumber",
            Faker::fixed_line_number,
            Faker::fixed_line_number_with_options,
        ),
        (
            "voipNumber",
            Faker::voip_number,
            Faker::voip_number_with_options,
        ),
    ] {
        malaysia.seed(94);
        let expected = default(&malaysia)?;
        malaysia.seed(94);
        assert_eq!(malaysia.format(name, &[])?, expected.into());

        for (prefix, formatted) in [(false, false), (false, true), (true, false), (true, true)] {
            malaysia.seed(94);
            let expected = with_options(&malaysia, prefix, formatted)?;
            malaysia.seed(94);
            assert_eq!(
                malaysia.format(name, &[prefix.into(), formatted.into()])?,
                expected.into()
            );
        }
        assert!(malaysia.format(name, &["false".into()]).is_err());
        assert!(
            malaysia
                .format(name, &[true.into(), false.into(), false.into()])
                .is_err()
        );
    }

    let brazil = Faker::new("pt-BR")?;
    for (name, generate) in [
        (
            "cellphoneNumber",
            Faker::cellphone_number as fn(&Faker, Option<bool>) -> DatabaseResult<String>,
        ),
        ("landlineNumber", Faker::landline_number),
        ("cellphone", Faker::cellphone),
        ("landline", Faker::landline),
        ("phone", Faker::phone),
    ] {
        brazil.seed(94);
        let expected = generate(&brazil, Some(false))?;
        assert!(expected.bytes().all(|byte| byte.is_ascii_digit()));
        for argument in [false.into(), Value::Null] {
            brazil.seed(94);
            assert_eq!(brazil.format(name, &[argument])?, expected.clone().into());
        }
    }

    for (locale, name, generate) in [
        (
            "en-CA",
            "phoneNumberWithExtension",
            Faker::phone_number_with_extension as fn(&Faker) -> DatabaseResult<String>,
        ),
        ("ko-KR", "CELLphoneNUMBER", Faker::cell_phone_number),
        (
            "fr-FR",
            "phoneNumber06WithSeparator",
            Faker::phone_number06_with_separator,
        ),
        (
            "en-SG",
            "tollFreeInternationalNumber",
            Faker::toll_free_international_number,
        ),
        ("el-GR", "premiumRateNumber", Faker::premium_rate_number),
        (
            "fi-FI",
            "e164MobileNetworkAreaCode",
            Faker::e164_mobile_network_area_code,
        ),
        ("en-US", "areaCode", Faker::area_code),
    ] {
        let faker = Faker::new(locale)?;
        faker.seed(94);
        let expected = generate(&faker)?;
        faker.seed(94);
        assert_eq!(faker.format(name, &[])?, expected.into());
    }

    let us = Faker::new("en-US")?;
    assert!(us.format("phoneNumber06", &[]).is_err());
    assert!(us.format("separator", &[]).is_err());
    let singapore = Faker::new("en-SG")?;
    assert!(singapore.format("mobileNumber", &[false.into()]).is_err());
    singapore.seed(94);
    for _ in 0..32 {
        let Value::Text(number) = singapore.format("voipNumber", &[])? else {
            panic!("expected a textual VoIP number");
        };
        let local = number
            .strip_prefix("+65")
            .or_else(|| number.strip_prefix("65"))
            .unwrap_or(&number)
            .trim_start();
        assert!(local.starts_with('3'));
        assert_eq!(local.bytes().filter(u8::is_ascii_digit).count(), 8);
    }
    Ok(())
}

#[test]
fn faker_named_file_and_remaining_regional_arguments_preserve_contracts() -> DatabaseResult<()> {
    let root = Root::new();
    let source = root.0.join("copy-source");
    let target = root.0.join("copy-target");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::create_dir_all(&target).unwrap();
    fs::write(source.join("nested/ignored.txt"), b"ignored").unwrap();
    let paths: [Value; _] = [
        source.to_str().unwrap().into(),
        target.to_str().unwrap().into(),
    ];
    let faker = Faker::new("en-US")?;
    assert!(
        matches!(faker.format("file", &paths), Err(DatabaseError::FakerIo(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
    fs::write(source.join("binary.gz"), b"\0\xffpayload").unwrap();
    faker.seed(94);
    let Value::Text(first) = faker.format("file", &paths)? else {
        panic!("expected a full path");
    };
    faker.seed(94);
    let Value::Text(second) =
        faker.format("file", &[paths[0].clone(), paths[1].clone(), false.into()])?
    else {
        panic!("expected a basename");
    };
    assert_eq!(Path::new(&first).parent(), Some(target.as_path()));
    assert_eq!(Path::new(&second).components().count(), 1);
    assert!(second.ends_with(".gz"));
    assert_ne!(Path::new(&first), target.join(&second));
    assert_eq!(fs::read(&first).unwrap(), b"\0\xffpayload");
    assert_eq!(fs::read(target.join(second)).unwrap(), b"\0\xffpayload");
    assert!(matches!(
        faker.format("file", &[paths[0].clone(), source.join("nested/..").to_str().unwrap().into()]),
        Err(DatabaseError::FakerIo(error)) if error.kind() == std::io::ErrorKind::InvalidInput
    ));
    assert!(matches!(
        faker.format("file", &[]),
        Err(DatabaseError::FakerIo(_))
    ));

    let malaysia = Faker::new("ms-MY")?;
    let Value::Text(id) = malaysia.format("myKadNumber", &["female".into(), ":".into()])? else {
        panic!("expected MyKad text");
    };
    assert_eq!(id.matches(':').count(), 2);
    assert!(matches!(
        id.as_bytes().last(),
        Some(b'0' | b'2' | b'4' | b'6' | b'8')
    ));
    assert_eq!(malaysia.format("countryCodePrefix", &[])?, "+6".into());
    let romania = Faker::new("ro-RO")?;
    assert!(matches!(
        romania.format("cnp", &["female".into(), "1981-06-16".into(), "B1".into()])?,
        Value::Text(number) if number.starts_with("2810616") && number.len() == 13
    ));
    assert!(
        romania
            .format(
                "cnp",
                &["female".into(), "1981-06-16".into(), "missing".into()]
            )
            .is_err()
    );
    let taiwan = Faker::new("zh-TW")?;
    assert!(matches!(
        taiwan.format("VAT", &[])?,
        Value::Integer(10_000_000..=99_999_999)
    ));
    assert_eq!(
        faker.format("calculateRoutingNumberChecksum", &["02100002".into()])?,
        1.into()
    );
    assert!(
        matches!(faker.format("calculateModulus97", &["1234567".into()])?, Value::Text(code) if code.len() == 2)
    );
    assert!(matches!(faker.format("sha256", &[])?, Value::Text(hash) if hash.len() == 64));
    assert!(
        matches!(faker.format("randomHtml", &[1.into(), 1.into()])?, Value::Text(html) if html.starts_with("<html>"))
    );
    Ok(())
}

#[tokio::test]
async fn faker_named_identity_checks_and_image_parameters_are_typed() -> DatabaseResult<()> {
    let cz = Faker::new("cs-CZ")?;
    assert!(matches!(
        cz.format("birthNumber", &["female".into(), 21.into(), 21.into(), 100.into()])?,
        Value::Text(number) if number.contains('/')
    ));
    let pl = Faker::new("pl-PL")?;
    assert!(matches!(
        pl.format("pesel", &["2000-02-29".into(), "F".into()])?,
        Value::Text(number) if number.starts_with("002229") && number.len() == 11
    ));
    assert!(pl.format("pesel", &["2001-02-29".into()]).is_err());
    let sa = Faker::new("ar-SA")?;
    assert!(matches!(
        sa.format("nationalIdNumber", &[])?,
        Value::Text(number) if number.starts_with('1') && number.len() == 10
    ));
    let tr = Faker::new("tr-TR")?;
    let identity = tr.format("tcNo", &[])?;
    assert_eq!(tr.format("tcNoIsValid", &[identity])?, true.into());

    let faker = Faker::new("en-US")?;
    assert_eq!(
        faker.format(
            "imageUrl",
            &[
                64.into(),
                48.into(),
                "樣圖".into(),
                false.into(),
                "demo".into(),
                true.into(),
                "jpeg".into()
            ]
        )?,
        "https://via.placeholder.com/64x48.jpeg/CCCCCC?text=%E6%A8%A3%E5%9C%96+demo".into()
    );
    assert_eq!(
        faker.format("getFormatConstants", &[])?,
        Value::Json(json!({"jpg":2,"jpeg":2,"png":3}))
    );
    assert!(faker.format("imageUrl", &[(-1).into()]).is_err());
    let invalid = [
        Value::Null,
        64.into(),
        48.into(),
        Value::Null,
        true.into(),
        false.into(),
        Value::Null,
        true.into(),
        "bmp".into(),
    ];
    assert!(
        matches!(faker.format_async("image", &invalid).await, Err(DatabaseError::FakerIo(error)) if error.kind() == std::io::ErrorKind::InvalidInput)
    );
    faker.add_provider(FakerProvider::new().formatter("image", |_, _| Ok("custom.png".into())));
    assert_eq!(faker.format_async("IMAGE", &[]).await?, "custom.png".into());
    Ok(())
}

#[test]
fn faker_named_regional_address_person_and_payment_follow_the_locale() -> DatabaseResult<()> {
    let ru = Faker::new("ru-RU")?;
    ru.seed(331);
    let surname = ru.last_name_with_gender(Some("female"));
    ru.seed(331);
    assert_eq!(ru.format("lastName", &["female".into()])?, surname.into());
    let ja = Faker::new("ja-JP")?;
    ja.seed(332);
    let prefecture = ja.prefecture()?;
    ja.seed(332);
    assert_eq!(ja.format("prefecture", &[])?, prefecture.into());
    let my = Faker::new("ms-MY")?;
    assert!(matches!(
        my.format("postcode", &["perlis".into()])?,
        Value::Text(code) if code.len() == 5
    ));
    assert!(my.format("postcode", &["missing-state".into()]).is_err());
    let fr = Faker::new("fr-FR")?;
    assert!(matches!(
        fr.format("siren", &[])?,
        Value::Text(number) if number.split_whitespace().count() == 3
    ));
    assert!(matches!(
        fr.format("siren", &[false.into()])?,
        Value::Text(number) if number.len() == 9 && number.bytes().all(|byte| byte.is_ascii_digit())
    ));
    assert!(matches!(
        fr.format("department", &[])?,
        Value::Json(serde_json::Value::Array(parts)) if parts.len() == 2
    ));

    let en = Faker::new("en-US")?;
    assert!(en.format("prefecture", &[]).is_err());
    assert!(
        en.format("creditCardNumber", &["missing-type".into()])
            .is_err()
    );
    let Value::Text(card) = en.format(
        "creditCardNumber",
        &["Visa".into(), true.into(), ":".into()],
    )?
    else {
        panic!("expected a card number");
    };
    assert_eq!(card.split(':').count(), 4);
    let Value::Json(details) = en.format("creditCardDetails", &[])? else {
        panic!("expected credit card details");
    };
    for key in ["type", "number", "name", "expirationDate"] {
        assert!(details[key].is_string());
    }
    assert!(matches!(
        en.format("creditCardExpirationDateString", &[true.into(), "Y-m".into()])?,
        Value::Text(date) if date.len() == 7 && date.as_bytes()[4] == b'-'
    ));
    assert!(matches!(
        en.format("iban", &["DE".into(), "123".into(), 18.into()])?,
        Value::Text(code) if code.len() == 22 && code.starts_with("DE") && &code[4..7] == "123"
    ));
    let uk = Faker::new("en-GB")?;
    assert!(matches!(
        uk.format("vat", &["gov".into()])?,
        Value::Text(value) if value.starts_with("GBGD")
    ));
    ja.add_provider(FakerProvider::new().formatter("prefecture", |_, _| Ok("override".into())));
    assert_eq!(ja.format("PREFECTURE", &[])?, "override".into());
    Ok(())
}

#[test]
fn faker_named_text_and_internet_reuse_typed_generators() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    faker.seed(91);
    let expected = faker.sentence_with_word_count(2, false);
    faker.seed(91);
    assert_eq!(
        faker.format("sentence", &[2.into(), false.into()])?,
        expected.into()
    );
    let Value::Json(serde_json::Value::Array(words)) = faker.format("words", &[])? else {
        panic!("expected an array of words");
    };
    assert_eq!(words.len(), 3);
    assert!(words.iter().all(serde_json::Value::is_string));
    assert_eq!(
        faker.format("words", &[Value::Null, true.into()])?,
        "".into()
    );
    assert_eq!(
        faker.format("sentences", &[0.into()])?,
        Value::Json(json!([]))
    );
    assert_eq!(faker.format("paragraph", &[0.into()])?, "".into());
    let Value::Text(paragraphs) = faker.format("paragraphs", &[2.into(), true.into()])? else {
        panic!("expected joined paragraphs");
    };
    assert_eq!(paragraphs.split("\n\n").count(), 2);
    assert!(faker.format("text", &[4.into()]).is_err());
    assert!(faker.format("text", &[Value::Null]).is_err());
    assert!(faker.format("words", &[i64::MAX.into()]).is_err());
    assert!(faker.format("realText", &[200.into(), 0.into()]).is_err());

    let Value::Text(password) = faker.format("password", &[8.into(), 8.into()])? else {
        panic!("expected a password string");
    };
    assert_eq!(password.len(), 8);
    assert!(password.bytes().all(|byte| byte.is_ascii_graphic()));
    let Value::Text(slug) = faker.format("slug", &[2.into(), false.into()])? else {
        panic!("expected a slug string");
    };
    assert_eq!(slug.split('-').count(), 2);
    assert!(matches!(
        faker.get_formatter("IPv4")?(&[])?,
        Value::Text(ip) if ip.parse::<std::net::Ipv4Addr>().is_ok()
    ));
    assert!(matches!(
        faker.format("userAgent", &[])?,
        Value::Text(agent) if agent.contains('/')
    ));
    faker.add_provider(FakerProvider::new().formatter("word", |_, _| Ok("override".into())));
    assert_eq!(faker.format("word", &[])?, "override".into());
    Ok(())
}

#[test]
fn faker_named_collection_formatters_keep_keys_types_and_bounds() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    faker.seed(123);
    assert!(matches!(
        faker.format("randomElement", &[])?,
        Value::Text(value) if matches!(value.as_str(), "a" | "b" | "c")
    ));
    assert_eq!(
        faker.format("randomElement", &[Value::Json(json!([]))])?,
        Value::Null
    );
    assert_eq!(
        faker.format("randomElement", &[Value::Json(json!({"one":true}))])?,
        true.into()
    );
    assert_eq!(
        faker.format("randomKey", &[Value::Json(json!({"named":9}))])?,
        "named".into()
    );
    assert_eq!(
        faker.format("randomKey", &[Value::Json(json!([99]))])?,
        0.into()
    );
    assert_eq!(faker.format("randomKey", &[])?, Value::Null);
    assert_eq!(
        faker.format("randomElements", &[Value::Json(json!([9]))])?,
        Value::Json(json!([9]))
    );
    assert_eq!(
        faker.format(
            "randomElements",
            &[Value::Json(json!([9])), 3.into(), true.into()]
        )?,
        Value::Json(json!([9, 9, 9]))
    );
    assert_eq!(
        faker.format("randomElements", &[Value::Json(json!([9])), Value::Null])?,
        Value::Json(json!([9]))
    );
    assert!(
        faker
            .format("randomElements", &[Value::Json(json!([9])), 2.into()])
            .is_err()
    );
    assert!(
        faker
            .format(
                "randomElements",
                &[Value::Json(json!([9])), i64::MAX.into(), true.into()]
            )
            .is_err()
    );
    assert_eq!(
        faker.format("randomElements", &[Value::Json(json!([])), 0.into()])?,
        Value::Json(json!([]))
    );
    assert_eq!(
        faker.format("shuffle", &[Value::Json(json!({"named":{"x":1}}))])?,
        Value::Json(json!([{"x":1}]))
    );
    assert_eq!(faker.format("shuffleArray", &[])?, Value::Json(json!([])));
    assert_eq!(faker.format("shuffle", &[])?, "".into());
    let Value::Text(text) = faker.format("shuffleString", &["繁體🙂é".into(), "UTF-8".into()])?
    else {
        panic!("expected Unicode text");
    };
    let mut characters: Vec<_> = text.chars().collect();
    characters.sort_unstable();
    let mut expected: Vec<_> = "繁體🙂é".chars().collect();
    expected.sort_unstable();
    assert_eq!(characters, expected);
    assert!(
        faker
            .format("shuffleString", &["文字".into(), "UTF-16".into()])
            .is_err()
    );
    let bytes = Value::Bytes(vec![0xff, 0x00]);
    assert_eq!(
        faker.format("passthrough", std::slice::from_ref(&bytes))?,
        bytes
    );
    Ok(())
}

#[test]
fn faker_qualified_date_ranges_match_extension_boundaries() -> DatabaseResult<()> {
    use jiff::{Timestamp, tz::TimeZone};

    let faker = Faker::new("en-US")?;
    let date = faker.ext(r"Faker\Extension\DateTimeExtension")?;
    assert_eq!(
        date.format(
            "dateTimeBetween",
            &["@0".into(), 0.into(), "Asia/Shanghai".into()]
        )?,
        "1970-01-01T08:00:00+08:00".into()
    );
    assert!(
        date.format("dateTimeBetween", &[1.into(), 0.into()])
            .is_err()
    );
    assert!(
        date.format(
            "dateTimeInInterval",
            &["@0".into(), "invalid interval".into()]
        )
        .is_err()
    );
    assert_eq!(
        date.format("dateTimeInInterval", &[0.into(), "+0 seconds".into()])?,
        "1970-01-01T00:00:00+00:00".into()
    );
    let Value::Text(sample) = date.format("dateTimeInInterval", &["@0".into(), "-1 day".into()])?
    else {
        panic!("expected a date string");
    };
    assert!((-86_400..=0).contains(&sample.parse::<Timestamp>().unwrap().as_second()));

    let Value::Text(week) = date.format("dateTimeThisWeek", &["monday this week".into()])? else {
        panic!("expected a date string");
    };
    assert_eq!(
        week.parse::<Timestamp>()
            .unwrap()
            .to_zoned(TimeZone::UTC)
            .weekday(),
        jiff::civil::Weekday::Monday
    );
    let Value::Text(month) =
        date.format("dateTimeThisMonth", &["first day of this month".into()])?
    else {
        panic!("expected a date string");
    };
    assert_eq!(
        month
            .parse::<Timestamp>()
            .unwrap()
            .to_zoned(TimeZone::UTC)
            .day(),
        1
    );
    let Value::Text(year) = date.format("dateTimeThisYear", &["first day of january".into()])?
    else {
        panic!("expected a date string");
    };
    assert_eq!(
        year.parse::<Timestamp>()
            .unwrap()
            .to_zoned(TimeZone::UTC)
            .day_of_year(),
        1
    );

    let year = Timestamp::now().to_zoned(TimeZone::UTC).year();
    for (formatter, width) in [("dateTimeThisDecade", 10), ("dateTimeThisCentury", 100)] {
        let first_year = year.div_euclid(width) * width;
        let latest = format!("{first_year}-01-02T00:00:00+00:00");
        let Value::Text(sample) = date.format(formatter, &[latest.clone().into()])? else {
            panic!("expected a date string");
        };
        let sample = sample.parse::<Timestamp>().unwrap();
        assert_eq!(sample.to_zoned(TimeZone::UTC).year(), first_year);
        assert!(sample <= latest.parse::<Timestamp>().unwrap());
    }
    Ok(())
}

#[test]
fn faker_datetime_version_and_file_extensions_use_seeded_and_real_outputs() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    let date = faker.ext(r"Faker\Extension\DateTimeExtension")?;
    assert_eq!(
        date.format("date", &[Value::Null, 0.into()])?,
        "1970-01-01".into()
    );
    assert_eq!(
        date.format("time", &["H:i:s".into(), 0.into()])?,
        "00:00:00".into()
    );
    assert_eq!(date.format("unixTime", &["@0".into()])?, 0.into());
    assert_eq!(
        date.format("iso8601", &[0.into()])?,
        "1970-01-01T00:00:00+0000".into()
    );
    assert_eq!(date.format("dayOfWeek", &[0.into()])?, "Thursday".into());
    assert_eq!(
        date.format("dateTime", &[0.into(), "Asia/Shanghai".into()])?,
        "1970-01-01T08:00:00+08:00".into()
    );
    assert!(
        date.format("dateTime", &[0.into(), "No/Zone".into()])
            .is_err()
    );

    let version = faker.ext(r"Faker\Extension\VersionExtension")?;
    faker.seed(81);
    let expected = faker.semver(false, false)?;
    faker.seed(81);
    assert_eq!(version.format("semver", &[])?, expected.into());
    let mut seen_prerelease = false;
    let mut seen_build = false;
    for _ in 0..75 {
        let Value::Text(version) = faker.format("semver", &[true.into(), true.into()])? else {
            panic!("version must be text");
        };
        let (main, build) = version
            .split_once('+')
            .map_or((version.as_str(), None), |(main, build)| {
                (main, Some(build))
            });
        let (base, pre) = main
            .split_once('-')
            .map_or((main, None), |(base, pre)| (base, Some(pre)));
        let numbers = base
            .split('.')
            .map(str::parse::<u8>)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(numbers.len(), 3);
        assert!(numbers[0] <= 9 && numbers[1] <= 99 && numbers[2] <= 99);
        if let Some(pre) = pre {
            seen_prerelease = true;
            let (stage, number) = pre
                .split_once('.')
                .map_or((pre, None), |(stage, number)| (stage, Some(number)));
            assert!(matches!(stage, "alpha" | "beta" | "rc"));
            if let Some(number) = number {
                assert!((1..=99).contains(&number.parse::<u8>().unwrap()));
            }
        }
        if let Some(build) = build {
            seen_build = true;
            assert!(
                (build.len() == 7 && build.bytes().all(|c| c.is_ascii_hexdigit()))
                    || (build.len() == 14 && build.bytes().all(|c| c.is_ascii_digit()))
            );
        }
    }
    assert!(seen_prerelease && seen_build);
    assert!(version.format("semver", &["true".into()]).is_err());

    let Value::Text(path) = faker.format(r"Faker\Extension\FileExtension->filePath", &[])? else {
        panic!("file path must be text");
    };
    let typed = faker.file_path().unwrap();
    let metadata = fs::metadata(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::remove_file(&typed).unwrap();
    assert!(metadata.is_file());
    assert_eq!(metadata.len(), 0);
    assert_ne!(Path::new(&path), typed);
    assert!(faker.format("filePath", &[1.into()]).is_err());
    Ok(())
}

#[test]
fn faker_qualified_extensions_resolve_and_compose_with_modifiers() -> DatabaseResult<()> {
    const NUMBER: &str = r"Faker\Extension\NumberExtension";
    const CUSTOM: &str = r"App\Faker\Code";
    const CODE: &str = r"App\Faker\Code->value";
    let faker = Faker::new("en-US")?;
    let previous = faker.get_formatter("numberBetween")?;
    assert_eq!(
        faker
            .ext(NUMBER)?
            .format("NUMBERBETWEEN", &[7.into(), 7.into()])?,
        7.into()
    );
    assert_eq!(
        faker.format(
            r"Faker\Extension\NumberExtension->numberBetween",
            &[8.into(), 8.into()]
        )?,
        8.into()
    );
    assert!(matches!(
        faker.format(r"Faker\Extension\BarcodeExtension->ean13", &[])?,
        Value::Text(code) if code.len() == 13
    ));
    assert!(matches!(
        faker.format(r"Faker\Extension\ColorExtension->rgbColorAsArray", &[])?,
        Value::Json(serde_json::Value::Array(channels)) if channels.len() == 3
    ));
    faker.add_extension(
        NUMBER,
        FakerProvider::new().formatter("numberBetween", |_, _| Ok(12.into())),
    );
    assert_eq!(
        faker.format("numberBetween", &[3.into(), 3.into()])?,
        12.into()
    );
    assert_eq!(previous(&[3.into(), 3.into()])?, 3.into());
    assert_eq!(faker.number_between(3, 3), 3);
    assert!(matches!(
        faker.format("randomDigit", &[]),
        Err(DatabaseError::UnknownFakerFormatter(name)) if name == "randomDigit"
    ));

    faker.add_provider(FakerProvider::new().formatter(CODE, |_, _| Ok("provider".into())));
    assert!(
        matches!(faker.format(CODE, &[]), Err(DatabaseError::UnknownFakerExtension(id)) if id == CUSTOM)
    );
    faker.add_extension(
        CUSTOM,
        FakerProvider::new()
            .formatter("Value", |_, _| Ok("extension".into()))
            .formatter("failure", |_, _| {
                Err(DatabaseError::InvalidQuery("extension failed"))
            }),
    );
    assert_eq!(faker.parse(r"{{ App\Faker\Code->VALUE }}")?, "extension");
    assert!(matches!(
        faker.format(r"app\Faker\Code->value", &[]),
        Err(DatabaseError::UnknownFakerExtension(_))
    ));
    assert!(matches!(
        faker.format(r"App\Faker\Code->missing", &[]),
        Err(DatabaseError::UnknownFakerFormatter(_))
    ));
    assert_eq!(
        faker.unique_with_retries(false, 1).format(CODE, &[])?,
        "extension".into()
    );
    assert!(matches!(
        faker.unique_with_retries(false, 1).format(CODE, &[]),
        Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
    ));
    assert_eq!(
        faker.optional(Some(0.0), Some("fallback".into()), |f| f
            .format("Absent->value", &[]))?,
        Some("fallback".into())
    );
    assert_eq!(
        faker.valid(
            |v| v == &Value::from("extension"),
            Some(1),
            |f| f.format(CODE, &[])
        )?,
        "extension".into()
    );
    assert!(matches!(
        faker.valid(
            |_| true,
            Some(1),
            |f| f.format(r"App\Faker\Code->failure", &[])
        ),
        Err(DatabaseError::InvalidQuery("extension failed"))
    ));
    let isolated = Faker::new("en-US")?;
    assert!(matches!(
        isolated.format(CODE, &[]),
        Err(DatabaseError::UnknownFakerExtension(_))
    ));
    Ok(())
}

#[test]
fn faker_named_modifiers_keep_upstream_retry_and_history_rules() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    faker.add_provider(
        FakerProvider::new().formatter("constant", move |_, arguments| {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok(arguments.first().cloned().unwrap_or_else(|| 7.into()))
        }),
    );
    let unique = faker.unique_with_retries(false, 2);
    assert_eq!(unique.format("constant", &[])?, 7.into());
    assert_eq!(unique.format("constant", &["7".into()])?, "7".into());
    assert_eq!(unique.format("Constant", &[])?, 7.into());
    assert!(matches!(
        unique.format("constant", &[7.into()]),
        Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 6);

    faker.add_provider(FakerProvider::new().formatter("constant", |_, _| Ok(7.into())));
    assert!(unique.format("constant", &[]).is_err());
    assert_eq!(faker.unique(true).format("constant", &[])?, 7.into());
    assert!(matches!(
        unique.format("unknown", &[]),
        Err(DatabaseError::UnknownFakerFormatter(name)) if name == "unknown"
    ));
    assert_eq!(
        faker.optional(Some(0.0), Some(Value::from("skip")), |faker| faker
            .format("unknown", &[]))?,
        Some("skip".into())
    );
    assert!(matches!(
        faker.optional(Some(1.0), None, |faker| faker.format("unknown", &[])),
        Err(DatabaseError::UnknownFakerFormatter(name)) if name == "unknown"
    ));

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    faker.add_provider(FakerProvider::new().formatter("sequence", move |_, _| {
        Ok((counter.fetch_add(1, Ordering::Relaxed) as i64).into())
    }));
    assert_eq!(
        faker.valid(
            |value| value == &Value::Integer(1),
            Some(2),
            |faker| faker.format("sequence", &[])
        )?,
        1.into()
    );
    assert!(matches!(
        faker.valid(|_| false, Some(2), |faker| faker.format("sequence", &[])),
        Err(DatabaseError::InvalidQuery("Faker valid values exhausted"))
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 5);
    Ok(())
}

#[test]
fn faker_named_uniqueness_claims_results_across_threads() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    faker.add_provider(
        FakerProvider::new().formatter("fixed", |_, _| Ok(Value::Json(json!({"ok":true})))),
    );
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| faker.unique_with_retries(false, 1).format("fixed", &[]));
        let second = scope.spawn(|| faker.unique_with_retries(false, 1).format("fixed", &[]));
        (first.join().unwrap(), second.join().unwrap())
    });
    assert_ne!(first.is_ok(), second.is_ok());
    assert!(matches!(first.or(second)?, Value::Json(value) if value == json!({"ok":true})));
    Ok(())
}

static FAKER_SEEDER_GENERATOR: std::sync::Mutex<Option<Arc<Faker>>> = std::sync::Mutex::new(None);

struct FakerApplicationScope;

impl Seeder for FakerApplicationScope {
    fn run<'a>(
        &'a self,
        context: &'a mut SeederContext<'_>,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let default = fake(None)?;
            let alias = fake(Some("ZH-hant"))?;
            let independent = openrust::database::eloquent::factories::Faker::new("zh-TW")?;
            assert!(Arc::ptr_eq(&default, &alias));
            let regional = fake(Some("zh-Hant-TW"))?;
            assert!(!Arc::ptr_eq(&default, &regional));
            assert_eq!(regional.locale(), "zh-Hant-TW");
            assert!(matches!(
                default.format("registeredHere", &[]),
                Err(DatabaseError::UnknownFakerFormatter(_))
            ));
            default.add_provider(
                FakerProvider::new().formatter("registeredHere", |_, _| Ok(21.into())),
            );
            assert_eq!(alias.format("registeredHere", &[])?, Value::from(21));
            assert!(fake(Some("en-US"))?.format("registeredHere", &[]).is_err());
            default.seed(813);
            independent.seed(813);
            assert_eq!(default.address(), independent.address());
            assert_eq!(alias.phone_number(), independent.phone_number());
            fake(Some("en-US"))?.seed(814);
            assert_eq!(default.company(), independent.company());
            let value = |generator: &openrust::database::eloquent::factories::Faker| {
                generator.unique(false).number_between(760, 760)
            };
            assert_eq!(value(&default)?, 760);
            assert!(value(&alias).is_err());
            assert_eq!(value(fake(Some("en-US"))?.as_ref())?, 760);
            let factory = || {
                Factory::<FactoryChild>::try_with_faker(move |generator| {
                    Ok(Attributes::from([(
                        "generated_score".into(),
                        value(generator)?.into(),
                    )]))
                })
            };
            let connection = context.connection();
            connection
                .get_schema_builder()
                .create("seed_factory_children", |table| {
                    table.id();
                    table.integer("generated_score");
                })
                .await?;
            assert!(factory().create_on(connection).await?.state().exists());
            assert!(matches!(
                factory().create_on(connection).await,
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert_eq!(
                connection
                    .table("seed_factory_children")
                    .pluck("generated_score")
                    .await?,
                [760.into()]
            );
            connection
                .get_schema_builder()
                .drop("seed_factory_children")
                .await?;
            *FAKER_SEEDER_GENERATOR.lock().unwrap() = Some(default);
            Ok(())
        })
    }
}

#[tokio::test]
async fn artisan_faker_is_shared_by_application_and_locale_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let args = [
        "db:seed",
        "--database=alternate",
        "--class=FakerApplicationScope",
        "--force",
    ]
    .map(OsString::from);
    let mut previous = None::<(Arc<Faker>, Faker)>;
    // Run in one process so a process-wide Faker cache cannot pass this test.
    for _ in 0..2 {
        let root = Root::new();
        let mut app = Application::configure(root.0.clone())
            .with_config(load)
            .with_providers(vec![Box::new(Provider)])
            .create()
            .unwrap();
        app.boot().await.unwrap();
        // The fixture defaults to SQLite unless the test explicitly installs its MySQL connection.
        let config = app
            .container()
            .get::<dyn openrust::contracts::config::Repository>("config")
            .unwrap();
        config
            .set(
                "database.connections.faker_test",
                json!({"url":root.mysql(&url).url}),
            )
            .unwrap();
        let mut arguments = args.clone();
        arguments[1] = "--database=faker_test".into();
        assert_eq!(app.handle_command(&arguments).await.unwrap(), 0);
        let current = FAKER_SEEDER_GENERATOR.lock().unwrap().take().unwrap();
        if let Some((first, control)) = previous.take() {
            assert!(!Arc::ptr_eq(&current, &first));
            assert_eq!(first.address(), control.address());
        }
        let control = Faker::new("zh-TW").unwrap();
        current.seed(951);
        control.seed(951);
        assert_eq!(current.address(), control.address());
        previous = Some((current, control));
        drop(config);
        root.close().await;
    }
}

#[test]
fn faker_file_copies_bytes_and_preserves_upstream_catalog() {
    verify_faker_file(&Root::new().0).unwrap();
}
fn verify_faker_file(root: &std::path::Path) -> std::io::Result<()> {
    let faker = openrust::database::eloquent::factories::Faker::new("en").unwrap();
    let source = root.join("source");
    let target = root.join("target");
    fs::create_dir_all(source.join("nested"))?;
    fs::create_dir_all(&target)?;
    fs::write(source.join("nested/ignored.txt"), b"ignored")?;
    let result = faker.file(&source, &target, true);
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    fs::write(source.join("sample.tar.gz"), b"\x00\xffsample")?;
    let copied = faker.file(&source, &target, true)?;
    assert_eq!(copied.parent(), Some(target.as_path()));
    assert_eq!(copied.extension().unwrap(), "gz");
    let id = copied.file_stem().unwrap().to_str().unwrap();
    assert_eq!(id.len(), 36);
    assert_eq!(id.as_bytes()[14], b'3');
    assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
    assert_eq!(faker.uuid().get_version_num(), 3);
    assert_eq!(fs::read(&copied)?, b"\x00\xffsample");
    let basename = faker.file(&source, &target, false)?;
    assert_eq!(basename.components().count(), 1);
    assert_ne!(copied, target.join(&basename));
    assert_eq!(fs::read(target.join(basename))?, b"\x00\xffsample");
    assert!(faker.file(&source, &root.join("missing"), false).is_err());
    assert!(
        faker
            .file(&source, &source.join("nested/.."), false)
            .is_err()
    );
    assert!(
        faker
            .file(&source.join("sample.tar.gz"), &target, true)
            .is_err()
    );
    let unique = faker.unique(true);
    let types = (0..461)
        .map(|_| unique.mime_type().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(types.len(), 461);
    assert!(types.contains("application/json"));
    assert!(unique.mime_type().is_err());
    let extension = unique.file_extension().unwrap();
    assert!(!extension.is_empty() && !extension.starts_with('.'));
    let id = unique.uuid().unwrap();
    assert_ne!(unique.uuid().unwrap(), id);
    Ok(())
}

#[test]
fn faker_date_formats_report_errors_through_public_consumers() -> DatabaseResult<()> {
    let faker = openrust::database::eloquent::factories::Faker::new("en")?;
    let epoch = jiff::Timestamp::UNIX_EPOCH;
    assert_eq!(faker.date(Some("%Y-%m-%d"), Some(epoch))?, "1970-01-01");
    assert_eq!(faker.time(Some("%H:%M:%S"), Some(epoch))?, "00:00:00");
    for format in ["%", "%J"] {
        assert!(faker.date(Some(format), Some(epoch)).is_err());
        assert!(faker.time(Some(format), Some(epoch)).is_err());
        assert!(faker.unique(true).date(Some(format), Some(epoch)).is_err());
        assert!(
            faker
                .credit_card_expiration_date_string(true, Some(format))
                .is_err()
        );
    }
    assert_eq!(faker.unique(true).date(Some("%Y"), Some(epoch))?, "1970");
    assert!(
        jiff::civil::Date::strptime(
            "%Y-%m-%d",
            faker.credit_card_expiration_date_string(true, Some("%Y-%m-%d"))?
        )
        .is_ok()
    );
    Ok(())
}
