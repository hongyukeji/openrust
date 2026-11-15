use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        Arc,
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
    support::{ProviderResult, ServiceProvider, facades::DB},
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
                .getSchemaBuilder()
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
        Box::pin(async { context.getSchemaBuilder().dropIfExists("seed_items").await })
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
                .callOnce(&["Database\\Seeders\\FirstSeeder"])
                .await?;
            context.callSilent(&["SilentSeeder"]).await?;
            context
                .callWith(
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
                .callWithMany(&["FirstSeeder", "SecondSeeder"], params("shared"))
                .await?;
            context
                .callSilentWith(&["SilentSeeder"], params("quiet"))
                .await?;
            context
                .callOnceWith(
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
                    .callOnceWith(&["RetrySeeder", "UnreachedSeeder"], true, params("fail"))
                    .await
                    .is_err()
            );
            context
                .callOnceWith(&["RetrySeeder"], false, params("recovered"))
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
                DB::connection(None)?.getName(),
                context.connection().getName()
            );
            let attributes = Attributes::from([("name".into(), name)]);
            let guarded = attributes.clone();
            tokio::spawn(async move {
                assert!(matches!(
                    SeedItem::newInstance(guarded),
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
                DB::connection(None)?.getName(),
                context.connection().getName()
            );
            SeedItem::newInstance(Attributes::from([("name".into(), "unsaved".into())]))?;
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

            fn namedRelations() -> NamedRelations<Self> {
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
    NamedRelations::default().morphTo("owner", || {
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
            .uniqueId(UniqueId::Uuid)
            .uniqueIds(&["external_id"]);
        &METADATA
    }

    fn from_state(state: ModelState) -> Self {
        let mut model = Self(state);
        model.setAttribute("label", "model-default");
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
            if model.getAttribute("generated_name") == Some(&"quiet-fixture".into()) {
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
            .parentAttribute("owner_id", Factory::<FactoryOwner>::new(Attributes::new))
    };
    let omitted = factory()
        .count(2)
        .withoutParents(vec![])
        .rawOn(connection)
        .await?;
    assert!(
        omitted
            .iter()
            .all(|attributes| attributes["owner_id"] == Value::Null)
    );
    let selected = factory()
        .withoutParents(vec![FactoryParentSelector::model::<FactoryOwner>()])
        .rawOn(connection)
        .await?;
    assert_eq!(selected["owner_id"], Value::Null);
    let selected = factory()
        .withoutParents(vec![FactoryParentSelector::attribute("owner_id")])
        .rawOn(connection)
        .await?;
    assert_eq!(selected["owner_id"], Value::Null);
    let explicit = factory()
        .makeOneOn(
            connection,
            Attributes::from([("owner_id".into(), 99_i64.into())]),
        )
        .await?;
    assert_eq!(explicit.getAttribute("owner_id"), Some(&99_i64.into()));
    assert_eq!(connection.table("seed_factory_owners").count("*").await?, 0);

    let generated = factory().count(2).createOn(connection).await?;
    assert_ne!(
        generated[0].getAttribute("owner_id"),
        generated[1].getAttribute("owner_id")
    );
    let owner = Factory::<FactoryOwner>::new(Attributes::new)
        .createOn(connection)
        .await?;
    let key = owner.getKey().unwrap().clone();
    let recycled = factory().recycle(owner).rawOn(connection).await?;
    assert_eq!(recycled["owner_id"], key);
    let relation = MorphTo::<FactoryChild>::new("owner_type", "owner_id")
        .target::<FactoryOwner>("seed-owner", "id");
    let explicit_relation = Factory::<FactoryChild>::new(Attributes::new)
        .r#for(relation, Factory::<FactoryOwner>::new(Attributes::new))
        .withoutParents(vec![])
        .rawOn(connection)
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
        .createOn(connection)
        .await?;
    let existing_keys = existing.modelKeys();
    let single = children().createOn(connection).await?;
    let single_key = single.getKey().unwrap().clone();
    owners()
        .hasAttachedNamed::<_, MorphToMany<FactoryOwner, FactoryChild>>(
            "attached",
            existing,
            vec![pivot("existing-first"), pivot("existing-second")],
        )
        .hasAttachedNamed::<_, MorphToMany<FactoryOwner, FactoryChild>>(
            "attached",
            single,
            pivot("existing-single"),
        )
        .createOn(connection)
        .await?;
    let links = connection
        .table("seed_factory_links")
        .orderBy("child_id", "asc")
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
    let invalid = owners().hasAttachedNamed::<_, MorphToMany<FactoryOwner, FactoryChild>>(
        "attached",
        children().makeOn(connection).await?,
        pivot("invalid"),
    );
    assert!(matches!(
        invalid.createOn(connection).await,
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
    let remote = children().createOn(sqlite).await?;
    let invalid = owners().hasAttachedNamed::<_, MorphToMany<FactoryOwner, FactoryChild>>(
        "attached",
        remote,
        pivot("invalid"),
    );
    assert!(matches!(
        invalid.createOn(connection).await,
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
            assert_eq!(connection.getName(), "alternate");
            let sqlite = DB::connection(Some("sqlite"))?;
            sqlite
                .statement(
                    "CREATE TABLE seed_factory_owners (id INTEGER PRIMARY KEY)",
                    &[],
                )
                .await?;
            let factory = Factory::<FactoryOwner>::new(Attributes::new).connection(Some("sqlite"));
            assert_eq!(factory.getConnectionName(), Some("sqlite"));
            assert_eq!(factory.create().await?.getConnectionName(), Some("sqlite"));
            assert_eq!(
                factory.createOn(connection).await?.getConnectionName(),
                Some("alternate")
            );
            let counted = factory.count(2);
            assert_eq!(counted.getConnectionName(), Some("sqlite"));
            let deferred = counted.lazy(Attributes::new());
            assert!(
                deferred()
                    .await?
                    .iter()
                    .all(|model| model.getConnectionName() == Some("sqlite"))
            );
            drop(deferred);
            assert_eq!(sqlite.table("seed_factory_owners").count("*").await?, 3);
            assert_eq!(connection.table("seed_factory_owners").delete().await?, 1);
            let counted = counted.connection(None);
            assert_eq!(counted.getConnectionName(), None);
            assert!(
                counted
                    .make()
                    .await?
                    .iter()
                    .all(|model| model.getConnectionName() == Some("alternate"))
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
                nested.rawOn(connection).await,
                Err(DatabaseError::UnknownConnection(name)) if name == "missing"
            ));
            let nested = Factory::<FactoryChild>::new(Attributes::new).r#for(
                owner_relation(),
                Factory::<FactoryOwner>::new(Attributes::new).connection(Some("alternate")),
            );
            assert!(matches!(
                nested.rawOn(connection).await?.get("owner_id"),
                Some(Value::Integer(_) | Value::Unsigned(_))
            ));
            assert_eq!(connection.table("seed_factory_owners").delete().await?, 1);
            let query = || {
                BulkSeed::queryOn(connection).withAttributes(
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
            let rows = BulkSeed::queryOn(connection)
                .fillForInsert(vec![Attributes::new()])
                .await?;
            assert_eq!(rows[0]["label"], "model-default".into());
            let rows = query()
                .fillForInsert(vec![Attributes::from([
                    ("score".into(), "42".into()),
                    ("options".into(), true.into()),
                ])])
                .await?;
            assert_eq!(rows[0]["label"], "query-default".into());
            assert_eq!(rows[0]["score"], "42".into());
            assert_eq!(rows[0]["options"], Value::Json(json!(true)));
            assert!(
                matches!(&rows[0]["external_id"], Value::Text(id) if BulkSeed::isValidUniqueId(id))
            );
            assert_eq!(rows[0]["created_at"], rows[0]["updated_at"]);
            assert!(query().fillAndInsert(vec![]).await?);
            let mut explicit = record(" BULK-A ");
            explicit.insert("created_at".into(), "2001-01-01 00:00:00".into());
            assert!(
                query()
                    .fillAndInsert(vec![explicit, record("bulk-b")])
                    .await?
            );
            assert_eq!(
                query()
                    .fillAndInsertOrIgnore(vec![record("bulk-a"), record("bulk-c")])
                    .await?,
                1
            );
            let id = query().fillAndInsertGetId(record("bulk-d")).await?;
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
                .insertOn(connection, Attributes::new())
                .await?;
            bulk_factory().insert(Attributes::new()).await?;
            bulk_factory()
                .sequence(|index| {
                    Attributes::from([("label".into(), format!("factory-{index}").into())])
                })
                .afterMaking(|model| {
                    model.setAttribute("score", 52_i64);
                    Ok(())
                })
                .has(
                    openrust::database::eloquent::relations::HasMany::new("id", "owner_id"),
                    Factory::<FactoryChild>::new(Attributes::new)
                        .afterMaking(|_| Err(DatabaseError::InvalidQuery("bulk created a child")))
                        .count(1),
                )
                .afterCreating(|_| Err(DatabaseError::InvalidQuery("bulk ran afterCreating")))
                .count(2)
                .insertOn(connection, Attributes::new())
                .await?;
            let rows = connection
                .table("seed_bulk")
                .r#where("label", "like", "factory-%")
                .orderBy("label", "asc")
                .get()
                .await?;
            assert_eq!(rows.len(), 3);
            assert_eq!(rows[0].get("score"), Some(&52_i64.into()));
            assert_eq!(rows[2].get("score"), Some(&51_i64.into()));
            assert!(rows.iter().all(|row| matches!(row.get("external_id"),
                Some(Value::Text(id)) if BulkSeed::isValidUniqueId(id))));
            assert!(
                rows.iter()
                    .all(|row| row.get("created_at") == row.get("updated_at"))
            );
            assert!(
                bulk_factory()
                    .count(2)
                    .insertOn(
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
                .afterMaking(|_| Err(DatabaseError::InvalidQuery("raw invoked callback")))
            };
            let records = quiet()
                .sequence(|index| Attributes::from([("index".into(), index.to_string().into())]))
                .count(2)
                .rawOn(connection)
                .await?;
            assert_eq!(records[0]["generated_name"], "quiet-fixture".into());
            assert_eq!(records[1]["index"], "1".into());
            assert!(quiet().count(0).rawOn(connection).await?.is_empty());
            let expanded = quiet()
                .r#for(
                    MorphTo::<FactoryChild>::new("owner_type", "owner_id")
                        .target::<FactoryOwner>("seed-owner", "id"),
                    Factory::<FactoryOwner>::new(Attributes::new),
                )
                .count(2)
                .rawOn(connection)
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
            .afterCreating({
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
            factory.createQuietly().await?;
            let batch = factory.count(2);
            assert_eq!(batch.createQuietlyOn(connection).await?.len(), 2);
            assert_eq!(callbacks.load(Ordering::Relaxed), 3);
            assert!(batch.createOn(connection).await.is_err());
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
                .afterMaking(record_phase("making"))
                .afterCreating(record_phase("created"))
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
                .prependState(|attributes| {
                    assert_eq!(attributes["generated_score"], 40_i64.into());
                    Attributes::from([("generated_score".into(), 41_i64.into())])
                })
                .count(2)
                .prependState(|attributes| {
                    assert_eq!(attributes["generated_name"], "definition".into());
                    Attributes::from([("generated_score".into(), 40_i64.into())])
                })
                .set("generated_name", "batch-set")
                .state(|attributes| {
                    assert_eq!(attributes["generated_name"], "batch-set".into());
                    assert_eq!(attributes["generated_score"], 42_i64.into());
                    Attributes::new()
                });
            let raw = configured.rawOn(connection).await?;
            assert_eq!(raw.len(), 2);
            assert_eq!(raw[0]["generated_name"], "batch-set".into());
            let rows = configured
                .createManyOn(
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
                .orderBy("id", "asc")
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
                .afterMaking(|_| Err(DatabaseError::InvalidQuery("removed making callback")))
                .withoutAfterMaking()
                .afterMaking(|model| {
                    model.setAttribute("generated_name", "callback-readded");
                    Ok(())
                })
                .afterCreating(|model| {
                    assert!(model.state().exists());
                    assert_eq!(
                        model.getAttribute("generated_name"),
                        Some(&"callback-readded".into())
                    );
                    model.setAttribute("generated_name", "callback-created");
                    Ok(())
                })
                .createOn(connection)
                .await?;
            assert_eq!(
                created.getAttribute("generated_name"),
                Some(&"callback-created".into())
            );
            let created = Factory::<FactoryChild>::new(Attributes::new)
                .afterMaking(|model| {
                    model.setAttribute("generated_name", "callback-batch");
                    Ok(())
                })
                .afterCreating(|_| Err(DatabaseError::InvalidQuery("removed creating callback")))
                .count(2)
                .withoutAfterCreating()
                .createOn(connection)
                .await?;
            assert_eq!(created.len(), 2);
            assert!(created.iter().all(
                |model| model.getAttribute("generated_name") == Some(&"callback-batch".into())
            ));
            assert_eq!(connection.table("seed_factory_children").delete().await?, 3);
            assert!(matches!(
                Factory::<FactoryChild>::new(move || overrides("quiet-fixture"))
                    .count(1)
                    .withoutAfterMaking()
                    .withoutAfterCreating()
                    .createOn(connection)
                    .await,
                Err(DatabaseError::InvalidQuery(
                    "factory model creation cancelled"
                ))
            ));
            let preview = batch
                .makeOneOn(connection, overrides("preview-one"))
                .await?;
            assert!(!preview.state().exists());
            assert_eq!(
                preview.getAttribute("generated_name"),
                Some(&"preview-one".into())
            );
            let previews = batch.makeMany(FactoryRecords::Default).await?;
            assert_eq!(previews.len(), 3);
            assert!(previews.iter().all(|model| !model.state().exists()));
            assert_eq!(
                previews[2].getAttribute("generated_name"),
                Some(&"many-3".into())
            );
            let previews = batch
                .makeManyOn(
                    connection,
                    FactoryRecords::Attributes(vec![overrides("preview-many")]),
                )
                .await?;
            assert_eq!(previews.len(), 1);
            assert_eq!(
                previews[0].getAttribute("generated_name"),
                Some(&"preview-many".into())
            );
            assert!(
                batch
                    .makeManyOn(connection, FactoryRecords::Count(0))
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
                |model| model.getAttribute("generated_name") == Some(&"deferred-batch".into())
            ));
            assert!(deferred_one().await?.state().exists());
            assert!(deferred_one().await?.state().exists());
            assert_eq!(connection.table("seed_factory_children").delete().await?, 3);
            let each = Factory::<FactoryChild>::new(Attributes::new)
                .forEachSequence(vec![overrides("sequence-one"), overrides("sequence-two")])
                .createOn(connection)
                .await?;
            assert_eq!(each.len(), 2);
            assert_eq!(
                each[1].getAttribute("generated_name"),
                Some(&"sequence-two".into())
            );
            assert!(
                Factory::<FactoryChild>::new(Attributes::new)
                    .forEachSequence(vec![])
                    .createOn(connection)
                    .await?
                    .is_empty()
            );
            let score = |value: i64| Attributes::from([("generated_score".into(), value.into())]);
            let crossed = Factory::<FactoryChild>::new(Attributes::new)
                .crossJoinSequence(vec![
                    vec![overrides("cross-a"), overrides("cross-b")],
                    vec![score(31), score(32)],
                    vec![score(40), score(41)],
                ])?
                .count(9)
                .createOn(connection)
                .await?;
            let order: Vec<_> = crossed
                .iter()
                .map(|model| {
                    (
                        model.getAttribute("generated_name").cloned(),
                        model.getAttribute("generated_score").cloned(),
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
                Factory::<FactoryChild>::new(Attributes::new).crossJoinSequence(vec![vec![]]),
                Err(DatabaseError::InvalidQuery(
                    "factory cross join sequence is empty"
                ))
            ));
            assert_eq!(
                connection.table("seed_factory_children").delete().await?,
                11
            );
            let one = batch.createOneOn(connection, overrides("many-one")).await?;
            assert_eq!(one.getAttribute("generated_name"), Some(&"many-one".into()));
            phases.lock().unwrap().clear();
            let defaults = batch.createMany(FactoryRecords::Default).await?;
            assert_eq!(defaults.len(), 1);
            assert_eq!(*phases.lock().unwrap(), ["making", "created"]);
            assert_eq!(
                defaults[0].getAttribute("generated_name"),
                Some(&"many-7".into())
            );
            let counted = batch
                .createManyOn(connection, FactoryRecords::Count(1))
                .await?;
            assert_eq!(counted.len(), 1);
            assert!(
                batch
                    .createManyOn(connection, FactoryRecords::Count(0))
                    .await?
                    .is_empty()
            );
            assert!(matches!(
                batch
                    .createManyOn(
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
            batch.createOneQuietly(overrides("quiet-fixture")).await?;
            let quiet = batch
                .createManyQuietlyOn(
                    connection,
                    FactoryRecords::Attributes(vec![
                        overrides("quiet-fixture"),
                        overrides("many-quiet"),
                    ]),
                )
                .await?;
            assert_eq!(quiet.len(), 2);
            assert_eq!(
                quiet[1].getAttribute("generated_name"),
                Some(&"many-quiet".into())
            );
            assert_eq!(connection.table("seed_factory_children").delete().await?, 7);
            assert_eq!(fake(None)?.locale(), "zh_TW");
            assert_eq!(fake(Some("fr-fr"))?.locale(), "fr_FR");
            let invalid = fake(Some("../en"));
            assert!(matches!(
                invalid,
                Err(DatabaseError::InvalidConfiguration(_))
            ));
            let epoch = chrono::DateTime::UNIX_EPOCH;
            let taipei = "Asia/Taipei".parse().unwrap();
            let new_york = "America/New_York".parse().unwrap();
            let utc = "UTC".parse().unwrap();
            let faker = fake(None)?;
            let other = fake(Some("fr-fr"))?;
            assert_eq!(faker.getDefaultTimezone(), None);
            assert_eq!(faker.iso8601(Some(epoch)), "1970-01-01T08:00:00+0800");
            assert_eq!(
                other.dateTimeInTimezone(Some(epoch), None).timezone(),
                taipei
            );
            other.setDefaultTimezone(Some(new_york));
            assert_eq!(other.getDefaultTimezone(), Some(new_york));
            assert_eq!(other.date(None, Some(epoch))?, "1969-12-31");
            assert_eq!(other.year(Some(epoch)), "1969");
            assert_eq!(other.dayOfWeek(Some(epoch)), "Wednesday");
            assert_eq!(
                other
                    .unique(false)
                    .dateTimeInTimezone(Some(epoch), Some(utc))?
                    .timezone(),
                utc
            );
            assert_eq!(
                faker.dateTimeInTimezone(Some(epoch), None).timezone(),
                taipei
            );
            other.setDefaultTimezone(None);
            assert_eq!(
                other.dateTimeInTimezone(Some(epoch), None).timezone(),
                taipei
            );
            assert_eq!(
                openrust::database::eloquent::factories::Faker::new("en")?
                    .dateTimeInTimezone(Some(epoch), None)
                    .timezone(),
                utc
            );
            faker.setDefaultTimezone(Some(utc));
            assert_eq!(fake(Some("zh_TW"))?.getDefaultTimezone(), Some(utc));
            for value in [
                faker.firstName(),
                faker.lastName(),
                faker.userName(),
                faker.streetName(),
                faker.postcode(),
                faker.phoneNumber(),
                faker.company(),
                faker.word(),
            ] {
                assert!(!value.trim().is_empty());
            }
            assert!(faker.freeEmail().contains('@'));

            let start = chrono::DateTime::from_timestamp(-1, 0).unwrap();
            let end = chrono::DateTime::from_timestamp(1, 0).unwrap();
            let max = Some(faker.resolveDateMax("0")?);
            assert_eq!(faker.unixTime(max), 0);
            assert_eq!(faker.dateTime(max), epoch);
            assert_eq!(faker.date(None, max)?, "1970-01-01");
            assert_eq!(faker.time(None, max)?, "00:00:00");
            assert_eq!(
                faker.unique(false).iso8601(max)?,
                "1970-01-01T00:00:00+0000"
            );
            assert!(faker.unique(false).iso8601(Some(epoch)).is_err());
            faker.unique(true);
            assert_eq!(faker.resolveDateMax(" -1.9 ")?.timestamp(), -1);
            assert_eq!(faker.resolveDateMax("1e1")?.timestamp(), 10);
            assert_eq!(
                faker.resolveDateMax("2024-01-02 12:00")?.timestamp(),
                1_704_168_000
            );
            assert_eq!(
                faker.resolveDateMax("2024-01-02 12:00 UTC")?.timestamp(),
                1_704_196_800
            );
            let before = chrono::Utc::now();
            let resolved = faker.resolveDateMax("")?;
            assert!(
                (before.timestamp()..=chrono::Utc::now().timestamp())
                    .contains(&resolved.timestamp())
            );
            assert!(matches!(
                faker.resolveDateMax("bad date"),
                Err(DatabaseError::InvalidQuery("invalid Faker date expression"))
            ));
            assert!(faker.resolveDateMax(" ").is_err());
            assert_eq!(faker.resolveDateMax("\u{b}1e999\u{c}")?, epoch);
            assert_eq!(faker.resolveDateMax("-1e999")?, epoch);
            assert_eq!(faker.unique(false).unixTimeExpression("1e999")?, 0);
            assert!(faker.unique(false).unixTime(Some(epoch)).is_err());
            faker.unique(true);
            assert!(faker.unixTimeExpression("+1000000-01-01")? >= 0);
            assert!(faker.unixTimeExpression("@-9223372036854775808")? <= 0);
            assert!(faker.unixTimeExpression("9223372036854775808")? >= 0);

            assert!(matches!(
                faker.resolveDateMax("1e100"),
                Err(DatabaseError::InvalidQuery(
                    "Faker timestamp is out of range"
                ))
            ));
            assert_eq!(faker.unixTime(Some(epoch)), 0);
            assert!((-1..=0).contains(&faker.unixTime(Some(start))));
            assert!((0..=chrono::Utc::now().timestamp()).contains(&faker.unixTime(None)));
            assert_eq!(faker.dateTime(Some(epoch)), epoch);
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
            let first_ad = chrono::DateTime::from_timestamp(-62_135_597_361, 0).unwrap();
            assert_eq!(faker.dateTimeAD(Some(first_ad)), first_ad);
            assert_eq!(faker.amPm(Some(epoch)), "上午");
            assert_eq!(faker.dayOfMonth(Some(epoch)), "01");
            assert_eq!(faker.dayOfWeek(Some(epoch)), "星期四");
            assert_eq!(faker.month(Some(epoch)), "01");
            assert_eq!(faker.monthName(Some(epoch)), "一月");
            assert_eq!(faker.year(Some(epoch)), "1970");
            let calendar_unique = faker.unique(false);
            assert_eq!(calendar_unique.dateTimeAD(Some(first_ad))?, first_ad);
            assert_eq!(calendar_unique.amPm(Some(epoch))?, "上午");
            assert_eq!(calendar_unique.dayOfMonth(Some(epoch))?, "01");
            assert_eq!(calendar_unique.dayOfWeek(Some(epoch))?, "星期四");
            assert_eq!(calendar_unique.month(Some(epoch))?, "01");
            assert_eq!(calendar_unique.monthName(Some(epoch))?, "一月");
            assert_eq!(calendar_unique.year(Some(epoch))?, "1970");
            assert!(calendar_unique.amPm(Some(epoch)).is_err());
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
                faker.date(Some("%Q"), Some(epoch)),
                Err(DatabaseError::InvalidQuery("invalid Faker date format"))
            ));
            let unique = faker.unique(false);
            assert_eq!(unique.unixTime(Some(epoch))?, 0);
            assert_eq!(unique.dateTime(Some(epoch))?, epoch);
            assert_eq!(unique.date(None, Some(epoch))?, "1970-01-01");
            assert_eq!(unique.time(None, Some(epoch))?, "00:00:00");
            assert_eq!(unique.iso8601(Some(epoch))?, "1970-01-01T00:00:00+0000");
            assert!(matches!(
                unique.iso8601(Some(epoch)),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            for _ in 0..20 {
                assert!((-2..=3).contains(&faker.numberBetween(3, -2)));
                assert!(faker.randomDigit() <= 9);
                assert!((1..=9).contains(&faker.randomDigitNotNull()));
                assert!((start..=end).contains(&faker.dateTimeBetween(start, end)?));
            }
            assert!(matches!(
                faker.unique(false).dateTimeBetween(end, start),
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
                let expected = chrono::DateTime::parse_from_rfc3339(expected).unwrap();
                let upper = expected.timestamp().to_string();
                assert_eq!(
                    faker.dateTimeBetweenExpressions(expression, &upper)?,
                    expected
                );
            }
            assert_eq!(faker.unixTimeExpression("bad date")?, 0);
            assert_eq!(faker.dateTimeBetweenExpressions("bad date", "0")?, epoch);
            assert_eq!(faker.dateTimeBetweenExpressions("@0", "bad date")?, epoch);
            assert!(matches!(
                faker.dateTimeBetweenExpressions("@1", "bad date"),
                Err(DatabaseError::InvalidQuery(
                    "Faker start date exceeds end date"
                ))
            ));
            assert!(
                faker
                    .dateTimeInIntervalExpressions(Some("bad date"), Some("1 day"))
                    .is_err()
            );
            assert!(matches!(
                faker.unixTimeExpression(&" ".repeat(20 * 1024 * 1024 + 1)),
                Err(DatabaseError::InvalidQuery(
                    "Faker date expression exceeds parser limits"
                ))
            ));
            assert!(matches!(
                faker.dateTimeBetweenExpressions("tomorrow", "today"),
                Err(DatabaseError::InvalidQuery(
                    "Faker start date exceeds end date"
                ))
            ));
            assert_eq!(faker.unique(false).dateTimeBetween(start, start)?, start);
            assert!(matches!(
                faker.unique(false).dateTimeBetweenExpressions("@-1", "-1"),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            let expression_interval = faker.dateTimeInIntervalExpressions(
                Some("2024-03-09 12:00 America/New_York"),
                Some("+1 day"),
            )?;
            let spring_start =
                chrono::DateTime::parse_from_rfc3339("2024-03-09T17:00:00Z").unwrap();
            let spring_end = chrono::DateTime::parse_from_rfc3339("2024-03-10T16:00:00Z").unwrap();
            assert!(
                (spring_start.timestamp()..=spring_end.timestamp())
                    .contains(&expression_interval.timestamp())
            );
            assert_eq!(
                faker
                    .unique(false)
                    .dateTimeInIntervalExpressions(Some("@0"), Some("today"))?,
                epoch
            );
            assert!(matches!(
                faker
                    .unique(false)
                    .dateTimeInInterval(Some(epoch), Some(chrono::TimeDelta::zero())),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            faker.unique(true);
            assert!(matches!(
                faker.dateTimeInIntervalExpressions(Some("now"), Some("1 day UTC")),
                Err(DatabaseError::InvalidQuery(
                    "invalid Faker date or interval expression"
                ))
            ));
            let second = chrono::TimeDelta::seconds(1);
            for interval in [second, -second] {
                let other = epoch + interval;
                let sampled = faker.dateTimeInInterval(Some(epoch), Some(interval))?;
                assert!((epoch.min(other)..=epoch.max(other)).contains(&sampled));
            }
            assert!(matches!(
                faker.dateTimeInInterval(
                    Some(chrono::DateTime::<chrono::Utc>::MAX_UTC),
                    Some(second)
                ),
                Err(DatabaseError::InvalidQuery(
                    "Faker timestamp is out of range"
                ))
            ));
            assert_eq!(
                unique.dateTimeInInterval(Some(epoch), Some(chrono::TimeDelta::zero()))?,
                epoch
            );
            assert!(matches!(
                unique.dateTimeInInterval(Some(epoch), Some(chrono::TimeDelta::zero())),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            for date in [
                unique.dateTimeThisCentury(Some(epoch))?,
                unique.dateTimeThisDecade(None)?,
                unique.dateTimeThisYear(None)?,
                unique.dateTimeThisMonth(None)?,
                faker.dateTimeInInterval(None, None)?,
            ] {
                assert!(date <= chrono::Utc::now());
                assert_eq!(date.timestamp_subsec_nanos(), 0);
            }
            let year_start = chrono::NaiveDate::from_yo_opt(
                chrono::Datelike::year(&chrono::Utc::now().with_timezone(&taipei)),
                1,
            )
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
                - chrono::TimeDelta::hours(8);
            assert_eq!(faker.dateTimeThisYear(Some(year_start))?, year_start);
            for date in [
                faker.dateTimeThisCentury(Some(chrono::DateTime::<chrono::Utc>::MIN_UTC)),
                faker.dateTimeThisDecade(Some(epoch)),
                faker.dateTimeThisYear(Some(epoch)),
                faker.dateTimeThisMonth(Some(epoch)),
            ] {
                assert!(matches!(
                    date,
                    Err(DatabaseError::InvalidQuery(
                        "Faker start date exceeds end date"
                    ))
                ));
            }
            assert_eq!(
                faker.unique(false).numberBetween(i64::MIN, i64::MIN)?,
                i64::MIN
            );
            assert_eq!(
                faker.unique(false).numberBetween(i64::MAX, i64::MAX)?,
                i64::MAX
            );
            assert!(matches!(
                faker.unique(false).numberBetween(i64::MIN, i64::MIN),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert!(faker.unique(false).randomDigit()? <= 9);
            assert!((1..=9).contains(&faker.unique(false).randomDigitNotNull()?));
            assert_ne!(faker.unique(false).randomDigitNot(4)?, 4);
            assert_eq!(faker.randomFloat(Some(2), 1.005, Some(1.005))?, 1.01);
            assert_eq!(faker.randomFloat(Some(2), -1.005, Some(-1.005))?, -1.01);
            assert_eq!(faker.randomFloat(Some(-1), -25.0, Some(-25.0))?, -30.0);
            assert_eq!(
                faker.randomFloat(Some(0), 1_000_000_000.0, None)?,
                1_000_000_000.0
            );
            for _ in 0..20 {
                assert!((-2.0..=2.0).contains(&faker.randomFloat(None, 2.0, Some(-2.0))?));
            }
            assert!(faker.randomFloat(Some(2), f64::NAN, None).is_err());
            assert_eq!(
                faker.unique(false).randomFloat(Some(0), -0.0, Some(-0.0))?,
                0.0
            );
            assert!(matches!(
                faker.unique(false).randomFloat(Some(0), 0.0, Some(0.0)),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert!((100_000_000..=999_999_999).contains(&faker.randomNumber(Some(9), true)?));
            assert!((0..=999_999_999).contains(&faker.randomNumber(None, false)?));
            assert!(matches!(
                faker.randomNumber(Some(10), false),
                Err(DatabaseError::InvalidQuery(
                    "Faker random number exceeds digit limit"
                ))
            ));
            assert!(matches!(
                faker.randomNumber(Some(0), true),
                Err(DatabaseError::InvalidQuery(
                    "Faker strict digit count is zero"
                ))
            ));
            assert_eq!(faker.unique(false).randomNumber(Some(0), false)?, 0);
            assert!(matches!(
                faker.unique(false).randomNumber(Some(0), false),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));

            assert_eq!(faker.unique(false).sentence(0..1)?, ".");
            assert!(matches!(
                faker.unique(false).sentence(0..1),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert_eq!(faker.unique(true).sentence(0..1)?, ".");

            let exhausted = Factory::<FactoryChild>::tryWithFaker(|faker| {
                Ok(Attributes::from([(
                    "generated_name".into(),
                    faker.unique(false).sentence(0..1)?.into(),
                )]))
            })
            .count(2);
            for _ in 0..2 {
                assert!(matches!(
                    exhausted.createOn(connection).await,
                    Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
                ));
                assert_eq!(
                    connection.table("seed_factory_children").count("*").await?,
                    0
                );
            }
            let same_locale = fake(Some("zh-Hant-TW"))?;
            assert!(Arc::ptr_eq(&faker, &same_locale));
            assert!(!Arc::ptr_eq(&faker, &other));
            assert!(matches!(
                same_locale.unique(false).sentence(0..1),
                Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
            ));
            assert_eq!(other.unique(true).sentence(0..1)?, ".");
            faker.unique(true);
            faker.setDefaultTimezone(None);
            let owners = || Factory::<FactoryOwner>::new(Attributes::new);
            let children = || Factory::<FactoryChild>::new(Attributes::new);
            let pivot = |label: &str| Attributes::from([("label".into(), label.into())]);
            let pivot_calls = Arc::new(AtomicUsize::new(0));
            owners()
                .hasAttachedNamed::<_, MorphToMany<FactoryOwner, FactoryChild>>(
                    "attached",
                    children().count(3),
                    vec![pivot("first"), pivot("second")],
                )
                .hasAttachedNamed::<_, MorphToMany<FactoryOwner, FactoryChild>>(
                    "attached",
                    children().count(2),
                    FactoryPivot::resolve({
                        let pivot_calls = Arc::clone(&pivot_calls);
                        move |owner: &FactoryOwner| {
                            assert!(owner.state().exists());
                            assert!(owner.getKey().is_some());
                            let index = pivot_calls.fetch_add(1, Ordering::Relaxed);
                            Ok(Attributes::from([(
                                "label".into(),
                                format!("resolved-{index}").into(),
                            )]))
                        }
                    }),
                )
                .createOn(connection)
                .await?;
            let labels = connection
                .table("seed_factory_links")
                .orderBy("child_id", "asc")
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
            let localized = Factory::<FactoryChild>::tryWithFaker(move |faker| {
                assert_eq!(faker.locale(), "zh_TW");
                assert_eq!(
                    faker.dateTimeInTimezone(Some(epoch), None).timezone(),
                    taipei
                );
                let name = faker.unique(false).firstName()?;
                assert!(
                    name.chars()
                        .any(|character| ('\u{4e00}'..='\u{9fff}').contains(&character))
                );
                Ok(Attributes::from([
                    ("generated_name".into(), name.into()),
                    (
                        "generated_score".into(),
                        faker.unique(false).numberBetween(10, 11)?.into(),
                    ),
                    (
                        "generated_at".into(),
                        faker.dateTimeBetween(start, start)?.to_rfc3339().into(),
                    ),
                ]))
            })
            .count(2)
            .createOn(connection)
            .await?;
            assert!(localized.iter().all(|model| model.state().exists()));
            assert_ne!(
                localized[0].getAttribute("generated_name"),
                localized[1].getAttribute("generated_name")
            );
            let subject = || {
                MorphTo::<FactoryChild>::new("owner_type", "owner_id")
                    .target::<FactoryOwner>("seed-owner", "id")
            };
            for name in ["missing", "owner"] {
                let invalid = children().forNamed::<FactoryOwner, openrust::database::eloquent::relations::BelongsTo<FactoryChild, FactoryOwner>>(name, owners());
                assert!(matches!(
                    invalid.makeOn(connection).await,
                    Err(DatabaseError::InvalidQuery(_))
                ));
            }
            assert_eq!(connection.table("seed_factory_owners").count("*").await?, 0);
            let parented = children()
                .forNamed::<_, MorphTo<FactoryChild>>("owner", owners())
                .count(2);
            let first = parented.createOn(connection).await?;
            let second = parented.createOn(connection).await?;
            assert_eq!(
                first[0].getAttribute("owner_id"),
                first[1].getAttribute("owner_id")
            );
            assert_eq!(
                first[0].getAttribute("owner_id"),
                second[0].getAttribute("owner_id")
            );
            assert_eq!(
                second[0].getAttribute("owner_id"),
                second[1].getAttribute("owner_id")
            );
            assert_eq!(
                first[0].getAttribute("owner_type"),
                Some(&"seed-owner".into())
            );

            let foreign = Root::new();
            let url = std::env::var("SEED_FIXTURE_MYSQL").unwrap();
            let foreign_connection = foreign.connection(Some(&url), "");
            let foreign_owner = FactoryOwner::queryOn(&foreign_connection)
                .findOrFail(first[0].getAttribute("owner_id").unwrap().clone())
                .await?;
            let independent = children()
                .r#for(subject(), owners())
                .recycle(foreign_owner)
                .makeOn(connection)
                .await?;
            assert_ne!(
                independent.getAttribute("owner_id"),
                first[0].getAttribute("owner_id")
            );

            let owner_pool = FactoryOwner::queryOn(connection).get().await?;
            let reusable_ids = owner_pool.modelKeys();
            let drafts = children()
                .r#for(subject(), owners())
                .count(3)
                .recycleMany(owner_pool)
                .makeOn(connection)
                .await?;
            let reused = drafts[0].getAttribute("owner_id").unwrap();
            assert!(reusable_ids.contains(reused));
            assert!(
                drafts.iter().all(|draft| !draft.state().exists()
                    && draft.getAttribute("owner_id") == Some(reused))
            );

            let recycled = FactoryOwner::queryOn(connection)
                .findOrFail(reusable_ids[0].clone())
                .await?;
            let recycled_id = recycled.getKey().unwrap().clone();
            let owner = owners()
                .recycle(recycled)
                .hasNamed::<_, MorphMany<FactoryOwner, FactoryChild>>(
                    "children",
                    children().count(2),
                )
                .hasAttachedNamed::<_, MorphToMany<FactoryOwner, FactoryChild>>(
                    "attached",
                    children().r#for(subject(), owners()).count(1),
                    Attributes::from([("label".into(), "attached".into())]),
                )
                .createOn(connection)
                .await?;
            let attached = connection
                .table("seed_factory_links")
                .pluck("child_id")
                .await?;
            let linked = FactoryChild::queryOn(connection)
                .findOrFail(attached[0].clone())
                .await?;
            assert_eq!(linked.getAttribute("owner_id"), Some(&recycled_id));
            let draft = children()
                .forExisting(subject(), owner)
                .makeOn(connection)
                .await?;
            assert_eq!(draft.getAttribute("owner_type"), Some(&"seed-owner".into()));
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
            .createOn(context.connection())
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
    let mut schema = connection.getSchemaBuilder();
    schema
        .create("seed_factory_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("seed_factory_children", |table| {
            table.id();
            table.unsignedBigInteger("owner_id").nullable();
            table.string("generated_name", 80);
        })
        .await?;
    let parent = Factory::<FactoryOwner>::new(Attributes::new)
        .createOn(connection)
        .await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let factory = || {
        let calls = Arc::clone(&calls);
        Factory::<FactoryChild>::new(Attributes::new)
            .stateWithParent::<FactoryOwner>(|_, parent| {
                let key = parent.and_then(Model::getKey).unwrap();
                Attributes::from([("generated_name".into(), format!("parent-{key:?}").into())])
            })
            .afterCreatingWithParent::<FactoryOwner>(move |model, parent| {
                assert!(model.state().exists());
                assert!(parent.and_then(Model::getKey).is_some());
                calls.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })
    };
    let final_state = |attributes: &Attributes, model: &FactoryOwner| {
        let key = model.getKey().unwrap();
        assert_eq!(model.getKey(), parent.getKey());
        assert_eq!(
            attributes["generated_name"],
            format!("parent-{key:?}").into()
        );
        Attributes::from([("generated_name".into(), "computed".into())])
    };
    let child = factory()
        .createWithParentStateOn(connection, &parent, final_state)
        .await?;
    assert!(child.getAttribute("owner_id").is_none());
    assert_eq!(
        child.getAttribute("generated_name"),
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
            .createWithParentOn(connection, &parent, cancelled())
            .await
            .is_err()
    );
    let quiet_state = |attributes: &Attributes, owner: &FactoryOwner| {
        final_state(attributes, owner);
        cancelled()
    };
    factory()
        .createQuietlyWithParentStateOn(connection, &parent, quiet_state)
        .await?;
    let batch = factory().count(2);
    let mut quiet_calls = 0;
    batch
        .createQuietlyWithParentStateOn(connection, &parent, |attributes, owner| {
            quiet_calls += 1;
            quiet_state(attributes, owner)
        })
        .await?;
    assert_eq!(quiet_calls, 2);
    assert!(
        batch
            .createWithParentOn(connection, &parent, cancelled())
            .await
            .is_err()
    );
    let single = factory();
    let lazy_one = single.lazyWithParent(&parent, Attributes::new());
    let lazy_batch = batch.lazyWithParent(&parent, Attributes::new());
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
            .whereNull("owner_id")
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
                SeederEntry::new("MutedSeeder", &MutedSeeder).withoutModelEvents(),
                SeederEntry::new("EventModelSeeder", &EventModelSeeder),
                SeederEntry::new("FailingEventSeeder", &FailingEventSeeder).withoutModelEvents(),
                SeederEntry::new("FailedSeederEventFlow", &FailedSeederEventFlow),
                SeederEntry::new("ParentCreationSeeder", &ParentCreationSeeder),
                SeederEntry::new("PolymorphicFactories", &PolymorphicFactories),
                SeederEntry::new("FakerApplicationScope", &FakerApplicationScope),
            ];
            FactoryOwner::resolveRelationUsing(app, "children", |_| {
                MorphMany::<FactoryOwner, FactoryChild>::new(
                    "id",
                    "owner_id",
                    "owner_type",
                    "seed-owner",
                )
            })?;
            FactoryChild::registerModelEvent(app, ModelEvent::Creating, CancelFactoryChild)?;
            BulkSeed::registerModelEvent(app, ModelEvent::Creating, CancelFactoryChild)?;
            MorphMap::morphMap(
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
        .withConfig(load)
        .withProviders(vec![Box::new(Provider)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.container().get::<DatabaseManager>("db").unwrap();
    let status = app.handleCommand(&args).await.unwrap();
    assert!(matches!(
        SeedItem::newInstance(Attributes::from([("name".into(), "outside".into())])),
        Err(DatabaseError::MassAssignment(name)) if name == "name"
    ));
    assert_eq!(manager.getDefaultConnection().unwrap(), "sqlite");
    std::process::exit(status);
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Root(PathBuf);
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
        Self(root)
    }
    fn fixture(&self, environment: &str, mysql: Option<&str>, args: &[&str]) -> Output {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "seeder_command_fixture", "--nocapture"])
            .env("SEED_FIXTURE_ROOT", &self.0)
            .env("SEED_FIXTURE_ENV", environment)
            .env("SEED_FIXTURE_ARGS", serde_json::to_string(args).unwrap());
        if let Some(url) = mysql {
            command.env("SEED_FIXTURE_MYSQL", url);
        } else {
            command.env_remove("SEED_FIXTURE_MYSQL");
        }
        command.output().unwrap()
    }
    fn connection(&self, mysql: Option<&str>, file: &str) -> Connection {
        let config = mysql.map_or_else(
            || json!({"driver":"sqlite","database":file}),
            |url| json!({"url":url}),
        );
        let manager = DatabaseManager::new(
            Arc::new(Repository::new(BTreeMap::from([(
                "database".into(),
                json!({"default":"verify","connections":{"verify":config}}),
            )]))),
            &self.0,
        );
        manager.connection(None).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
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
        db.getSchemaBuilder().dropIfExists(table).await.unwrap();
    }
    let run = |args: &[&str]| root.fixture("local", mysql, args);
    let pretend = success(run(&[
        "migrate",
        "--database=alternate",
        "--pretend",
        "--seed",
    ]));
    assert!(!pretend.contains("Seeding database"));
    assert!(!db.getSchemaBuilder().hasTable("seed_items").await.unwrap());
    let seeded = success(run(&["migrate", "--database=alternate", "--seed"]));
    assert!(seeded.contains("FirstSeeder DONE") && seeded.contains("ParameterSeeder DONE"));
    assert!(!seeded.contains("SilentSeeder"));
    let rows = || db.table("seed_items").orderBy("id", "asc");
    assert_eq!(
        rows().pluck("name").await.unwrap(),
        ["first", "second", "silent", "parameter"].map(Value::from)
    );
    let default = root.connection(None, "database/database.sqlite");
    assert!(
        !default
            .getSchemaBuilder()
            .hasTable("seed_items")
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
        db.getSchemaBuilder().drop(table).await.unwrap();
    }
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
async fn artisan_seeder_persists_polymorphic_factory_relationships_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let root = Root::new();
    let db = root.connection(Some(&url), "");
    let mut schema = db.getSchemaBuilder();
    for table in [
        "seed_bulk",
        "seed_factory_links",
        "seed_factory_children",
        "seed_factory_owners",
    ] {
        schema.dropIfExists(table).await.unwrap();
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
}

#[tokio::test]
async fn mysql_fresh_seeds_the_selected_database() {
    if let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_WIPE_URL") {
        verify(Some(&url), true).await;
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
    let faker = openrust::database::eloquent::factories::Faker::new("en_US")?;
    for (value, decimals, expected) in [
        (5.0e-310, 309, 5.0e-310),
        (1.4239704891618352e-200, 200, 1.0e-200),
        (1.75e308, -308, 1.75e308),
        (1.0e308, -309, 0.0),
        (0.285, 10, 0.285),
    ] {
        assert_eq!(
            faker.randomFloat(Some(decimals), value, Some(value))?,
            expected
        );
    }
    Ok(())
}

#[test]
fn faker_lorem_preserves_counts_byte_limits_and_unique_history() -> DatabaseResult<()> {
    let faker = openrust::database::eloquent::factories::Faker::new("en_US")?;
    assert!(faker.words(0).is_empty());
    assert!(faker.sentenceWithWordCount(0, true).is_empty());
    assert!(faker.paragraph(0, true).is_empty());
    assert_eq!(faker.words(4).len(), 4);
    assert_eq!(faker.wordsAsText(4).split_whitespace().count(), 4);
    let sentence = faker.sentenceWithWordCount(4, false);
    assert_eq!(sentence.split_whitespace().count(), 4);
    assert!(sentence.starts_with(char::is_uppercase));
    assert!(sentence.ends_with('.'));
    assert_eq!(faker.paragraph(4, false).matches('.').count(), 4);
    assert_eq!(faker.sentences(4).len(), 4);
    assert_eq!(faker.sentencesAsText(4).matches('.').count(), 4);
    assert_eq!(faker.paragraphs(4).len(), 4);
    assert_eq!(faker.paragraphsAsText(4).split("\n\n").count(), 4);
    for _ in 0..16 {
        assert!(
            (7..=15).contains(
                &faker
                    .sentenceWithWordCount(10, true)
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
    assert!(unique.wordsAsText(0)?.is_empty());
    assert!(unique.wordsAsText(0).is_err());
    assert!(unique.paragraphs(0)?.is_empty());
    assert!(unique.paragraphs(0).is_err());
    assert!(faker.unique(true).words(0)?.is_empty());
    let localized = openrust::database::eloquent::factories::Faker::new("zh_CN")?;
    let text = localized.text(200)?;
    assert!(text.len() <= 200 && !text.is_ascii());
    Ok(())
}

#[test]
fn faker_placeholders_preserve_classes_literals_and_unique_history() -> DatabaseResult<()> {
    let faker = openrust::database::eloquent::factories::Faker::new("zh_TW")?;
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
        assert!(faker.randomLetter().is_ascii_lowercase());
        assert!(faker.randomAscii().is_ascii_graphic());
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
    let faker = openrust::database::eloquent::factories::Faker::new("en_US")?;
    let values = [4, 4, 9];
    assert_eq!(faker.randomElement::<i32>(&[]), None);
    assert_eq!(faker.randomKey::<i32>(&[]), None);
    assert!(faker.randomElements::<i32>(&[], Some(0), false)?.is_empty());
    assert!(faker.randomElements::<i32>(&[], None, false).is_err());
    assert!(faker.randomElements::<i32>(&[], Some(1), true).is_err());
    assert!(faker.randomElements(&values, Some(4), false).is_err());
    for _ in 0..64 {
        let mut selected = faker.randomElements(&values, Some(3), false)?;
        selected.sort_unstable();
        assert_eq!(selected, [&4, &4, &9]);
        assert!((1..=3).contains(&faker.randomElements(&values, None, false)?.len()));
        assert_eq!(
            faker.randomElements(&values[..1], Some(5), true)?,
            vec![&4; 5]
        );
        assert!(
            faker
                .randomKey(&values)
                .is_some_and(|index| index < values.len())
        );
        assert!(
            faker
                .randomElement(&values)
                .is_some_and(|item| values.contains(item))
        );
        assert!(!faker.boolean(Some(0)));
        assert!(faker.boolean(Some(100)));
    }
    let mut shuffled = faker.shuffleArray(&values);
    shuffled.sort_unstable();
    assert_eq!(shuffled, values);
    let mut shuffled: Vec<_> = faker.shuffleString("中a😊中").chars().collect();
    shuffled.sort_unstable();
    assert_eq!(shuffled, ['a', '中', '中', '😊']);
    assert_eq!(faker.shuffleString(""), "");
    let unique = faker.unique(false);
    assert!(!unique.boolean(Some(0))?);
    assert!(unique.boolean(Some(0)).is_err());
    assert_eq!(unique.randomKey(&values[..1])?, Some(0));
    assert!(unique.randomKey(&values[..1]).is_err());
    assert_eq!(unique.randomElement(&values[..1])?, Some(&4));
    assert!(unique.randomElement(&values[1..2]).is_err());
    assert_eq!(unique.randomElements(&values[..1], Some(1), false)?, [&4]);
    assert!(
        unique
            .randomElements(&values[1..2], Some(1), false)
            .is_err()
    );
    assert_eq!(unique.shuffleArray(&values[..1])?, [4]);
    assert!(unique.shuffleArray(&values[..1]).is_err());
    assert_eq!(unique.shuffleString("中")?, "中");
    assert!(unique.shuffleString("中").is_err());
    Ok(())
}

#[test]
fn faker_miscellaneous_codes_and_emoji_preserve_the_configured_locale() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    let faker = Faker::new("zh-Hant-TW")?;
    let unique = faker.unique(false);
    let mut language_codes = std::collections::HashSet::new();
    for _ in 0..184 {
        let language = unique.languageCode()?;
        assert_eq!(language.len(), 2);
        assert!(language.bytes().all(|byte| byte.is_ascii_lowercase()));
        assert!(language_codes.insert(language));
    }
    assert!(language_codes.contains("en"));
    assert!(language_codes.contains("zh"));
    assert!(unique.languageCode().is_err());

    let mut locales = std::collections::HashSet::new();
    for _ in 0..255 {
        let locale = unique.randomLocale()?;
        assert_eq!(
            openrust::translation::canonical_locale(locale)
                .as_deref()
                .ok(),
            Some(locale)
        );
        assert!(locales.insert(locale));
    }
    for locale in ["en_GB", "en_US", "zh_CN", "zh_HK", "zh_TW"] {
        assert!(locales.contains(locale));
    }
    assert!(unique.randomLocale().is_err());

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

    check_codes(|| unique.countryCode(), 247, 2)?;
    check_codes(|| unique.countryISOAlpha3(), 249, 3)?;
    check_codes(|| unique.currencyCode(), 154, 3)?;
    let mut emoji = (0..56)
        .map(|_| unique.emoji())
        .collect::<DatabaseResult<Vec<_>>>()?;
    emoji.sort_unstable();
    assert_eq!(emoji.first(), Some(&"😀"));
    assert_eq!(emoji.last(), Some(&"😷"));
    assert!(unique.emoji().is_err());
    assert_eq!(faker.locale(), "zh_TW");
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

    let faker = Faker::new("en_US")?;
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

    let faker = Faker::new("en_US")?;
    let unique = faker.unique(false);
    let mut addresses = std::collections::HashSet::new();
    for _ in 0..256 {
        let sample = unique.ipv4()?;
        let address = u32::from(sample.parse::<Ipv4Addr>().unwrap());
        assert!((0x0100_0000..=0xffff_fffe).contains(&address));
        assert!(addresses.insert(sample));

        let sample = unique.localIpv4()?;
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

        let sample = unique.macAddress()?;
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

    let faker = Faker::new("en_US")?;
    assert!(faker.slug(Some(0), true).is_empty());
    for _ in 0..64 {
        assert_eq!(faker.slug(Some(3), false).split('-').count(), 3);
        assert!((4..=9).contains(&faker.slug(None, true).split('-').count()));
        let domain = faker.domainName()?;
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
            "zh_CN",
            &[
                "biz", "biz.cn", "cn", "com", "com.cn", "edu.cn", "gov.cn", "info", "net",
                "net.cn", "org", "org.cn",
            ][..],
        ),
        ("zh_TW", &["biz", "com", "info", "net", "org"][..]),
        ("fr_FR", &["com", "fr", "net", "org"][..]),
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
    assert!(unique.domainWord().is_ok());
    assert!(unique.domainName().is_ok());
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
        let username = faker.userName();
        assert!(!username.trim_matches(['.', '_']).is_empty());
        assert!(
            username
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_'))
        );
        for sample in [faker.freeEmail(), faker.email()?] {
            let (name, domain) = sample.split_once('@').unwrap();
            assert!(!name.is_empty());
            assert!(free_domains.contains(&domain));
        }
        for sample in [faker.safeEmail(), faker.companyEmail()?] {
            assert_eq!(sample.matches('@').count(), 1);
            assert!(!sample.contains(char::is_whitespace));
        }
        assert!(matches!(
            faker.safeEmailDomain(),
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
        .map(|_| unique.freeEmailDomain())
        .collect::<DatabaseResult<Vec<_>>>()?;
    sampled.sort_unstable();
    let mut expected = free_domains;
    expected.sort_unstable();
    assert_eq!(sampled, expected);
    assert!(unique.freeEmailDomain().is_err());
    assert!(unique.email().is_ok());
    assert!(unique.companyEmail().is_ok());
    assert!(unique.safeEmail().is_ok());
    assert!(unique.freeEmail().is_ok());
    assert!(unique.userName().is_ok());
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
        let number = cn.phoneNumber();
        assert_eq!(number.len(), 11);
        assert!(number.starts_with("1") && number.bytes().all(|byte| byte.is_ascii_digit()));
        let number = tw.phoneNumber();
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
        ("en_US", "+1", 12..=12),
        ("fr_FR", "+33", 12..=12),
        ("de_DE", "+49", 12..=15),
        ("en_GB", "+44", 13..=13),
        ("tr_TR", "+90", 13..=13),
    ] {
        let faker = Faker::new(locale)?;
        let unique = faker.unique(false);
        let mut numbers = std::collections::HashSet::new();
        for _ in 0..128 {
            let number = unique.e164PhoneNumber()?;
            assert!(number.starts_with(prefix));
            assert!(lengths.contains(&number.len()), "{locale}: {number}");
            assert!(number[1..].bytes().all(|byte| byte.is_ascii_digit()));
            assert!(numbers.insert(number));
        }
    }
    assert!(tw.unique(false).phoneNumber().is_ok());
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
        assert!(matches!(us.buildingNumber().len(), 3..=5));
        assert!(us.secondaryAddress().starts_with(['A', 'S']));

        let address = cn.address();
        assert!(address.ends_with('区') && !address.contains('\n'));
        let postcode = cn.postcode();
        assert_eq!(postcode.len(), 6);
        assert!(postcode.ends_with("00"));
        assert!((1..=85).contains(&postcode[..2].parse::<u8>().unwrap()));
        assert!((10..=88).contains(&postcode[2..4].parse::<u8>().unwrap()));

        let street = tw.streetAddress();
        assert!(street.contains('號'));
        assert!(street.ends_with(['號', '樓']));
        assert!(tw.address().contains('號'));
        assert!(tw.secondaryAddress().ends_with(['室', '房']));

        let address = fr.address();
        let (_, locality) = address.split_once('\n').unwrap();
        let (postcode, _) = locality.split_once(' ').unwrap();
        assert_eq!(postcode.len(), 5);
        assert!(postcode.bytes().all(|byte| byte.is_ascii_digit()));

        assert!(["先生", "博士", "教授"].contains(&tw.title(Some("male"))));
        assert!(["小姐", "太太", "博士", "教授"].contains(&tw.titleFemale()));
        assert!(
            [
                "女士", "小姐", "夫人", "太太", "医生", "老师", "教授", "律师", "经理"
            ]
            .contains(&cn.title(Some("female")))
        );
        assert!(!us.country().is_empty());
        assert!(!us.cityPrefix().is_empty());
        assert!(!us.citySuffix().is_empty());
        assert!(!us.streetSuffix().is_empty());
        assert!(!us.state().is_empty());
    }
    let unique = tw.unique(false);
    let mut titles = (0..3)
        .map(|_| unique.titleMale())
        .collect::<DatabaseResult<Vec<_>>>()?;
    titles.sort_unstable();
    assert_eq!(titles, ["先生", "博士", "教授"]);
    assert!(unique.titleMale().is_err());
    assert!(unique.title(None).is_ok());
    assert!(unique.address().is_ok());
    assert!(tw.unique(true).titleMale().is_ok());
    Ok(())
}

#[test]
fn faker_gendered_names_follow_catalogs_and_share_unique_history() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    for locale in [
        "ar_SA", "de_DE", "fa_IR", "fr_FR", "it_IT", "ja_JP", "nl_NL", "pt_BR", "pt_PT", "tr_TR",
        "zh_CN", "zh_TW", "en_US", "cy_GB",
    ] {
        let faker = Faker::new(locale)?;
        for _ in 0..64 {
            assert!(!faker.firstNameMale().is_empty());
            assert!(!faker.firstNameFemale().is_empty());
            for gender in [Some("male"), Some("female"), None, Some("other")] {
                let name = faker.nameWithGender(gender);
                assert!(!name.trim().is_empty());
                if matches!(locale, "zh_CN" | "zh_TW") {
                    assert!(name.chars().all(|character| !character.is_ascii()));
                }
            }
        }
    }

    let faker = Faker::new("zh-Hant-TW")?;
    let unique = faker.unique(false);
    let names = (0..64)
        .map(|_| unique.firstNameWithGender(Some("female")))
        .collect::<DatabaseResult<std::collections::HashSet<_>>>()?;
    assert_eq!(names.len(), 64);
    assert!(
        names
            .iter()
            .all(|name| (1..=2).contains(&name.chars().count()))
    );
    assert!(unique.firstNameMale().is_ok());
    assert!(unique.firstNameFemale().is_ok());
    assert!(unique.nameWithGender(Some("male")).is_ok());
    assert!(unique.name().is_ok());
    assert!(faker.unique(true).firstNameWithGender(None).is_ok());
    Ok(())
}

#[test]
fn faker_coordinates_preserve_bounds_precision_and_unique_history() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::Faker;

    let global = Faker::new("en_US")?;
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
            let sample = faker.localCoordinates()?;
            assert!(latitude.contains(&sample.latitude));
            assert!(longitude.contains(&sample.longitude));
            for coordinate in [sample.latitude, sample.longitude] {
                assert_eq!(
                    coordinate,
                    format!("{coordinate:.6}").parse::<f64>().unwrap()
                );
            }
        }
        assert!((22.0..=25.0).contains(&taiwan.localLatitude()?));
        assert!((120.0..=122.0).contains(&taiwan.localLongitude()?));
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
    assert!(unique.localCoordinates().is_ok());
    assert!(taiwan.unique(false).localLatitude().is_ok());
    assert!(taiwan.unique(false).localLongitude().is_ok());
    Ok(())
}

struct TrashedProbe(ModelState);

impl Model for TrashedProbe {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("trashed_factory_probe")
            .timestamps(None, None)
            .softDeletes("removed_at");
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
    db.getSchemaBuilder()
        .create("trashed_factory_probe", |table| {
            table.id();
            table.string("name", 60);
            table.dateTime("removed_at", 0).nullable();
        })
        .await?;
    let definition = || Attributes::from([("name".into(), "sample".into())]);
    let now = chrono::Utc::now();
    let batch = Factory::<TrashedProbe>::new(definition)
        .count(2)
        .trashed(None)?;
    let previews = batch.makeOn(db).await?;
    assert_eq!(previews.len(), 2);
    let mut previous = None;
    for model in previews.iter() {
        assert!(model.trashed());
        let Some(Value::Text(value)) = model.getAttributeValue("removed_at")? else {
            panic!("expected date text");
        };
        let raw = chrono::DateTime::parse_from_rfc3339(&value).unwrap();
        assert!((86_399..=86_401).contains(&now.signed_duration_since(raw).num_seconds()));
        if let Some(previous) = &previous {
            assert_eq!(&value, previous);
        }
        previous = Some(value);
    }
    assert!(
        TrashedProbe::queryOn(db)
            .withTrashed()
            .get()
            .await?
            .is_empty()
    );
    assert_eq!(batch.createOn(db).await?.len(), 2);

    let explicit = "2020-02-03 04:05:06";
    let model = Factory::<TrashedProbe>::new(definition)
        .trashed(Some(explicit.into()))?
        .createOn(db)
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
        .createOn(db)
        .await?;
    assert!(!restored.trashed());
    assert_eq!(TrashedProbe::queryOn(db).get().await?.len(), 1);
    assert_eq!(
        TrashedProbe::queryOn(db).onlyTrashed().get().await?.len(),
        3
    );
    db.getSchemaBuilder().drop("trashed_factory_probe").await?;
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
    db.getSchemaBuilder()
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
        .unlessElse(
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
        single.rawOn(db).await?.get("name"),
        Some(&"unless fallback".into())
    );
    single.createOn(db).await?;

    let batch = Factory::<ConditionalProbe>::new(definition)
        .count(2)
        .whenElse(
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
            .makeOn(db)
            .await?
            .iter()
            .all(|model| model.getAttribute("name") == Some(&"batch".into()))
    );
    assert_eq!(batch.createOn(db).await?.len(), 2);
    assert_eq!(
        db.table("conditional_factory_probe")
            .orderBy("id", "asc")
            .pluck("name")
            .await?,
        ["unless fallback".into(), "batch".into(), "batch".into()]
    );
    db.getSchemaBuilder()
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
        configured.rawOn(&connection).await.unwrap().get("label"),
        Some(&"configured".into())
    );
    assert_eq!(
        overridden.rawOn(&connection).await.unwrap().get("label"),
        Some(&"explicit".into())
    );
    assert_eq!(
        BoundFactory::factory()
            .rawOn(&connection)
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
    let mut schema = db.getSchemaBuilder();
    schema
        .create("factory_parent_probe", |table| {
            table.id();
            table.string("name", 60);
        })
        .await?;
    schema
        .create("factory_child_probe", |table| {
            table.id();
            table.unsignedBigInteger("parent_id").nullable();
            table.string("name", 60);
        })
        .await?;
    schema
        .create("factory_parent_links", |table| {
            table.unsignedBigInteger("parent_id");
            table.unsignedBigInteger("child_id");
        })
        .await?;

    let standalone =
        Factory::<ChildProbe>::new(|| Attributes::from([("name".into(), "standalone".into())]))
            .stateWithParent::<ParentProbe>(|_, parent| {
                assert!(parent.is_none());
                Attributes::new()
            })
            .afterCreatingWithParent::<ParentProbe>(|_, parent| {
                assert!(parent.is_none());
                Ok(())
            });
    standalone.createOn(db).await?;

    let children = |attached| {
        Factory::<ChildProbe>::new(|| Attributes::from([("name".into(), "base".into())]))
            .stateWithParent::<ChildProbe>(|_, parent| {
                assert!(parent.is_none());
                Attributes::new()
            })
            .count(2)
            .stateWithParent::<ParentProbe>(move |attributes, parent| {
                assert_eq!(attributes.get("name"), Some(&"base".into()));
                let parent = parent.unwrap();
                assert!(parent.getAttribute("id").is_some());
                let name = if attached { "attached" } else { "direct" };
                assert_eq!(parent.getAttribute("name"), Some(&"owner".into()));
                Attributes::from([("name".into(), name.into())])
            })
            .sequenceWithParent::<ParentProbe>(|index, attributes, parent| {
                assert!(parent.is_some_and(|model| model.state().exists()));
                let Value::Text(name) = &attributes["name"] else {
                    panic!("earlier state did not set the child name");
                };
                Attributes::from([("name".into(), format!("{name}-{index}").into())])
            })
            .afterCreatingWithParent::<ParentProbe>(move |child, parent| {
                let parent = parent.unwrap();
                assert!(child.getAttribute("id").is_some());
                if !attached {
                    assert_eq!(child.getAttribute("parent_id"), parent.getAttribute("id"));
                }
                assert_eq!(parent.getAttribute("name"), Some(&"owner".into()));
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
        .hasAttached(
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
        .createOn(db)
        .await?;
    assert_eq!(
        db.table("factory_child_probe")
            .orderBy("id", "asc")
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
    let mut schema = connection.getSchemaBuilder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_children", |table| {
            table.id();
            table.unsignedBigInteger("owner_id").nullable();
        })
        .await?;

    let factory = || {
        Factory::<ExpansionChild>::new(Attributes::new)
            .parentAttribute("owner_id", Factory::<ExpansionOwner>::new(Attributes::new))
    };
    let before = factory();
    Factory::<ExpansionOwner>::dontExpandRelationshipsByDefault();
    let disabled = factory()
        .count(2)
        .withoutParents(vec![FactoryParentSelector::attribute("other")]);
    let enabled = {
        Factory::<ExpansionChild>::expandRelationshipsByDefault();
        factory()
    };
    let owner = Factory::<ExpansionOwner>::new(Attributes::new)
        .createOn(connection)
        .await?;
    let key = owner.getKey().unwrap().clone();
    assert!(
        disabled
            .recycle(owner)
            .rawOn(connection)
            .await?
            .iter()
            .all(|row| row["owner_id"] == Value::Null)
    );
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    Factory::<ExpansionOwner>::dontExpandRelationshipsByDefault();
    let overridden = factory()
        .set("owner_id", key.clone())
        .createOn(connection)
        .await?;
    assert_eq!(overridden.getAttribute("owner_id"), Some(&key));
    let relation = BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id");
    let owner = ExpansionOwner::queryOn(connection)
        .get()
        .await?
        .into_iter()
        .next()
        .unwrap();
    let stored_key = owner.getKey().unwrap().clone();
    let explicit = Factory::<ExpansionChild>::new(Attributes::new)
        .forExisting(relation, owner)
        .createOn(connection)
        .await?;
    assert_eq!(explicit.getAttribute("owner_id"), Some(&stored_key));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    let suppressed = enabled
        .withoutParents(vec![])
        .withoutParents(vec![FactoryParentSelector::attribute("other")]);
    assert_eq!(suppressed.rawOn(connection).await?["owner_id"], Value::Null);
    assert_ne!(
        before.createOn(connection).await?.getAttribute("owner_id"),
        Some(&Value::Null)
    );
    Factory::<ExpansionChild>::flushState();
    assert_ne!(
        factory()
            .createOn(connection)
            .await?
            .getAttribute("owner_id"),
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
}

async fn verify_deferred_factory_owner(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_children", |table| {
            table.id();
            table.unsignedBigInteger("owner_id").nullable();
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
        .rawOn(connection)
        .await?;
    assert_eq!(skipped["owner_id"], Value::Null);

    let overridden = children()
        .r#for(relation(), owners())
        .makeOneOn(
            connection,
            Attributes::from([("owner_id".into(), 5_i64.into())]),
        )
        .await?;
    assert_eq!(overridden.getAttribute("owner_id"), Some(&5_i64.into()));
    assert!(!overridden.relationLoaded("owner"));

    let morph_class = ExpansionOwner::from_state(ModelState::default()).getMorphClass()?;
    let morph = MorphTo::<ExpansionChild>::new("owner_type", "owner_id")
        .target::<ExpansionOwner>(morph_class, "id");
    let morph_preview = children()
        .r#for(morph, owners())
        .state(move |attributes| {
            assert_eq!(attributes["owner_type"], morph_class.into());
            Attributes::from([("owner_id".into(), Value::Null)])
        })
        .rawOn(connection)
        .await?;
    assert_eq!(morph_preview["owner_id"], Value::Null);
    assert_eq!(morph_preview["owner_type"], morph_class.into());
    assert_eq!(connection.table("expansion_owners").count("*").await?, 0);

    let records = children()
        .parentAttribute("owner_id", owners().connection(Some("missing")))
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
        .createOn(connection)
        .await?;
    assert_eq!(records[0].getAttribute("owner_id"), Some(&5_i64.into()));
    assert!(!records[0].relationLoaded("owner"));
    let generated_key = records[1].getAttribute("owner_id").unwrap();
    assert_ne!(*generated_key, 98_i64.into());
    assert_eq!(Some(generated_key), records[2].getAttribute("owner_id"));
    assert!(records[1].relationLoaded("owner"));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    let owner = ExpansionOwner::queryOn(connection)
        .findOrFail(generated_key.clone())
        .await?;
    let replaced = children()
        .forExisting(relation(), owner)
        .set("owner_id", Value::Null)
        .createOn(connection)
        .await?;
    assert_eq!(replaced.getAttribute("owner_id"), Some(&Value::Null));
    assert!(!replaced.relationLoaded("owner"));
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
    assert!(factory.rawOn(connection).await.is_err());
    let mut schema = connection.getSchemaBuilder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    let (raw, model) = tokio::join!(factory.rawOn(connection), factory.makeOn(connection));
    let raw = raw?;
    let model = model?;
    assert_eq!(Some(&raw["owner_id"]), model.getAttribute("owner_id"));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);

    let batch = factory.count(2);
    assert!(
        batch
            .rawOn(connection)
            .await?
            .iter()
            .all(|row| row["owner_id"] == raw["owner_id"])
    );
    assert!(batch.rawOn(independent).await.is_err());
    let mut independent_schema = independent.getSchemaBuilder();
    independent_schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    independent
        .table("expansion_owners")
        .insert(&[Attributes::from([("id".into(), 100_i64.into())])])
        .await?;
    let independent_result = batch.rawOn(independent).await?;
    assert_ne!(independent_result[0]["owner_id"], raw["owner_id"]);
    assert_eq!(
        independent_result[0]["owner_id"],
        independent_result[1]["owner_id"]
    );
    assert_eq!(independent.table("expansion_owners").count("*").await?, 2);
    assert_eq!(
        batch.rawOn(connection).await?[0]["owner_id"],
        raw["owner_id"]
    );

    let local_owner = ExpansionOwner::queryOn(connection)
        .findOrFail(raw["owner_id"].clone())
        .await?;
    let local_key = local_owner.getKey().unwrap().clone();
    let remote_owner = ExpansionOwner::queryOn(independent)
        .findOrFail(independent_result[0]["owner_id"].clone())
        .await?;
    let remote_key = remote_owner.getKey().unwrap().clone();
    let recycled = Factory::<ExpansionChild>::new(Attributes::new);
    assert!(
        recycled
            .getRandomRecycledModel::<ExpansionOwner>()
            .is_none()
    );
    let recycled = recycled.recycle(ExpansionOwner::from_state(ModelState::default()));
    assert!(
        recycled
            .getRandomRecycledModel::<ExpansionOwner>()
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
            .getRandomRecycledModel::<ExpansionChild>()
            .is_none()
    );
    let keys = [&local_key, &remote_key];
    assert!(
        keys.contains(
            &recycled
                .getRandomRecycledModel::<ExpansionOwner>()
                .unwrap()
                .getKey()
                .unwrap()
        )
    );
    assert!(
        recycled
            .rawOn(connection)
            .await?
            .iter()
            .all(|row| row["owner_id"] == local_key)
    );
    assert!(
        recycled
            .rawOn(independent)
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
    assert_eq!(initial.modelName(), std::any::type_name::<ShortcutModel>());
    let attributes = Attributes::from([("label".into(), "direct".into())]);
    let unsaved = initial.newModel(attributes.clone())?;
    assert!(!unsaved.state().exists());
    assert_eq!(unsaved.getAttribute("label"), Some(&"direct".into()));
    assert!(
        initial
            .newModel(Attributes::new())?
            .getAttribute("label")
            .is_none()
    );
    let guarded = initial.newModel(Attributes::from([
        ("label".into(), "direct".into()),
        ("id".into(), 9_i64.into()),
    ]))?;
    assert!(guarded.getAttribute("id").is_none());
    let computed = ShortcutFactory::newWithState(|attributes| {
        assert_eq!(attributes["label"], "definition".into());
        Attributes::from([("label".into(), "computed".into())])
    });
    assert_eq!(
        computed.rawOn(connection).await?["label"],
        "computed-configured".into()
    );

    assert_eq!(
        initial.rawOn(connection).await?["label"],
        "initial-configured".into()
    );
    assert_eq!(
        initial.set("label", "override").rawOn(connection).await?["label"],
        "override".into()
    );

    let batch = ShortcutFactory::times(2);
    assert_eq!(batch.modelName(), std::any::type_name::<ShortcutModel>());
    assert_eq!(
        batch.newModel(attributes)?.getAttribute("label"),
        Some(&"direct".into())
    );
    let previews = batch.rawOn(connection).await?;
    assert_eq!(previews.len(), 2);
    assert!(
        previews
            .iter()
            .all(|row| row["label"] == "definition-configured".into())
    );

    let mut schema = connection.getSchemaBuilder();
    schema
        .create("factory_shortcuts", |table| {
            table.id();
            table.string("label", 100);
        })
        .await?;
    assert!(
        ShortcutFactory::times(0)
            .createOn(connection)
            .await?
            .is_empty()
    );
    assert_eq!(connection.table("factory_shortcuts").count("*").await?, 0);
    assert_eq!(batch.createOn(connection).await?.len(), 2);
    assert_eq!(
        connection
            .table("factory_shortcuts")
            .orderBy("id", "asc")
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
}

async fn verify_factory_raw_overrides(connection: &Connection) -> DatabaseResult<()> {
    let factory = Factory::<FactoryChild>::factoryForModel();
    assert_eq!(
        factory.rawOn(connection).await?["label"],
        "configured".into()
    );
    let overrides = Attributes::from([("label".into(), "temporary".into())]);
    assert_eq!(
        factory.rawWithOn(connection, overrides.clone()).await?["label"],
        "temporary".into()
    );
    let raw = factory
        .rawWithStateOn(connection, |attributes| {
            assert_eq!(attributes["label"], "configured".into());
            overrides.clone()
        })
        .await?;
    assert_eq!(raw["label"], "temporary".into());
    assert_eq!(
        factory.rawOn(connection).await?["label"],
        "configured".into()
    );

    let mut schema = connection.getSchemaBuilder();
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
        .afterMaking(|_| panic!("raw invoked afterMaking"))
        .sequence(|index| Attributes::from([("position".into(), index.to_string().into())]))
        .count(2);
    let rows = batch
        .rawWithStateOn(connection, |attributes| {
            assert!(attributes.contains_key("position"));
            Attributes::from([("owner_id".into(), Value::Null)])
        })
        .await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["position"], "0".into());
    assert_eq!(rows[1]["position"], "1".into());
    assert!(rows.iter().all(|row| row["owner_id"] == Value::Null));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 0);

    let rows = batch.rawWithOn(connection, overrides).await?;
    assert_eq!(rows[0]["owner_id"], rows[1]["owner_id"]);
    assert!(rows.iter().all(|row| row["label"] == "temporary".into()));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);
    assert_eq!(batch.rawOn(connection).await?[0].get("label"), None);
    let empty = Factory::<ExpansionChild>::new(|| panic!("empty batch evaluated definition"))
        .count(0)
        .rawWithStateOn(connection, |_| panic!("empty batch evaluated state"))
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
}

async fn verify_raw_parent_context(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    let owners = || Factory::<ExpansionOwner>::new(Attributes::new);
    let parent = owners().createOn(connection).await?;
    let key = parent.getKey().unwrap().clone();
    let children = || {
        Factory::<ExpansionChild>::new(Attributes::new).stateWithParent::<ExpansionOwner>(
            |_, parent| {
                Attributes::from([(
                    "context_key".into(),
                    parent
                        .and_then(Model::getKey)
                        .cloned()
                        .unwrap_or(Value::Null),
                )])
            },
        )
    };
    let only_context = children()
        .rawWithParentOn(connection, &parent, |_, _| Attributes::new())
        .await?;
    assert_eq!(only_context["context_key"], key);
    assert!(!only_context.contains_key("owner_id"));

    let factory = children().r#for(
        BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id"),
        owners(),
    );
    let attributes = factory
        .rawWithParentOn(connection, &parent, |attributes, model| {
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
        .rawWithParentOn(connection, &parent, |attributes, model| {
            assert!(std::ptr::eq(model, &parent));
            assert_eq!(attributes["context_key"], key);
            calls += 1;
            Attributes::from([("owner_id".into(), key.clone())])
        })
        .await?;
    assert_eq!(calls, 2);
    assert!(rows.iter().all(|row| row["owner_id"] == key));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);
    let without_parent = batch.rawOn(connection).await?;
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
}

async fn verify_make_and_insert_parent(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();
    schema
        .create("expansion_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("expansion_children", |table| {
            table.id();
            table.unsignedBigInteger("owner_id").nullable();
            table.unsignedBigInteger("context_key");
            table.string("label", 80);
        })
        .await?;
    let owners = || Factory::<ExpansionOwner>::new(Attributes::new);
    let parent = owners().createOn(connection).await?;
    let key = parent.getKey().unwrap().clone();
    let factory = || {
        Factory::<ExpansionChild>::new(Attributes::new)
            .stateWithParent::<ExpansionOwner>(|_, parent| {
                Attributes::from([(
                    "context_key".into(),
                    parent
                        .and_then(Model::getKey)
                        .cloned()
                        .unwrap_or(Value::Null),
                )])
            })
            .r#for(
                BelongsTo::<ExpansionChild, ExpansionOwner>::new("owner_id", "id"),
                owners(),
            )
            .afterMaking(|model| {
                assert!(!model.state().exists());
                model.setAttribute("label", "made");
                Ok(())
            })
            .afterCreating(|_| panic!("insert ran afterCreating"))
    };
    let overrides = || Attributes::from([("owner_id".into(), Value::Null)]);
    let final_state = |attributes: &Attributes, parent: &ExpansionOwner| {
        assert_eq!(parent.getKey(), Some(&key));
        assert_eq!(attributes["context_key"], key);
        overrides()
    };
    let single = factory();
    let model = single
        .makeWithParentStateOn(connection, &parent, final_state)
        .await?;
    assert_eq!(model.getAttribute("context_key"), Some(&key));
    assert_eq!(model.getAttribute("owner_id"), Some(&Value::Null));
    assert_eq!(model.getAttribute("label"), Some(&"made".into()));
    let batch = factory().count(2);
    let drafts = batch
        .makeWithParentStateOn(connection, &parent, final_state)
        .await?;
    assert_eq!(drafts.len(), 2);
    assert!(
        drafts
            .iter()
            .all(|model| model.getAttribute("context_key") == Some(&key))
    );
    assert_eq!(connection.table("expansion_children").count("*").await?, 0);

    Factory::<ExpansionChild>::new(|| panic!("empty batch evaluated definition"))
        .count(0)
        .insertWithParentOn(connection, &parent, overrides())
        .await?;
    assert_eq!(connection.table("expansion_children").count("*").await?, 0);
    single
        .insertWithParentOn(connection, &parent, overrides())
        .await?;
    batch
        .insertWithParentOn(connection, &parent, overrides())
        .await?;
    let rows = connection.table("expansion_children");
    assert_eq!(rows.count("*").await?, 3);
    let rows = ExpansionChild::queryOn(connection).get().await?;
    assert!(rows.iter().all(|model| {
        matches!(
            model.getAttribute("context_key"),
            Some(Value::Integer(1) | Value::Unsigned(1))
        ) && model.getAttribute("owner_id") == Some(&Value::Null)
            && model.getAttribute("label") == Some(&"made".into())
    }));
    assert_eq!(connection.table("expansion_owners").count("*").await?, 1);
    let without_parent = single.makeOneOn(connection, overrides()).await?;
    assert_eq!(
        without_parent.getAttribute("context_key"),
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
}

async fn verify_attachment_pivot_count(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();
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
            table.unsignedBigInteger("owner_id");
            table.unsignedBigInteger("child_id");
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
        .hasAttached(relation(), children(), pivots())
        .createOn(connection)
        .await?;
    let explicit = owners()
        .hasAttached(relation(), children().count(3), pivots())
        .createOn(connection)
        .await?;
    let empty = owners()
        .hasAttached(relation(), children().count(0), pivots())
        .createOn(connection)
        .await?;
    let default = owners()
        .hasAttached(relation(), children(), Vec::<Attributes>::new())
        .createOn(connection)
        .await?;
    let shared = owners()
        .hasAttached(
            relation(),
            children(),
            Attributes::from([("label".into(), "shared".into())]),
        )
        .createOn(connection)
        .await?;
    let labels = |owner: &ExpansionOwner| {
        connection
            .table("expansion_links")
            .r#where("owner_id", "=", owner.getKey().unwrap().clone())
            .orderBy("child_id", "asc")
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
}

async fn verify_seeder_events(root: &Root, mysql: Option<&str>) {
    let connection = root.connection(mysql, "database/alternate.sqlite");
    let mut schema = connection.getSchemaBuilder();
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
            command.env("DB_URL", url);
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
    verify_default_application_seeder(&Root::new(), Some(&url)).await;
}

async fn verify_sequence_lifecycle(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();
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
            table.unsignedBigInteger("owner_id");
            table.unsignedBigInteger("child_id");
            table.string("label", 40);
        })
        .await?;
    let label = |value: &str| Attributes::from([("label".into(), value.into())]);
    let factory = || {
        Factory::<ExpansionChild>::new(Attributes::new)
            .sequence(|index| Attributes::from([("label".into(), index.to_string().into())]))
    };
    let builder = factory();
    assert_eq!(builder.rawOn(connection).await?["label"], "0".into());
    let batch = builder.count(2);
    assert_eq!(
        batch.makeOn(connection).await?[1].getAttribute("label"),
        Some(&"2".into())
    );
    batch.insertOn(connection, Attributes::new()).await?;
    assert_eq!(
        batch.createOn(connection).await?[0].getAttribute("label"),
        Some(&"5".into())
    );
    assert_eq!(factory().rawOn(connection).await?["label"], "0".into());
    let each = Factory::<ExpansionChild>::new(Attributes::new)
        .forEachSequence(vec![label("first"), label("second")]);
    assert_eq!(
        each.makeOneOn(connection, Attributes::new())
            .await?
            .getAttribute("label"),
        Some(&"first".into())
    );
    assert_eq!(each.rawOn(connection).await?[0]["label"], "second".into());
    let product = Factory::<ExpansionChild>::new(Attributes::new)
        .crossJoinSequence(vec![vec![label("x"), label("y")]])?;
    assert_eq!(product.rawOn(connection).await?["label"], "x".into());
    assert_eq!(product.rawOn(connection).await?["label"], "y".into());

    let parent = Factory::<ExpansionOwner>::new(Attributes::new)
        .createOn(connection)
        .await?;
    let contextual =
        Factory::<ExpansionChild>::new(|| Attributes::from([("label".into(), "base".into())]))
            .set("label", "earlier")
            .count(2)
            .sequenceWithState(|index, attributes| {
                assert_eq!(attributes["label"], "earlier".into());
                Attributes::from([("label".into(), format!("step-{index}").into())])
            })
            .sequenceWithParent::<ExpansionOwner>(|index, attributes, parent| {
                assert_eq!(attributes["label"], format!("step-{index}").into());
                let suffix = if parent.is_some_and(|model| model.state().exists()) {
                    "saved"
                } else {
                    "none"
                };
                Attributes::from([("label".into(), format!("step-{index}-{suffix}").into())])
            });
    let draft = contextual.makeOneOn(connection, Attributes::new()).await?;
    assert_eq!(draft.getAttribute("label"), Some(&"step-0-none".into()));
    let created = contextual
        .createWithParentOn(connection, &parent, Attributes::new())
        .await?;
    assert_eq!(created.len(), 2);
    assert_eq!(
        created[0].getAttribute("label"),
        Some(&"step-1-saved".into())
    );
    assert_eq!(
        created[1].getAttribute("label"),
        Some(&"step-2-saved".into())
    );
    let preview = contextual.makeOneOn(connection, label("override")).await?;
    assert_eq!(preview.getAttribute("label"), Some(&"override".into()));
    assert_eq!(
        contextual
            .makeOneOn(connection, Attributes::new())
            .await?
            .getAttribute("label"),
        Some(&"step-4-none".into())
    );

    let mixed = Factory::<ExpansionChild>::new(move || label("earlier"))
        .count(9)
        .forEachSequenceEntries(vec![
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
        .sequenceEntries(vec![]);
    assert_eq!(
        mixed
            .makeOneOn(connection, Attributes::new())
            .await?
            .getAttribute("label"),
        Some(&"fixed".into())
    );
    let models = mixed
        .createWithParentOn(connection, &parent, Attributes::new())
        .await?;
    assert_eq!(models.len(), 2);
    assert_eq!(
        models[0].getAttribute("label"),
        Some(&"mixed-1-saved".into())
    );
    assert_eq!(models[1].getAttribute("label"), Some(&"fixed".into()));
    let rows = mixed.rawOn(connection).await?;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["label"], "mixed-3-none".into());
    assert_eq!(rows[1]["label"], "fixed".into());

    let empty = Factory::<ExpansionChild>::new(|| panic!("zero count evaluated the definition"))
        .count(3)
        .forEachSequenceEntries(vec![]);
    assert!(empty.rawOn(connection).await?.is_empty());

    let relation = openrust::database::eloquent::relations::BelongsToMany::<
        ExpansionOwner,
        ExpansionChild,
    >::new("expansion_links", "owner_id", "child_id", "id", "id");
    let parents = Factory::<ExpansionOwner>::new(Attributes::new).hasAttached(
        relation,
        Factory::<ExpansionChild>::new(Attributes::new).count(1),
        vec![label("first"), label("second")],
    );
    parents.createOn(connection).await?;
    parents.createOn(connection).await?;
    assert_eq!(
        connection
            .table("expansion_links")
            .orderBy("owner_id", "asc")
            .pluck("label")
            .await?,
        ["first".into(), "second".into()]
    );
    assert_eq!(
        connection
            .table("expansion_children")
            .orderBy("id", "asc")
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
}

async fn verify_factory_many_count(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();
    schema
        .create("expansion_children", |table| {
            table.id();
        })
        .await?;
    let factory = || Factory::<ExpansionChild>::new(Attributes::new).count(3);
    let batch = factory();
    assert_eq!(batch.rawOn(connection).await?.len(), 3);
    assert_eq!(
        batch
            .makeManyOn(connection, FactoryRecords::Default)
            .await?
            .len(),
        3
    );
    assert_eq!(
        batch
            .makeManyOn(connection, FactoryRecords::Default)
            .await?
            .len(),
        1
    );
    assert_eq!(batch.makeOn(connection).await?.len(), 1);
    assert_eq!(batch.rawOn(connection).await?.len(), 1);
    assert_eq!(connection.table("expansion_children").count("*").await?, 0);

    let explicit = factory();
    assert!(
        explicit
            .makeManyOn(connection, FactoryRecords::Count(0))
            .await?
            .is_empty()
    );
    assert_eq!(explicit.rawOn(connection).await?.len(), 1);

    let create = factory();
    assert_eq!(
        create
            .createManyQuietlyOn(connection, FactoryRecords::Default)
            .await?
            .len(),
        3
    );
    assert_eq!(create.createOn(connection).await?.len(), 1);
    let failed = Factory::<ExpansionChild>::new(Attributes::new)
        .afterMaking(|_| Err(DatabaseError::InvalidQuery("factory count fixture error")))
        .count(3);
    assert!(
        failed
            .makeManyOn(connection, FactoryRecords::Default)
            .await
            .is_err()
    );
    assert_eq!(failed.rawOn(connection).await?.len(), 1);
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
    let mut schema = connection.getSchemaBuilder();
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
    assert!(batch.insertOn(connection, Attributes::new()).await.is_err());
    assert_eq!(
        connection
            .table("factory_insert_projection")
            .count("*")
            .await?,
        0
    );
    factory().insertOn(connection, Attributes::new()).await?;
    let row = connection
        .table("factory_insert_projection")
        .first()
        .await?
        .unwrap();
    assert_eq!(row.get("label"), Some(&"write-read-write-sample".into()));
    assert_eq!(row.get("secret"), Some(&"retained".into()));
    let model = InsertProjection::queryOn(connection)
        .get()
        .await?
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        model.getAttributeValue("options")?,
        Some(Value::Json(json!({"ok":true})))
    );
    assert!(
        matches!(model.getAttributeValue("published_at")?, Some(Value::Text(date)) if date.starts_with("2020-01-02T03:04:05"))
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
    verify_factory_insert_projection(&Root::new().connection(Some(&url), ""))
        .await
        .unwrap();
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
        let bic = unique.swiftBicNumber()?;
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
            let formatted = generator.creditCardNumber(Some(vendor), true, Some("::"))?;
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
    assert!(faker.creditCardNumber(Some("Elo"), false, None).is_err());
    assert!(brazil.creditCardNumber(Some("JCB"), false, None).is_err());
    assert!(faker.creditCardNumber(Some("bogus"), false, None).is_err());
    let start = chrono::Utc::now().timestamp();
    let upper = faker.resolveDateMax("+36 months")?.timestamp() + 1;
    let lower = faker.resolveDateMax("-36 months")?.timestamp() - 1;
    for _ in 0..16 {
        assert!((start..=upper).contains(&faker.creditCardExpirationDate(true)?.timestamp()));
        assert!((lower..=upper).contains(&faker.creditCardExpirationDate(false)?.timestamp()));
    }
    let default_date = faker.creditCardExpirationDateString(true, None)?;
    assert!(chrono::NaiveDate::parse_from_str(&format!("01/{default_date}"), "%d/%m/%y").is_ok());
    assert!(
        faker
            .creditCardExpirationDateString(true, Some("%Q"))
            .is_err()
    );
    let unique = faker.unique(true);
    let mut vendors = std::collections::HashSet::new();
    for _ in 0..6 {
        assert!(vendors.insert(unique.creditCardType()?));
    }
    assert!(unique.creditCardType().is_err());
    let details = unique.creditCardDetails(true)?;
    assert!(vendors.contains(details.r#type));
    assert!(!details.name.is_empty());
    assert!(!details.number.contains('-'));
    assert_eq!(
        serde_json::to_value(&details).unwrap()["expirationDate"],
        details.expiration_date
    );
    assert!(unique.creditCardExpirationDate(true)?.timestamp() >= start);
    assert_eq!(
        unique
            .creditCardExpirationDateString(true, Some("%Y-%m"))?
            .len(),
        7
    );
    let first = unique.creditCardNumber(None, false, None)?;
    assert_ne!(unique.creditCardNumber(None, false, None)?, first);
    Ok(())
}

#[test]
fn faker_regional_bank_accounts_and_routing_follow_provider_contract() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::{Faker, fake};

    for (locale, country) in [
        ("ar_SA", "SA"),
        ("bg_BG", "BG"),
        ("cs_CZ", "CZ"),
        ("da_DK", "DK"),
        ("at_AT", "AT"),
        ("de_AT", "AT"),
        ("de_CH", "CH"),
        ("de_DE", "DE"),
        ("el_CY", "CY"),
        ("el_GR", "GR"),
        ("en_GB", "GB"),
        ("es_ES", "ES"),
        ("fi_FI", "FI"),
        ("fr_BE", "BE"),
        ("fr_CH", "CH"),
        ("fr_FR", "FR"),
        ("he_IL", "IL"),
        ("hr_HR", "HR"),
        ("hu_HU", "HU"),
        ("is_IS", "IS"),
        ("it_CH", "CH"),
        ("it_IT", "IT"),
        ("ka_GE", "GE"),
        ("kk_KZ", "KZ"),
        ("lt_LT", "LT"),
        ("lv_LV", "LV"),
        ("me_ME", "ME"),
        ("nb_NO", "NO"),
        ("nl_BE", "BE"),
        ("nl_NL", "NL"),
        ("pl_PL", "PL"),
        ("pt_BR", "BR"),
        ("pt_PT", "PT"),
        ("ro_MD", "MD"),
        ("ro_RO", "RO"),
        ("sk_SK", "SK"),
        ("sl_SI", "SI"),
        ("sr_Cyrl_RS", "RS"),
        ("sr_Latn_RS", "RS"),
        ("sr_RS", "RS"),
        ("sv_SE", "SE"),
        ("tr_TR", "TR"),
        ("ar_EG", "EG"),
    ] {
        let account = fake(Some(locale))?.bankAccountNumber("", None, None)?;
        assert!(account.starts_with(country), "{locale}: {account}");
        assert_test_iban(&account);
    }
    let uk = fake(Some("en-GB"))?;
    let gb = uk.bankAccountNumber("BANK", None, None)?;
    assert_eq!(&gb[4..8], "BANK");
    assert_eq!(gb.len(), 22);
    let override_account = uk.bankAccountNumber("00", Some("ZZ"), Some(7))?;
    assert_eq!(override_account.len(), 11);
    assert_eq!(&override_account[4..6], "00");
    assert_eq!(
        fake(Some("me-ME"))?
            .bankAccountNumber("", Some("ZZ"), None)?
            .len(),
        22
    );

    let usa = fake(Some("en-US"))?;
    let malaysia = fake(Some("ms-MY"))?;
    let nepal = fake(Some("ne-NP"))?;
    let unique = usa.unique(true);
    let mut routes = std::collections::HashSet::new();
    for _ in 0..128 {
        let account = usa.bankAccountNumber("", None, None)?;
        assert!((5..=17).contains(&account.len()));
        assert!(account.bytes().all(|byte| byte.is_ascii_digit()));
        let account = malaysia.bankAccountNumber("", None, None)?;
        assert!((10..=16).contains(&account.len()));
        assert!(account.bytes().all(|byte| byte.is_ascii_digit()));
        let account = nepal.bankAccountNumber("", None, None)?;
        let digits = if let Some(digits) = account.strip_prefix("00") {
            digits
        } else {
            assert!(account.as_bytes()[0].is_ascii_uppercase());
            &account[1..]
        };
        assert!((9..=20).contains(&account.len()));
        assert!(digits.bytes().all(|byte| (b'1'..=b'9').contains(&byte)));
        let route = unique.bankRoutingNumber()?;
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
    assert_eq!(Faker::calculateRoutingNumberChecksum("02100002")?, 1);
    assert_eq!(Faker::calculateRoutingNumberChecksum("021000021")?, 1);
    assert!(Faker::calculateRoutingNumberChecksum("0210000x").is_err());
    assert!(Faker::calculateRoutingNumberChecksum("123").is_err());
    assert!(usa.bankAccountNumber("00", None, None).is_err());
    for locale in ["en-CA", "ja-JP", "zz-ZZ"] {
        let fallback = fake(Some(locale))?;
        let account = fallback.bankAccountNumber("", None, None)?;
        assert!((5..=17).contains(&account.len()));
        assert!(account.bytes().all(|byte| byte.is_ascii_digit()));
        let route = fallback.unique(true).bankRoutingNumber()?;
        let check = Faker::calculateRoutingNumberChecksum(&route)?;
        assert_eq!(route.len(), 9);
        assert_eq!(route.as_bytes()[8] - b'0', check);
        assert!(fallback.bankAccountNumber("00", None, None).is_err());
    }
    let alias = fake(Some("at-AT"))?;
    assert!(alias.vat(Some(false))?.starts_with("ATU"));
    assert!(alias.bankRoutingNumber().is_err());
    assert!(uk.bankRoutingNumber().is_err());
    assert!(
        fake(Some("zh-Hant"))?
            .bankAccountNumber("", None, None)
            .is_err()
    );
    assert_ne!(
        unique.bankAccountNumber("", None, None)?,
        unique.bankAccountNumber("", None, None)?
    );
    Ok(())
}

#[test]
fn faker_institutions_and_local_swift_follow_provider_contract() -> DatabaseResult<()> {
    use openrust::database::eloquent::factories::fake;

    for (locale, distinct, representative) in [
        ("de_DE", 60, "Bank 1 Saar"),
        ("el_CY", 12, "Τράπεζα Κύπρου"),
        ("ka_GE", 19, "ბანკი რესპუბლიკა"),
        ("kk_KZ", 2, "Қазкоммерцбанк"),
        ("pl_PL", 68, "Narodowy Bank Polski"),
        (
            "pt_BR",
            59,
            "BADESUL Desenvolvimento S.A. – Agência de Fomento/RS",
        ),
        ("ru_RU", 788, "Новый Промышленный Банк"),
        ("th_TH", 22, "ธนาคารแห่งประเทศไทย"),
        ("uk_UA", 89, "А-Банк"),
        ("zh_CN", 19, "渤海银行"),
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
    assert_eq!(unique.currencySymbol()?, "RM");
    assert!(unique.currencySymbol().is_err());
    assert!(!unique.insurance()?.is_empty());
    let nepali = fake(Some("ne-NP"))?;
    for _ in 0..64 {
        assert!(!malaysia.bank()?.is_empty());
        assert!(!malaysia.localBank()?.is_empty());
        assert!(!malaysia.foreignBank()?.is_empty());
        assert!(!malaysia.governmentBank()?.is_empty());
        let swift = malaysia.swiftCode()?;
        assert_eq!(swift.len(), 11);
        assert_eq!(&swift[4..6], "MY");
        let swift = nepali.swiftCode()?;
        assert!(matches!(swift.len(), 8 | 11));
        assert_eq!(&swift[4..6], "NP");
        assert!(
            nepali
                .microFinance()?
                .ends_with(" Laghubitta Bittiya Sanstha")
        );
        assert!(!nepali.commercialBank()?.is_empty());
        assert!(!nepali.developmentBank()?.is_empty());
        assert!(!nepali.financeCompany()?.is_empty());
        assert!(!nepali.digitalWallet()?.is_empty());
    }
    assert_ne!(unique.swiftCode()?, unique.swiftCode()?);
    assert!(malaysia.commercialBank().is_err());
    assert!(nepali.localBank().is_err());
    let traditional = fake(Some("zh-Hant"))?;
    assert!(traditional.bank().is_err());
    assert!(traditional.swiftCode().is_err());
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
        verify_luhn(&italian.unique(false).vatId()?[2..]);
        let be = belgian.vat(Some(false))?;
        assert_eq!(
            be[10..].parse::<u32>().unwrap(),
            97 - be[2..10].parse::<u32>().unwrap() % 97
        );
        let branch = uk.vatWithType(Some("branch"))?;
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
        let government = uk.vatWithType(Some("gov"))?;
        assert!(government.starts_with("GBGD"));
        assert_eq!(government.len(), 7);
        assert!(government[4..].parse::<u16>().unwrap() < 500);
        let health = uk.vatWithType(Some("health"))?;
        assert!(health.starts_with("GBHA"));
        assert!((500..=999).contains(&health[4..].parse::<u16>().unwrap()));
    }
    assert_eq!(Faker::calculateModulus97("0000000", false)?, 0);
    assert_eq!(Faker::calculateModulus97("0000000", true)?, 42);
    assert!(Faker::calculateModulus97("123456x", true).is_err());
    assert!(Faker::calculateModulus97("123", true).is_err());
    assert!(
        uk.unique(true)
            .vatWithType(Some("branch"))?
            .starts_with("GB")
    );
    assert!(fake(Some("nl-NL"))?.unique(true).btw()?.starts_with("NL"));
    let taiwanese = fake(Some("zh-Hant"))?.vat(None)?;
    assert_eq!(taiwanese.len(), 8);
    assert!(taiwanese.bytes().all(|digit| digit.is_ascii_digit()));
    assert!(fake(Some("en-US"))?.vat(None).is_err());
    assert!(french.vatWithType(Some("gov")).is_err());
    Ok(())
}
async fn verify_deferred_factory_attributes(connection: &Connection) -> DatabaseResult<()> {
    let mut schema = connection.getSchemaBuilder();
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
            table.unsignedBigInteger("owner_id").nullable();
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
        .parentAttributeWith("owner_id", |values| {
            let name = values["derived_name"].clone();
            Factory::<FactoryOwner>::new(move || {
                Attributes::from([("generated_name".into(), name.clone())])
            })
        });
    let preview = factory
        .rawWithOn(
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
        .createOneOn(
            connection,
            Attributes::from([("generated_name".into(), "saved".into())]),
        )
        .await?;
    assert_eq!(
        made.getAttribute("derived_name"),
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
        .createWithParentOn(
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
            .all(|model| model.getAttribute("derived_name") == Some(&"override".into()))
    );
    assert_eq!(
        connection.table("seed_factory_children").count("*").await?,
        3
    );
    assert_eq!(connection.table("seed_factory_owners").count("*").await?, 1);

    let disabled = Factory::<FactoryChild>::new(Attributes::new)
        .attribute("computed", |_| Ok("kept".into()))
        .parentAttributeWith::<FactoryOwner>("owner_id", |_| {
            panic!("excluded owner factory was called")
        })
        .withoutParents(Vec::new());
    let row = disabled.rawOn(connection).await?;
    assert_eq!(row["computed"], "kept".into());
    assert_eq!(row["owner_id"], Value::Null);

    let failed = Factory::<FactoryChild>::new(Attributes::new)
        .attribute("computed", |_| {
            Err(DatabaseError::InvalidQuery("probe failed"))
        })
        .parentAttributeWith::<FactoryOwner>("owner_id", |_| panic!("later resolver was called"));
    assert!(matches!(
        failed.createOn(connection).await,
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
    let db = Root::new().connection(Some(&url), "");
    verify_deferred_factory_attributes(&db).await.unwrap();
}

#[tokio::test]
async fn factory_model_keys_and_relation_cache_follow_lifecycle_on_mysql() {
    let Ok(url) = std::env::var("OPENRUST_TEST_MYSQL_URL") else {
        assert!(std::env::var_os("OPENRUST_REQUIRE_MYSQL").is_none());
        return;
    };
    let db = Root::new().connection(Some(&url), "");
    verify_factory_relation_cache(&db).await.unwrap();
}
async fn verify_factory_relation_cache(connection: &Connection) -> DatabaseResult<()> {
    use openrust::database::eloquent::{
        RelatedCollection,
        relations::{BelongsTo, HasMany},
    };

    let mut schema = connection.getSchemaBuilder();
    schema
        .create("seed_factory_owners", |table| {
            table.id();
        })
        .await?;
    schema
        .create("seed_factory_children", |table| {
            table.id();
            table.unsignedBigInteger("owner_id").nullable();
        })
        .await?;
    let existing = Factory::<FactoryOwner>::new(Attributes::new)
        .createOn(connection)
        .await?;
    let child = Factory::<FactoryChild>::new(Attributes::new)
        .modelAttribute("owner_id", &existing)
        .makeOn(connection)
        .await?;
    assert_eq!(child.getAttribute("owner_id"), existing.getKey());
    let unsaved = FactoryOwner::from_state(Default::default());
    let raw = Factory::<FactoryChild>::new(Attributes::new)
        .modelAttribute("owner_id", &unsaved)
        .rawOn(connection)
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
        .afterMaking(move |owner| {
            owner.setRelation::<Children>("children", RelatedCollection::fromShared(Vec::new()));
            owner.setRelation::<Children>(
                "known",
                RelatedCollection::fromShared(vec![known_child.clone()]),
            );
            owner.setRelation::<Single>("single", None);
            Ok(())
        })
        .has(
            Children::new("id", "owner_id"),
            Factory::<FactoryChild>::new(Attributes::new)
                .afterMaking(|child| {
                    type Nested = HasMany<FactoryChild, FactoryChild>;
                    child
                        .setRelation::<Nested>("nested", RelatedCollection::fromShared(Vec::new()));
                    Ok(())
                })
                .afterCreating(|child| {
                    assert!(!child.relationLoaded("nested"));
                    Ok(())
                })
                .count(2),
        )
        .afterCreating(|owner| {
            assert!(!owner.relationLoaded("children"));
            assert!(owner.relationLoaded("known"));
            assert!(owner.relationLoaded("single"));
            Ok(())
        });
    let preview = factory.makeOn(connection).await?;
    assert!(preview.relationLoaded("children"));
    let mut owner = factory.createOn(connection).await?;
    let relation = Children::new("id", "owner_id");
    let children = owner.getRelationOrLoad("children", &relation).await?;
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
    verify_factory_pivot_touch(&Root::new().connection(Some(&url), ""))
        .await
        .unwrap();
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
        .withPivot(["label", "meta"])
}

pivot_model!(
    PivotOwner,
    ModelMetadata::new("factory_touch_owners").timestamps(None, None),
    fn namedRelations() -> NamedRelations<Self> {
        NamedRelations::default().nested("members", pivot_members)
    },
    fn touches() -> openrust::database::eloquent::Touches<Self> {
        openrust::database::eloquent::Touches::default().belongsToMany(pivot_members())
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
    let mut schema = db.getSchemaBuilder();
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
            table.unsignedBigInteger("owner_id");
            table.unsignedBigInteger("member_id");
            table.string("label", 32);
            table.json("meta").nullable();
        })
        .await?;
    let member = || Factory::<PivotMember>::new(Attributes::new).set("updated_at", OLD);
    let first = member().createOn(db).await?;
    let second = member().createOn(db).await?;
    let third = member().createOn(db).await?;
    let untouched = third.getKey().cloned().unwrap();
    let prior = RelatedCollection::fromShared(vec![std::sync::Arc::new(PivotMember::from_state(
        first.state().clone(),
    ))]);
    let owner = Factory::<PivotOwner>::new(Attributes::new)
        .afterMaking(move |owner| {
            owner.setRelation::<PivotMembers>("members", prior.clone());
            Ok(())
        })
        .hasAttachedNamed::<PivotMember, PivotMembers>(
            "members",
            vec![first, second],
            vec![
                Attributes::from([("label".into(), "first".into())]),
                Attributes::from([("label".into(), "second".into())]),
            ],
        )
        .afterCreating(|owner| {
            let members = owner
                .getRelation::<RelatedCollection<PivotMember>>("members")
                .unwrap();
            assert_eq!(members.len(), 2);
            let mut labels = Vec::new();
            for member in members.iter() {
                assert_ne!(member.getAttribute("updated_at"), Some(&OLD.into()));
                let pivot = member.getRelation::<CustomFactoryPivot>("pivot").unwrap();
                labels.push(pivot.getAttribute("label").cloned().unwrap());
            }
            assert!(labels.contains(&"first".into()));
            assert!(labels.contains(&"second".into()));
            Ok(())
        })
        .createOn(db)
        .await?;
    assert!(owner.relationLoaded("members"));
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
        .hasAttachedNamed::<PivotMember, PivotMembers>(
            "members",
            member(),
            Attributes::from([
                ("label".into(), "new".into()),
                ("meta".into(), Value::Json(json!({"shared": true}))),
            ]),
        )
        .createOn(db)
        .await?;
    let members = appended
        .getRelation::<RelatedCollection<PivotMember>>("members")
        .unwrap();
    assert_eq!(members.len(), 1);
    assert_ne!(members[0].getAttribute("updated_at"), Some(&OLD.into()));
    let pivot = members[0]
        .getRelation::<CustomFactoryPivot>("pivot")
        .unwrap();
    assert_eq!(
        pivot.getAttributeValue("meta")?,
        Some(Value::Json(json!({"shared": true})))
    );
    let before = db.table("factory_touch_members").count("*").await?;
    let generated = Factory::<PivotOwner>::new(Attributes::new)
        .hasAttachedNamed::<PivotMember, PivotMembers>(
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
        .createOn(db)
        .await?;
    let members = pivot_members().getResults(&generated).await?;
    let mut meta = BTreeMap::new();
    for member in members.iter() {
        let pivot = member.getRelation::<CustomFactoryPivot>("pivot").unwrap();
        let Some(Value::Text(label)) = pivot.getAttribute("label") else {
            panic!("expected a pivot label");
        };
        meta.insert(label.clone(), pivot.getAttributeValue("meta")?);
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
                faker.phoneNumber(),
                faker.safeEmail(),
                faker.regexify("([A-Z]{2}|[0-9]{3})")?,
                faker.safeHexColor(),
                faker.chrome(),
                faker.uuid().to_string(),
                faker.numerify(Some("#%#-###")),
                faker.randomHtml(Some(2), Some(3))?,
                faker
                    .dateTimeBetweenExpressions("2020-01-01", "2020-02-01")?
                    .to_rfc3339(),
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
    assert_eq!(faker.unique(false).numberBetween(1, 1)?, 1);
    faker.seed(5);
    assert!(faker.unique(false).numberBetween(1, 1).is_err());
    assert_eq!(faker.unique(true).numberBetween(1, 1)?, 1);
    Ok(())
}

#[test]
fn faker_providers_resolve_latest_case_insensitive_callbacks() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    let independent = Faker::new("en-US")?;
    faker.seed(42);
    independent.seed(42);
    assert_eq!(faker.format("name", &[])?, Value::from(independent.name()));

    faker.addProvider(
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
    let resolved = faker.getFormatter("NAME")?;
    faker.addProvider(FakerProvider::new().formatter("NaMe", |_, _| Ok("latest".into())));
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
    faker.addProvider(FakerProvider::new().formatter("lastNameEn", |_, _| Ok("Lin".into())));
    assert!(faker.companyEn()?.starts_with("Lin"));
    assert!(
        matches!(faker.format("companyEn", &[])?, Value::Text(name) if name.starts_with("Lin"))
    );
    faker.addProvider(FakerProvider::new().formatter("lastNameEn", |_, _| Ok(false.into())));
    assert!(matches!(
        faker.companyEn(),
        Err(DatabaseError::InvalidQuery(
            "Faker lastNameEn must return text"
        ))
    ));
    faker.addProvider(FakerProvider::new().formatter("lastNameEn", |_, _| {
        Err(DatabaseError::UnknownFakerFormatter("upstreamName".into()))
    }));
    assert!(matches!(
        faker.companyEn(),
        Err(DatabaseError::UnknownFakerFormatter(name)) if name == "upstreamName"
    ));
    Ok(())
}

#[test]
fn faker_named_numeric_and_pattern_arguments_use_builtin_contracts() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    faker.addProvider(
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
    let regex = faker.getFormatter("regexify")?;
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
fn faker_named_company_and_person_reuse_regional_generators() -> DatabaseResult<()> {
    let malaysia = Faker::new("ms-MY")?;
    malaysia.seed(94);
    let name = malaysia.firstNameMaleChinese()?;
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
    let slogan = france.catchPhrase()?;
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
    let default = poland.licensePlate()?;
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
            Faker::peninsularPrefix as fn(&Faker) -> DatabaseResult<&'static str>,
        ),
        ("sabahPrefix", Faker::sabahPrefix),
        ("sarawakPrefix", Faker::sarawakPrefix),
        ("specialPrefix", Faker::specialPrefix),
        ("validAlphabet", Faker::validAlphabet),
    ] {
        malaysia.seed(94);
        let expected = generate(&malaysia)?;
        malaysia.seed(94);
        assert_eq!(malaysia.format(name, &[])?, expected.into());
    }
    malaysia.seed(94);
    let expected = malaysia.jpjNumberPlate()?;
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
            Faker::mobileNumber as fn(&Faker) -> DatabaseResult<String>,
            Faker::mobileNumberWithOptions as fn(&Faker, bool, bool) -> DatabaseResult<String>,
        ),
        (
            "fixedLineNumber",
            Faker::fixedLineNumber,
            Faker::fixedLineNumberWithOptions,
        ),
        (
            "voipNumber",
            Faker::voipNumber,
            Faker::voipNumberWithOptions,
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
            Faker::cellphoneNumber as fn(&Faker, Option<bool>) -> DatabaseResult<String>,
        ),
        ("landlineNumber", Faker::landlineNumber),
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
            Faker::phoneNumberWithExtension as fn(&Faker) -> DatabaseResult<String>,
        ),
        ("ko-KR", "CELLphoneNUMBER", Faker::cellPhoneNumber),
        (
            "fr-FR",
            "phoneNumber06WithSeparator",
            Faker::phoneNumber06WithSeparator,
        ),
        (
            "en-SG",
            "tollFreeInternationalNumber",
            Faker::tollFreeInternationalNumber,
        ),
        ("el-GR", "premiumRateNumber", Faker::premiumRateNumber),
        (
            "fi-FI",
            "e164MobileNetworkAreaCode",
            Faker::e164MobileNetworkAreaCode,
        ),
        ("en-US", "areaCode", Faker::areaCode),
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
    let paths: [Value; 2] = [
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
        matches!(faker.formatAsync("image", &invalid).await, Err(DatabaseError::FakerIo(error)) if error.kind() == std::io::ErrorKind::InvalidInput)
    );
    faker.addProvider(FakerProvider::new().formatter("image", |_, _| Ok("custom.png".into())));
    assert_eq!(faker.formatAsync("IMAGE", &[]).await?, "custom.png".into());
    Ok(())
}

#[test]
fn faker_named_regional_address_person_and_payment_follow_the_locale() -> DatabaseResult<()> {
    let ru = Faker::new("ru-RU")?;
    ru.seed(331);
    let surname = ru.lastNameWithGender(Some("female"));
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
    ja.addProvider(FakerProvider::new().formatter("prefecture", |_, _| Ok("override".into())));
    assert_eq!(ja.format("PREFECTURE", &[])?, "override".into());
    Ok(())
}

#[test]
fn faker_named_text_and_internet_reuse_typed_generators() -> DatabaseResult<()> {
    let faker = Faker::new("en-US")?;
    faker.seed(91);
    let expected = faker.sentenceWithWordCount(2, false);
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
        faker.getFormatter("IPv4")?(&[])?,
        Value::Text(ip) if ip.parse::<std::net::Ipv4Addr>().is_ok()
    ));
    assert!(matches!(
        faker.format("userAgent", &[])?,
        Value::Text(agent) if agent.contains('/')
    ));
    faker.addProvider(FakerProvider::new().formatter("word", |_, _| Ok("override".into())));
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
    use chrono::{DateTime, Datelike as _, Utc};

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
    assert!((-86_400..=0).contains(&DateTime::parse_from_rfc3339(&sample).unwrap().timestamp()));

    let Value::Text(week) = date.format("dateTimeThisWeek", &["monday this week".into()])? else {
        panic!("expected a date string");
    };
    assert_eq!(
        DateTime::parse_from_rfc3339(&week).unwrap().weekday(),
        chrono::Weekday::Mon
    );
    let Value::Text(month) =
        date.format("dateTimeThisMonth", &["first day of this month".into()])?
    else {
        panic!("expected a date string");
    };
    assert_eq!(DateTime::parse_from_rfc3339(&month).unwrap().day(), 1);
    let Value::Text(year) = date.format("dateTimeThisYear", &["first day of january".into()])?
    else {
        panic!("expected a date string");
    };
    assert_eq!(DateTime::parse_from_rfc3339(&year).unwrap().ordinal(), 1);

    let year = Utc::now().year();
    for (formatter, width) in [("dateTimeThisDecade", 10), ("dateTimeThisCentury", 100)] {
        let first_year = year.div_euclid(width) * width;
        let latest = format!("{first_year}-01-02T00:00:00+00:00");
        let Value::Text(sample) = date.format(formatter, &[latest.clone().into()])? else {
            panic!("expected a date string");
        };
        let sample = DateTime::parse_from_rfc3339(&sample).unwrap();
        assert_eq!(sample.year(), first_year);
        assert!(sample <= DateTime::parse_from_rfc3339(&latest).unwrap());
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
    let typed = faker.filePath().unwrap();
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
    let previous = faker.getFormatter("numberBetween")?;
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
    faker.addExtension(
        NUMBER,
        FakerProvider::new().formatter("numberBetween", |_, _| Ok(12.into())),
    );
    assert_eq!(
        faker.format("numberBetween", &[3.into(), 3.into()])?,
        12.into()
    );
    assert_eq!(previous(&[3.into(), 3.into()])?, 3.into());
    assert_eq!(faker.numberBetween(3, 3), 3);
    assert!(matches!(
        faker.format("randomDigit", &[]),
        Err(DatabaseError::UnknownFakerFormatter(name)) if name == "randomDigit"
    ));

    faker.addProvider(FakerProvider::new().formatter(CODE, |_, _| Ok("provider".into())));
    assert!(
        matches!(faker.format(CODE, &[]), Err(DatabaseError::UnknownFakerExtension(id)) if id == CUSTOM)
    );
    faker.addExtension(
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
        faker.uniqueWithRetries(false, 1).format(CODE, &[])?,
        "extension".into()
    );
    assert!(matches!(
        faker.uniqueWithRetries(false, 1).format(CODE, &[]),
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
    faker.addProvider(
        FakerProvider::new().formatter("constant", move |_, arguments| {
            counter.fetch_add(1, Ordering::Relaxed);
            Ok(arguments.first().cloned().unwrap_or_else(|| 7.into()))
        }),
    );
    let unique = faker.uniqueWithRetries(false, 2);
    assert_eq!(unique.format("constant", &[])?, 7.into());
    assert_eq!(unique.format("constant", &["7".into()])?, "7".into());
    assert_eq!(unique.format("Constant", &[])?, 7.into());
    assert!(matches!(
        unique.format("constant", &[7.into()]),
        Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 6);

    faker.addProvider(FakerProvider::new().formatter("constant", |_, _| Ok(7.into())));
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
    faker.addProvider(FakerProvider::new().formatter("sequence", move |_, _| {
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
    faker.addProvider(
        FakerProvider::new().formatter("fixed", |_, _| Ok(Value::Json(json!({"ok":true})))),
    );
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| faker.uniqueWithRetries(false, 1).format("fixed", &[]));
        let second = scope.spawn(|| faker.uniqueWithRetries(false, 1).format("fixed", &[]));
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
            let alias = fake(Some("zh-Hant-TW"))?;
            let independent = openrust::database::eloquent::factories::Faker::new("zh-TW")?;
            assert!(Arc::ptr_eq(&default, &alias));
            assert!(matches!(
                default.format("registeredHere", &[]),
                Err(DatabaseError::UnknownFakerFormatter(_))
            ));
            default.addProvider(
                FakerProvider::new().formatter("registeredHere", |_, _| Ok(21.into())),
            );
            assert_eq!(alias.format("registeredHere", &[])?, Value::from(21));
            assert!(fake(Some("en-US"))?.format("registeredHere", &[]).is_err());
            default.seed(813);
            independent.seed(813);
            assert_eq!(default.address(), independent.address());
            assert_eq!(alias.phoneNumber(), independent.phoneNumber());
            fake(Some("en-US"))?.seed(814);
            assert_eq!(default.company(), independent.company());
            let value = |generator: &openrust::database::eloquent::factories::Faker| {
                generator.unique(false).numberBetween(760, 760)
            };
            assert_eq!(value(&default)?, 760);
            assert!(value(&alias).is_err());
            assert_eq!(value(fake(Some("en-US"))?.as_ref())?, 760);
            let factory = || {
                Factory::<FactoryChild>::tryWithFaker(move |generator| {
                    Ok(Attributes::from([(
                        "generated_score".into(),
                        value(generator)?.into(),
                    )]))
                })
            };
            let connection = context.connection();
            connection
                .getSchemaBuilder()
                .create("seed_factory_children", |table| {
                    table.id();
                    table.integer("generated_score");
                })
                .await?;
            assert!(factory().createOn(connection).await?.state().exists());
            assert!(matches!(
                factory().createOn(connection).await,
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
                .getSchemaBuilder()
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
            .withConfig(load)
            .withProviders(vec![Box::new(Provider)])
            .create()
            .unwrap();
        app.boot().await.unwrap();
        // The fixture defaults to SQLite unless the test explicitly installs its MySQL connection.
        let config = app
            .container()
            .get::<dyn openrust::contracts::config::Repository>("config")
            .unwrap();
        config
            .set("database.connections.faker_test", json!({"url":url}))
            .unwrap();
        let mut arguments = args.clone();
        arguments[1] = "--database=faker_test".into();
        assert_eq!(app.handleCommand(&arguments).await.unwrap(), 0);
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
        .map(|_| unique.mimeType().unwrap())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(types.len(), 461);
    assert!(types.contains("application/json"));
    assert!(unique.mimeType().is_err());
    let extension = unique.fileExtension().unwrap();
    assert!(!extension.is_empty() && !extension.starts_with('.'));
    let id = unique.uuid().unwrap();
    assert_ne!(unique.uuid().unwrap(), id);
    Ok(())
}
