use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use openrust::database::eloquent::RelatedCollection;
use openrust::{
    config::Repository,
    database::{
        DatabaseManager, Value,
        eloquent::{
            Attributes, Collection, Model, ModelMetadata, ModelState,
            factories::{Factory, FactoryRecords},
            relations::{BelongsTo, BelongsToMany, HasMany, Pivot, Relation},
        },
    },
};
use serde_json::{Value as Json, json};

struct Author(ModelState);
struct Entry(ModelState);

impl Model for Author {
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
            "CREATE TABLE factory_entries (id {id}, author_id BIGINT NOT NULL, name VARCHAR(80) NOT NULL)"
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

    let entry = entries()
        .count(1)
        .forExisting(BelongsTo::new("author_id", "id"), author)
        .createOneOn(&connection, Attributes::new())
        .await
        .unwrap();
    assert!(entry.getAttribute("author_id").is_some());
    let entry = entries()
        .count(1)
        .r#for(BelongsTo::new("author_id", "id"), authors())
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
            child_factory.count(1),
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
        .sequence(|index| Attributes::from([("name".into(), format!("failure {index}").into())]))
        .afterMaking(|author| {
            if author.getAttribute("name") == Some(&"failure 1".into()) {
                return Err(openrust::database::DatabaseError::InvalidQuery(
                    "rejected draft",
                ));
            }
            Ok(())
        })
        .count(2)
        .createOn(&connection)
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

    for table in ["factory_links", "factory_entries", "factory_authors"] {
        connection
            .statement(&format!("DROP TABLE {table}"), &[])
            .await
            .unwrap();
    }
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
