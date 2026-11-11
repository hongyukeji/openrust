use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        DatabaseManager, Value,
        eloquent::{
            Attributes, Model, ModelMetadata, ModelState,
            factories::Factory,
            relations::{BelongsTo, BelongsToMany, HasMany, Pivot, Relation},
        },
    },
};
use serde_json::{Value as Json, json};

struct Author(ModelState);
struct Entry(ModelState);

impl Model for Author {
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
    let related: Vec<Arc<Entry>> = HasMany::new("id", "author_id")
        .constrain(|query| query.orderBy("id", "asc"))
        .getResults(&author)
        .await
        .unwrap();
    assert_eq!(related.len(), 2);
    assert_eq!(related[1].getAttribute("name"), Some(&"entry 1".into()));

    let entry = entries()
        .forExisting(BelongsTo::new("author_id", "id"), author)
        .createOn(&connection)
        .await
        .unwrap();
    assert!(entry.getAttribute("author_id").is_some());
    let entry = entries()
        .r#for(BelongsTo::new("author_id", "id"), authors())
        .createOn(&connection)
        .await
        .unwrap();
    assert!(entry.getAttribute("author_id").is_some());
    assert_eq!(Author::queryOn(&connection).get().await.unwrap().len(), 2);
    let links = || {
        BelongsToMany::<Author, Entry>::new("factory_links", "author_id", "entry_id", "id", "id")
    };
    let linked = authors()
        .hasAttached(
            links(),
            entries()
                .r#for(BelongsTo::new("author_id", "id"), authors())
                .count(1),
            Attributes::from([("label".into(), "featured".into())]),
        )
        .createOn(&connection)
        .await
        .unwrap();
    let attached = links()
        .withPivot(["label"])
        .getResults(&linked)
        .await
        .unwrap();
    assert_eq!(attached.len(), 1);
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
