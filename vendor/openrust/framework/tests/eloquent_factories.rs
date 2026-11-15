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
            factories::{Factory, FactoryRecords},
            relations::{BelongsTo, BelongsToMany, HasMany, HasOne, MorphMany, Pivot, Relation},
        },
    },
};
use serde_json::{Value as Json, json};

struct Author(ModelState);
struct Entry(ModelState);

impl Model for Author {
    fn namedRelations() -> NamedRelations<Self> {
        NamedRelations::default()
            .nested("entries", || {
                HasMany::<Author, Entry>::new("id", "author_id")
            })
            .nested("entry", || HasOne::<Author, Entry>::new("id", "author_id"))
    }

    fn automaticallyEagerLoadRelationships() -> bool {
        true
    }

    #[allow(non_snake_case, reason = "Matches Laravel's collection factory hook")]
    fn newCollection(models: Vec<Self>) -> Collection<Self> {
        Collection::fromEntries(
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
    #[allow(non_snake_case, reason = "Matches Laravel's collection factory hook")]
    fn newCollection(mut models: Vec<Self>) -> Collection<Self> {
        for model in &mut models {
            if model.getAttribute("name") == Some(&"explicit child".into()) {
                model.setAttribute("name", "collection hook ran");
            }
        }
        Collection::new(models)
    }

    fn namedRelations() -> NamedRelations<Self> {
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
        .makeOn(&connection)
        .await
        .unwrap();
    assert!(previews.getByKey("author-0").is_some());
    assert!(previews.getByKey("author-1").is_some());
    assert!(previews.iter().all(|author| !author.state().exists()));
    assert_eq!(Author::queryOn(&connection).get().await.unwrap().len(), 0);

    let author = authors()
        .afterMaking(|author| {
            assert!(!author.state().exists());
            author.setAttribute("name", "made");
            Ok(())
        })
        .afterCreating(|author| {
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
        .createOn(&connection)
        .await
        .unwrap();
    assert_eq!(author.getAttribute("name"), Some(&Value::from("made")));
    let related: RelatedCollection<Entry> = HasMany::new("id", "author_id")
        .constrain(|query| query.orderBy("id", "asc"))
        .getResults(&author)
        .await
        .unwrap();
    assert_eq!(related.len(), 2);
    assert_eq!(related[1].getAttribute("name"), Some(&"entry 1".into()));

    let author_id = author.getKey().cloned();
    let entry = entries()
        .count(1)
        .forExistingInferred::<_, BelongsTo<Entry, Author>>(author)
        .createOneOn(&connection, Attributes::new())
        .await
        .unwrap();
    assert_eq!(entry.getAttribute("author_id"), author_id.as_ref());
    let entry = entries()
        .count(1)
        .forInferred::<_, BelongsTo<Entry, Author>>(authors())
        .createOneOn(&connection, Attributes::new())
        .await
        .unwrap();
    assert!(entry.getAttribute("author_id").is_some());
    assert_eq!(Author::queryOn(&connection).get().await.unwrap().len(), 2);
    let links = || {
        BelongsToMany::<Author, Entry>::new("factory_links", "author_id", "entry_id", "id", "id")
    };
    let linked = authors()
        .count(1)
        .hasAttached(
            links().constrain(|query| {
                query.withAttributes(
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
        .createOn(&connection)
        .await
        .unwrap();
    let attached = links()
        .withPivot(["label"])
        .getResults(&linked[0])
        .await
        .unwrap();
    assert_eq!(attached.len(), 1);
    assert_eq!(
        attached[0].getAttribute("name"),
        Some(&"explicit attachment".into())
    );
    assert_eq!(
        attached[0]
            .getRelation::<Pivot>("pivot")
            .unwrap()
            .getAttribute("label"),
        Some(&"featured".into())
    );

    assert!(
        authors()
            .count(0)
            .createOn(&connection)
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
        .afterMaking(|child| {
            assert_ne!(child.getAttribute("author_id"), Some(&999.into()));
            assert!(child.getAttribute("author_id").is_some());
            assert_eq!(child.getAttribute("name"), Some(&"explicit child".into()));
            Ok(())
        })
        .afterCreating(move |child| {
            assert!(child.getAttribute("author_id").is_some());
            child_stages.lock().unwrap().push("child");
            Ok(())
        });

    let batch = authors()
        .count(5)
        .afterMaking(observe("make"))
        .afterCreating(observe("created"))
        .has(
            HasMany::new("id", "author_id").constrain(|query| {
                query.withAttributes(
                    Attributes::from([("name".into(), "relation default".into())]),
                    false,
                )
            }),
            child_factory,
        )
        .sequence(|index| Attributes::from([("name".into(), format!("batch {index}").into())]))
        .count(2)
        .createOn(&connection)
        .await
        .unwrap();
    assert_eq!(
        *stages.lock().unwrap(),
        ["make", "make", "child", "child", "created", "created"]
    );
    let before = Author::queryOn(&connection).get().await.unwrap().len();
    let failed = authors()
        .afterMaking(|author| {
            if author.getAttribute("name") == Some(&"failure 1".into()) {
                return Err(openrust::database::DatabaseError::InvalidQuery(
                    "rejected draft",
                ));
            }
            Ok(())
        })
        .count(2)
        .createWithStateOn(&connection, {
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
        Author::queryOn(&connection).get().await.unwrap().len(),
        before
    );
    for (index, model) in batch.iter().enumerate() {
        assert!(model.state().exists());
        assert_eq!(
            model.getAttribute("name"),
            Some(&format!("batch {index}").into())
        );
        assert_eq!(
            batch.getByKey(format!("author-{index}")).unwrap().getKey(),
            model.getKey()
        );
    }

    let relation = HasMany::<Author, Entry>::new("id", "author_id");
    let sequence = authors()
        .count(7)
        .forEachSequence(vec![
            Attributes::from([("name".into(), "one".into())]),
            Attributes::from([("name".into(), "two".into())]),
        ])
        .makeOn(&connection)
        .await
        .unwrap();
    assert_eq!(sequence.len(), 2);
    assert_eq!(sequence[0].getAttribute("name"), Some(&"one".into()));
    assert_eq!(sequence[1].getAttribute("name"), Some(&"two".into()));
    let crossed = authors()
        .count(3)
        .crossJoinSequence(vec![
            vec![Attributes::new(), Attributes::new()],
            vec![Attributes::from([("name".into(), "crossed".into())])],
        ])
        .unwrap()
        .attribute("derived_name", |attributes| {
            assert_eq!(attributes.get("name"), Some(&"crossed".into()));
            Ok("derived".into())
        })
        .makeOn(&connection)
        .await
        .unwrap();
    assert_eq!(crossed.len(), 3);
    assert!(
        crossed
            .iter()
            .all(|author| author.getAttribute("derived_name") == Some(&"derived".into()))
    );

    let entries_before = connection
        .table("factory_entries")
        .count("*")
        .await
        .unwrap();
    let named = authors()
        .count(2)
        .hasInferred::<_, HasMany<Author, Entry>>(entries())
        .createOn(&connection)
        .await
        .unwrap();
    assert_eq!(named.len(), 2);
    for owner in named.iter() {
        assert_eq!(
            connection
                .table("factory_entries")
                .r#where("author_id", "=", owner.getKey().unwrap().clone())
                .count("*")
                .await
                .unwrap(),
            1,
        );
    }
    let owner = authors()
        .hasNamed::<_, HasMany<Author, Entry>>("entries", entries().count(0))
        .createOn(&connection)
        .await
        .unwrap();
    let invalid = entries()
        .forExistingNamed::<_, BelongsTo<Entry, Author>>("missing", owner)
        .createOn(&connection)
        .await;
    assert!(invalid.is_err());
    assert_eq!(
        connection
            .table("factory_entries")
            .count("*")
            .await
            .unwrap(),
        entries_before + 2,
    );

    let mut constructed = authors().count(2).createOn(&connection).await.unwrap();
    constructed
        .getRelationOrLoad(0, "entries", &relation)
        .await
        .unwrap();
    assert!(!constructed[1].relationLoaded("entries"));

    let mut created_many = authors()
        .createManyOn(&connection, FactoryRecords::Count(2))
        .await
        .unwrap();
    created_many
        .getRelationOrLoad(0, "entries", &relation)
        .await
        .unwrap();
    assert!(!created_many[1].relationLoaded("entries"));

    let mut made_many = authors()
        .makeManyOn(&connection, FactoryRecords::Count(2))
        .await
        .unwrap();
    made_many
        .getRelationOrLoad(0, "entries", &relation)
        .await
        .unwrap();
    assert!(!made_many[1].relationLoaded("entries"));

    let mut queried = Author::queryOn(&connection).get().await.unwrap();
    queried
        .getRelationOrLoad(0, "entries", &relation)
        .await
        .unwrap();
    assert!(queried[1].relationLoaded("entries"));

    let mut opted_in = constructed.withRelationshipAutoloading();
    opted_in
        .getRelationOrLoad(1, "all_entries", &relation)
        .await
        .unwrap();
    assert!(opted_in[0].relationLoaded("all_entries"));

    verify_final_state(&connection).await;
    verify_child_relation_defaults(&connection).await;

    for table in ["factory_links", "factory_entries", "factory_authors"] {
        connection
            .statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
}

async fn verify_child_relation_defaults(connection: &Connection) {
    let named = |name: &str| Attributes::from([("name".into(), name.into())]);
    let one = || {
        HasOne::<Author, Entry>::new("id", "author_id")
            .constrain(move |query| query.withAttributes(named("one default"), false))
    };
    let morph = || {
        MorphMany::<Author, Entry>::new("id", "author_id", "owner_type", "author")
            .constrain(move |query| query.withAttributes(named("morph default"), false))
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
            .createOn(connection)
            .await
            .unwrap();
        let rows = connection
            .table("factory_entries")
            .r#where("author_id", "=", owner.getKey().unwrap().clone())
            .orderBy("id", "asc")
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
        .makeWithStateOn(connection, |attributes| {
            assert_eq!(attributes["name"], "configured".into());
            named("single override")
        })
        .await
        .unwrap();
    assert_eq!(draft.getAttribute("name"), Some(&"single override".into()));
    assert!(!draft.state().exists());

    let drafts = entries().parentAttribute("author_id", authors()).count(2);
    let made = drafts
        .makeWithOn(
            connection,
            Attributes::from([("author_id".into(), 100.into())]),
        )
        .await
        .unwrap();
    assert_eq!(made.len(), 2);
    assert!(made.iter().all(|entry| {
        !entry.state().exists() && entry.getAttribute("author_id") == Some(&100.into())
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
        .afterMaking(observe("make"))
        .afterCreating(observe("create"))
        .count(2);
    let mut names = ["first", "second"].into_iter();
    let models = batch
        .createWithStateOn(connection, |attributes| {
            assert_eq!(attributes["name"], "configured".into());
            named(names.next().unwrap())
        })
        .await
        .unwrap();
    assert_eq!(
        *phases.lock().unwrap(),
        ["make", "make", "create", "create"]
    );
    assert_eq!(models[0].getAttribute("name"), Some(&"first".into()));
    assert_eq!(models[1].getAttribute("name"), Some(&"second".into()));
    let original = batch.makeOn(connection).await.unwrap();
    assert_eq!(original.len(), 2);
    assert!(
        original
            .iter()
            .all(|model| model.getAttribute("name") == Some(&"configured".into()))
    );

    let quiet = batch
        .createQuietlyWithOn(connection, named("quiet"))
        .await
        .unwrap();
    assert_eq!(quiet.len(), 2);
    assert!(quiet.iter().all(|model| model.state().exists()));
    let quiet_one = authors()
        .createQuietlyWithStateOn(connection, |_| named("one quiet"))
        .await
        .unwrap();
    assert_eq!(quiet_one.getAttribute("name"), Some(&"one quiet".into()));
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
