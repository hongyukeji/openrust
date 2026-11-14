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
            factories::{Factory, FactoryParentSelector, FactoryPivot, FactoryRecords, fake},
            relations::{MorphMany, MorphMap, MorphMapEntry, MorphTo, MorphToMany},
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
            DB::table("seed_items")?
                .insert(&[Record::from([("name".into(), name)])])
                .await?;
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
                Some(&"many-2".into())
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
            assert_eq!(created.len(), 3);
            assert!(created.iter().all(
                |model| model.getAttribute("generated_name") == Some(&"deferred-batch".into())
            ));
            assert!(deferred_one().await?.state().exists());
            assert!(deferred_one().await?.state().exists());
            assert_eq!(connection.table("seed_factory_children").delete().await?, 5);
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
            assert_eq!(defaults.len(), 3);
            assert_eq!(
                *phases.lock().unwrap(),
                [
                    "making", "created", "making", "created", "making", "created"
                ]
            );
            assert_eq!(
                defaults[2].getAttribute("generated_name"),
                Some(&"many-2".into())
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
            assert_eq!(connection.table("seed_factory_children").delete().await?, 9);
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
            let mut faker = fake(None)?;
            let mut other = fake(Some("fr-fr"))?;
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
            assert_eq!(faker.amPm(Some(epoch)), "am");
            assert_eq!(faker.dayOfMonth(Some(epoch)), "01");
            assert_eq!(faker.dayOfWeek(Some(epoch)), "Thursday");
            assert_eq!(faker.month(Some(epoch)), "01");
            assert_eq!(faker.monthName(Some(epoch)), "January");
            assert_eq!(faker.year(Some(epoch)), "1970");
            let calendar_unique = faker.unique(false);
            assert_eq!(calendar_unique.dateTimeAD(Some(first_ad))?, first_ad);
            assert_eq!(calendar_unique.amPm(Some(epoch))?, "am");
            assert_eq!(calendar_unique.dayOfMonth(Some(epoch))?, "01");
            assert_eq!(calendar_unique.dayOfWeek(Some(epoch))?, "Thursday");
            assert_eq!(calendar_unique.month(Some(epoch))?, "01");
            assert_eq!(calendar_unique.monthName(Some(epoch))?, "January");
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
            assert_eq!(faker.unique(false).dateTimeBetween(start, start)?, start);
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
            assert_ne!(
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
                SeederEntry::new("FirstSeeder", &Insert("first")),
                SeederEntry::new("SecondSeeder", &Insert("second")),
                SeederEntry::new("SilentSeeder", &Insert("silent")),
                SeederEntry::new("ParameterSeeder", &Insert("fallback")),
                SeederEntry::new("FailingSeeder", &Failing),
                SeederEntry::new("RecursiveSeeder", &Recursive),
                SeederEntry::new("PolymorphicFactories", &PolymorphicFactories),
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
    assert_eq!(db.table("seed_factory_owners").count("*").await.unwrap(), 4);
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
    success(command(&["db:seed"]));
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
