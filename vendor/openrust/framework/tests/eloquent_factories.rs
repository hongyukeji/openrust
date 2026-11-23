use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use openrust::database::eloquent::RelatedCollection;
use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseManager, Value,
        eloquent::{
            Attributes, Collection, Model, ModelMetadata, ModelState, NamedRelations,
            factories::{Factory, FactoryDefinition, FactoryRecords},
            relations::{BelongsTo, BelongsToMany, HasMany, HasOne, MorphMany, Pivot, Relation},
        },
    },
};
use serde_json::{Value as Json, json};

struct Author(ModelState);
struct Entry(ModelState);

impl Model for Author {
    fn named_relations() -> NamedRelations<Self> {
        NamedRelations::default()
            .nested("entries", || {
                HasMany::<Author, Entry>::new("id", "author_id")
            })
            .nested("entry", || HasOne::<Author, Entry>::new("id", "author_id"))
    }

    fn automatically_eager_load_relationships() -> bool {
        true
    }

    fn new_collection(models: Vec<Self>) -> Collection<Self> {
        Collection::from_entries(
            models
                .into_iter()
                .enumerate()
                .map(|(index, model)| (format!("author-{index}"), model)),
        )
    }

    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata =
            ModelMetadata::new("factory_authors").timestamps(None, None);
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

impl Model for Entry {
    fn new_collection(mut models: Vec<Self>) -> Collection<Self> {
        for model in &mut models {
            if model.get_attribute("name") == Some(&"explicit child".into()) {
                model.set_attribute("name", "collection hook ran");
            }
        }
        Collection::new(models)
    }

    fn named_relations() -> NamedRelations<Self> {
        NamedRelations::default().nested("author", || {
            BelongsTo::<Entry, Author>::new("author_id", "id")
        })
    }

    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata =
            ModelMetadata::new("factory_entries").timestamps(None, None);
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

fn authors() -> Factory<Author> {
    Factory::new(|| Attributes::from([("name".into(), "author".into())]))
}

fn entries() -> Factory<Entry> {
    Factory::new(|| Attributes::from([("name".into(), "entry".into())]))
}

async fn verify(config: Json, id: &str) {
    let config = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"factories", "connections":{"factories":config}}),
    )]));
    let database = DatabaseManager::new(Arc::new(config), ".");
    let connection = database.connection(None).unwrap();
    for table in ["factory_links", "factory_entries", "factory_authors"] {
        connection
            .statement(&format!("DROP TABLE IF EXISTS {table}"), &[])
            .await
            .unwrap();
    }
    for query in [
        format!("CREATE TABLE factory_authors (id {id}, name VARCHAR(80) NOT NULL)"),
        format!(
            "CREATE TABLE factory_entries (id {id}, author_id BIGINT NOT NULL, name VARCHAR(80) NOT NULL, owner_type VARCHAR(40) NULL)"
        ),
    ] {
        connection.statement(&query, &[]).await.unwrap();
    }

    connection
        .statement("CREATE TABLE factory_links (author_id BIGINT NOT NULL, entry_id BIGINT NOT NULL, label VARCHAR(80) NOT NULL)", &[])
        .await
        .unwrap();

    let previews = authors()
        .state(|_| Attributes::from([("name".into(), "preview".into())]))
        .count(2)
        .make_on(&connection)
        .await
        .unwrap();
    assert!(previews.get_by_key("author-0").is_some());
    assert!(previews.get_by_key("author-1").is_some());
    assert!(previews.iter().all(|author| !author.state().exists()));
    assert_eq!(Author::query_on(&connection).get().await.unwrap().len(), 0);

    let author = authors()
        .after_making(|author| {
            assert!(!author.state().exists());
            author.set_attribute("name", "made");
            Ok(())
        })
        .after_creating(|author| {
            assert!(author.state().exists());
            Ok(())
        })
        .has(
            HasMany::new("id", "author_id"),
            entries()
                .sequence(|index| {
                    Attributes::from([("name".into(), format!("entry {index}").into())])
                })
                .count(2),
        )
        .create_on(&connection)
        .await
        .unwrap();
    assert_eq!(author.get_attribute("name"), Some(&Value::from("made")));
    let related: RelatedCollection<Entry> = HasMany::new("id", "author_id")
        .constrain(|query| query.order_by("id", "asc"))
        .get_results(&author)
        .await
        .unwrap();
    assert_eq!(related.len(), 2);
    assert_eq!(related[1].get_attribute("name"), Some(&"entry 1".into()));

    let author_id = author.get_key().cloned();
    let entry = entries()
        .count(1)
        .for_existing_inferred::<_, BelongsTo<Entry, Author>>(author)
        .create_one_on(&connection, Attributes::new())
        .await
        .unwrap();
    assert_eq!(entry.get_attribute("author_id"), author_id.as_ref());
    let entry = entries()
        .count(1)
        .for_inferred::<_, BelongsTo<Entry, Author>>(authors())
        .create_one_on(&connection, Attributes::new())
        .await
        .unwrap();
    assert!(entry.get_attribute("author_id").is_some());
    assert_eq!(Author::query_on(&connection).get().await.unwrap().len(), 2);
    let links = || {
        BelongsToMany::<Author, Entry>::new("factory_links", "author_id", "entry_id", "id", "id")
    };
    let linked = authors()
        .count(1)
        .has_attached(
            links().constrain(|query| {
                query.with_attributes(
                    Attributes::from([("name".into(), "attachment default".into())]),
                    false,
                )
            }),
            entries()
                .r#for(BelongsTo::new("author_id", "id"), authors())
                .state(|attributes| {
                    assert_eq!(attributes.get("name"), Some(&"attachment default".into()));
                    Attributes::from([("name".into(), "explicit attachment".into())])
                })
                .count(1),
            Attributes::from([("label".into(), "featured".into())]),
        )
        .create_on(&connection)
        .await
        .unwrap();
    let attached = links()
        .with_pivot(["label"])
        .get_results(&linked[0])
        .await
        .unwrap();
    assert_eq!(attached.len(), 1);
    assert_eq!(
        attached[0].get_attribute("name"),
        Some(&"explicit attachment".into())
    );
    assert_eq!(
        attached[0]
            .get_relation::<Pivot>("pivot")
            .unwrap()
            .get_attribute("label"),
        Some(&"featured".into())
    );

    assert!(
        authors()
            .count(0)
            .create_on(&connection)
            .await
            .unwrap()
            .is_empty()
    );
    let stages = Arc::new(Mutex::new(Vec::new()));
    let observe = |stage| {
        let stages = stages.clone();
        move |_: &mut Author| {
            stages.lock().unwrap().push(stage);
            Ok(())
        }
    };
    let child_stages = stages.clone();
    let child_factory = entries()
        .state(|attributes| {
            assert_eq!(attributes.get("name"), Some(&"relation default".into()));
            Attributes::from([
                ("name".into(), "explicit child".into()),
                ("author_id".into(), 999.into()),
            ])
        })
        .after_making(|child| {
            assert_ne!(child.get_attribute("author_id"), Some(&999.into()));
            assert!(child.get_attribute("author_id").is_some());
            assert_eq!(child.get_attribute("name"), Some(&"explicit child".into()));
            Ok(())
        })
        .after_creating(move |child| {
            assert!(child.get_attribute("author_id").is_some());
            child_stages.lock().unwrap().push("child");
            Ok(())
        });

    let batch = authors()
        .count(5)
        .after_making(observe("make"))
        .after_creating(observe("created"))
        .has(
            HasMany::new("id", "author_id").constrain(|query| {
                query.with_attributes(
                    Attributes::from([("name".into(), "relation default".into())]),
                    false,
                )
            }),
            child_factory,
        )
        .sequence(|index| Attributes::from([("name".into(), format!("batch {index}").into())]))
        .count(2)
        .create_on(&connection)
        .await
        .unwrap();
    assert_eq!(
        *stages.lock().unwrap(),
        ["make", "make", "child", "child", "created", "created"]
    );
    let before = Author::query_on(&connection).get().await.unwrap().len();
    let failed = authors()
        .after_making(|author| {
            if author.get_attribute("name") == Some(&"failure 1".into()) {
                return Err(openrust::database::DatabaseError::InvalidQuery(
                    "rejected draft",
                ));
            }
            Ok(())
        })
        .count(2)
        .create_with_state_on(&connection, {
            let mut index = 0;
            move |_| {
                let attributes =
                    Attributes::from([("name".into(), format!("failure {index}").into())]);
                index += 1;
                attributes
            }
        })
        .await;
    assert!(failed.is_err());
    assert_eq!(
        Author::query_on(&connection).get().await.unwrap().len(),
        before
    );
    for (index, model) in batch.iter().enumerate() {
        assert!(model.state().exists());
        assert_eq!(
            model.get_attribute("name"),
            Some(&format!("batch {index}").into())
        );
        assert_eq!(
            batch
                .get_by_key(format!("author-{index}"))
                .unwrap()
                .get_key(),
            model.get_key()
        );
    }

    let relation = HasMany::<Author, Entry>::new("id", "author_id");
    let sequence = authors()
        .count(7)
        .for_each_sequence(vec![
            Attributes::from([("name".into(), "one".into())]),
            Attributes::from([("name".into(), "two".into())]),
        ])
        .make_on(&connection)
        .await
        .unwrap();
    assert_eq!(sequence.len(), 2);
    assert_eq!(sequence[0].get_attribute("name"), Some(&"one".into()));
    assert_eq!(sequence[1].get_attribute("name"), Some(&"two".into()));
    let crossed = authors()
        .count(3)
        .cross_join_sequence(vec![
            vec![Attributes::new(), Attributes::new()],
            vec![Attributes::from([("name".into(), "crossed".into())])],
        ])
        .unwrap()
        .attribute("derived_name", |attributes| {
            assert_eq!(attributes.get("name"), Some(&"crossed".into()));
            Ok("derived".into())
        })
        .make_on(&connection)
        .await
        .unwrap();
    assert_eq!(crossed.len(), 3);
    assert!(
        crossed
            .iter()
            .all(|author| author.get_attribute("derived_name") == Some(&"derived".into()))
    );

    let entries_before = connection
        .table("factory_entries")
        .count("*")
        .await
        .unwrap();
    let titles = ["first entry", "second entry", "third entry"];
    let named = authors()
        .count(2)
        .has_inferred::<_, HasMany<Author, Entry>>(
            entries().for_each_sequence(
                titles
                    .into_iter()
                    .map(|name| Attributes::from([("name".into(), name.into())]))
                    .collect(),
            ),
        )
        .create_on(&connection)
        .await
        .unwrap();
    assert_eq!(named.len(), 2);
    for owner in named.iter() {
        assert_eq!(
            connection
                .table("factory_entries")
                .r#where("author_id", "=", owner.get_key().unwrap().clone())
                .order_by("id", "asc")
                .pluck("name")
                .await
                .unwrap(),
            titles.map(Value::from),
        );
    }
    let owner = authors()
        .has_named::<_, HasMany<Author, Entry>>("entries", entries().count(0))
        .create_on(&connection)
        .await
        .unwrap();
    let invalid = entries()
        .for_existing_named::<_, BelongsTo<Entry, Author>>("missing", owner)
        .create_on(&connection)
        .await;
    assert!(invalid.is_err());
    assert_eq!(
        connection
            .table("factory_entries")
            .count("*")
            .await
            .unwrap(),
        entries_before + 6,
    );

    let mut constructed = authors().count(2).create_on(&connection).await.unwrap();
    constructed
        .get_relation_or_load(0, "entries", &relation)
        .await
        .unwrap();
    assert!(!constructed[1].relation_loaded("entries"));

    let mut created_many = authors()
        .create_many_on(&connection, FactoryRecords::Count(2))
        .await
        .unwrap();
    created_many
        .get_relation_or_load(0, "entries", &relation)
        .await
        .unwrap();
    assert!(!created_many[1].relation_loaded("entries"));

    let mut made_many = authors()
        .make_many_on(&connection, FactoryRecords::Count(2))
        .await
        .unwrap();
    made_many
        .get_relation_or_load(0, "entries", &relation)
        .await
        .unwrap();
    assert!(!made_many[1].relation_loaded("entries"));

    let mut queried = Author::query_on(&connection).get().await.unwrap();
    queried
        .get_relation_or_load(0, "entries", &relation)
        .await
        .unwrap();
    assert!(queried[1].relation_loaded("entries"));

    let mut opted_in = constructed.with_relationship_autoloading();
    opted_in
        .get_relation_or_load(1, "all_entries", &relation)
        .await
        .unwrap();
    assert!(opted_in[0].relation_loaded("all_entries"));

    verify_final_state(&connection).await;
    verify_child_relation_defaults(&connection).await;
    verify_configured_callbacks(&connection).await;
    verify_recycled_children(&connection).await;

    for table in ["factory_links", "factory_entries", "factory_authors"] {
        connection
            .statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
}

struct ConfiguredAuthor;

impl FactoryDefinition for ConfiguredAuthor {
    type Model = Author;

    fn definition() -> Attributes {
        Attributes::from([("name".into(), "defined".into())])
    }

    fn configure(factory: Factory<Author>) -> Factory<Author> {
        factory
            .after_making(|author| {
                author.set_attribute("name", "made");
                Ok(())
            })
            .after_creating(|author| {
                assert!(author.state().exists());
                author.set_attribute("name", "created");
                Ok(())
            })
    }
}

async fn verify_configured_callbacks(connection: &Connection) {
    let no_making = ConfiguredAuthor::factory()
        .without_after_making()
        .create_on(connection)
        .await
        .unwrap();
    let no_creating = ConfiguredAuthor::times(2)
        .without_after_creating()
        .create_on(connection)
        .await
        .unwrap();
    assert_eq!(no_making.get_attribute("name"), Some(&"created".into()));
    assert_eq!(no_creating.len(), 2);
    assert!(
        no_creating
            .iter()
            .all(|a| a.get_attribute("name") == Some(&"made".into()))
    );
    let persisted = Author::query_on(connection)
        .find(no_making.get_key().unwrap().clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.get_attribute("name"), Some(&"defined".into()));
    let fresh = ConfiguredAuthor::factory()
        .create_on(connection)
        .await
        .unwrap();
    assert_eq!(fresh.get_attribute("name"), Some(&"created".into()));
    let persisted = Author::query_on(connection)
        .find(fresh.get_key().unwrap().clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(persisted.get_attribute("name"), Some(&"made".into()));
}

async fn verify_recycled_children(connection: &Connection) {
    let owner = authors().create_on(connection).await.unwrap();
    let owner_key = owner.get_key().unwrap().clone();
    let owned = entries()
        .for_existing(BelongsTo::new("author_id", "id"), owner)
        .create_on(connection)
        .await
        .unwrap();
    let owned_key = owned.get_key().unwrap().clone();

    let recycled = authors()
        .recycle_many(std::iter::empty::<Entry>())
        .recycle(owned)
        .recycle_many([authors().create_on(connection).await.unwrap()])
        .has(HasMany::new("id", "author_id"), entries());
    assert_eq!(
        recycled
            .get_random_recycled_model::<Entry>()
            .unwrap()
            .get_key(),
        Some(&owned_key)
    );
    assert!(recycled.get_random_recycled_model::<Author>().is_some());

    let parent = recycled.create_on(connection).await.unwrap();
    let children = HasMany::<Author, Entry>::new("id", "author_id")
        .get_results(&parent)
        .await
        .unwrap();
    assert_eq!(children.len(), 1);
    assert_ne!(children[0].get_key(), Some(&owned_key));
    let prior = Entry::query_on(connection)
        .find(owned_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(prior.get_attribute("author_id"), Some(&owner_key));

    let before = connection
        .table("factory_authors")
        .count("*")
        .await
        .unwrap();
    entries()
        .recycle_many(std::iter::empty::<Author>())
        .r#for(BelongsTo::new("author_id", "id"), authors())
        .create_on(connection)
        .await
        .unwrap();
    assert_eq!(
        connection
            .table("factory_authors")
            .count("*")
            .await
            .unwrap(),
        before + 1
    );
}

async fn verify_child_relation_defaults(connection: &Connection) {
    let named = |name: &str| Attributes::from([("name".into(), name.into())]);
    let one = || {
        HasOne::<Author, Entry>::new("id", "author_id")
            .constrain(move |query| query.with_attributes(named("one default"), false))
    };
    let morph = || {
        MorphMany::<Author, Entry>::new("id", "author_id", "owner_type", "author")
            .constrain(move |query| query.with_attributes(named("morph default"), false))
    };

    for overridden in [false, true] {
        let owner = authors()
            .has(
                one(),
                if overridden {
                    entries().state(move |attributes| {
                        assert_eq!(attributes["name"], "one default".into());
                        named("one override")
                    })
                } else {
                    entries()
                },
            )
            .has(
                morph(),
                if overridden {
                    entries().state(move |attributes| {
                        assert_eq!(attributes["name"], "morph default".into());
                        named("morph override")
                    })
                } else {
                    entries()
                }
                .count(2),
            )
            .create_on(connection)
            .await
            .unwrap();
        let rows = connection
            .table("factory_entries")
            .r#where("author_id", "=", owner.get_key().unwrap().clone())
            .order_by("id", "asc")
            .get()
            .await
            .unwrap();
        let names = if overridden {
            ["one override", "morph override", "morph override"]
        } else {
            ["one default", "morph default", "morph default"]
        };
        assert_eq!(rows.len(), 3);
        for (row, (name, kind)) in
            rows.iter().zip(
                names
                    .into_iter()
                    .zip([Value::Null, "author".into(), "author".into()]),
            )
        {
            assert_eq!(row.get("name"), Some(&name.into()));
            assert_eq!(row.get("owner_type"), Some(&kind));
        }
    }
}

async fn verify_final_state(connection: &Connection) {
    let named = |name: &str| Attributes::from([("name".into(), name.into())]);
    let author_rows = || connection.table("factory_authors");
    let before = author_rows().count("*").await.unwrap();
    let draft = authors()
        .state(move |_| named("configured"))
        .make_with_state_on(connection, |attributes| {
            assert_eq!(attributes["name"], "configured".into());
            named("single override")
        })
        .await
        .unwrap();
    assert_eq!(draft.get_attribute("name"), Some(&"single override".into()));
    assert!(!draft.state().exists());

    let drafts = entries().parent_attribute("author_id", authors()).count(2);
    let made = drafts
        .make_with_on(
            connection,
            Attributes::from([("author_id".into(), 100.into())]),
        )
        .await
        .unwrap();
    assert_eq!(made.len(), 2);
    assert!(made.iter().all(|entry| {
        !entry.state().exists() && entry.get_attribute("author_id") == Some(&100.into())
    }));
    assert_eq!(author_rows().count("*").await.unwrap(), before);

    let phases = Arc::new(Mutex::new(Vec::new()));
    let observe = |phase| {
        let phases = phases.clone();
        move |_: &mut Author| {
            phases.lock().unwrap().push(phase);
            Ok(())
        }
    };
    let batch = authors()
        .state(move |_| named("configured"))
        .after_making(observe("make"))
        .after_creating(observe("create"))
        .count(2);
    let mut names = ["first", "second"].into_iter();
    let models = batch
        .create_with_state_on(connection, |attributes| {
            assert_eq!(attributes["name"], "configured".into());
            named(names.next().unwrap())
        })
        .await
        .unwrap();
    assert_eq!(
        *phases.lock().unwrap(),
        ["make", "make", "create", "create"]
    );
    assert_eq!(models[0].get_attribute("name"), Some(&"first".into()));
    assert_eq!(models[1].get_attribute("name"), Some(&"second".into()));
    let original = batch.make_on(connection).await.unwrap();
    assert_eq!(original.len(), 2);
    assert!(
        original
            .iter()
            .all(|model| model.get_attribute("name") == Some(&"configured".into()))
    );

    let quiet = batch
        .create_quietly_with_on(connection, named("quiet"))
        .await
        .unwrap();
    assert_eq!(quiet.len(), 2);
    assert!(quiet.iter().all(|model| model.state().exists()));
    let quiet_one = authors()
        .create_quietly_with_state_on(connection, |_| named("one quiet"))
        .await
        .unwrap();
    assert_eq!(quiet_one.get_attribute("name"), Some(&"one quiet".into()));
    assert_eq!(author_rows().count("*").await.unwrap(), before + 5);
}

#[tokio::test]
async fn factories_make_and_persist_related_models_on_sqlite() {
    verify(
        json!({"driver":"sqlite","database":":memory:"}),
        "INTEGER PRIMARY KEY",
    )
    .await;
}

#[tokio::test]
async fn factories_make_and_persist_related_models_on_mysql() {
    let url = std::env::var("OPENRUST_TEST_ELOQUENT_MYSQL_URL").ok();
    assert!(url.is_some() || std::env::var_os("OPENRUST_REQUIRE_ELOQUENT_MYSQL").is_none());
    if let Some(url) = url {
        verify(json!({"url":url}), "BIGINT AUTO_INCREMENT PRIMARY KEY").await;
    }
}
