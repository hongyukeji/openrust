use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    config::Repository,
    database::{
        Connection, DatabaseError, DatabaseManager, DatabaseResult, Row, Value,
        eloquent::{Builder, Model, ModelMetadata, ModelState, Scope},
    },
};
use serde_json::json;

fn connection(driver: &str, url: Option<&str>) -> Connection {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default": "test", "connections": {"test": {"driver": driver, "url": url}}}),
    )]));
    DatabaseManager::new(Arc::new(settings), ".")
        .connection(None)
        .unwrap()
}

struct Document(ModelState);

impl Model for Document {
    fn metadata() -> &'static ModelMetadata {
        static METADATA: ModelMetadata = ModelMetadata::new("documents").timestamps(None, None);
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

    fn global_scopes() -> &'static [(&'static str, &'static dyn Scope<Self>)] {
        &[("visible", &VisibleDocuments)]
    }
}

struct VisibleDocuments;

impl Scope<Document> for VisibleDocuments {
    fn apply(&self, builder: &mut Builder<'_, Document>) -> DatabaseResult<()> {
        builder.r#where("id", "<", 3);
        Ok(())
    }
}

#[tokio::test]
async fn vector_queries_compile_native_expressions_and_keep_binding_order() {
    for (driver, expression) in [
        (
            "pgsql",
            "(\"d\".\"embedding\" <=> cast(cast(? as text) as vector))",
        ),
        (
            "mariadb",
            "vec_distance_cosine(`d`.`embedding`, vec_fromtext(?))",
        ),
    ] {
        let connection = connection(driver, None);
        let mut query = connection
            .table("documents as d")
            .order_by_vector_distance("d.embedding", &[0.0, 1.0])
            .unwrap()
            .r#where("d.id", ">", 4)
            .or_where_vector_distance_less_than("d.embedding", &[1.0, 0.0], 0.5)
            .unwrap()
            .select_vector_distance("d.embedding", &[0.5, 0.5], None)
            .unwrap();
        let sql = query.to_sql().unwrap();
        assert!(sql.contains(&format!("select {expression} as")));
        assert!(sql.contains("embedding_distance"));
        assert!(sql.contains(&format!("or {expression} <= ?")));
        assert!(sql.ends_with(&format!("order by {expression} asc")));
        assert_eq!(
            query.get_bindings().unwrap(),
            [
                "[0.5,0.5]".into(),
                4.into(),
                "[1.0,0.0]".into(),
                Value::Real(0.5),
                "[0.0,1.0]".into(),
            ]
        );
        let mut other = connection.table("documents").select(["embedding"]);
        let mut union = connection
            .table("documents")
            .select(["embedding"])
            .r#where("id", ">", 3)
            .union_all(&mut other)
            .unwrap()
            .order_by_vector_distance("embedding", &[1.0, 0.0])
            .unwrap();
        assert!(union.to_sql().unwrap().contains("union all"));
        assert_eq!(
            union.get_bindings().unwrap(),
            [3.into(), "[1.0,0.0]".into()]
        );
    }
}

#[tokio::test]
async fn vector_queries_reject_unsupported_drivers_and_invalid_inputs() {
    for driver in ["mysql", "sqlite", "sqlsrv"] {
        assert!(matches!(
            connection(driver, None)
                .table("documents")
                .order_by_vector_distance("embedding", &[1.0, 0.0]),
            Err(DatabaseError::UnsupportedOperation(
                "vector distance queries"
            ))
        ));
    }
    let db = connection("pgsql", None);
    for invalid in [&[][..], &[f64::NAN], &[f64::INFINITY]] {
        assert!(matches!(
            db.table("documents")
                .select_vector_distance("embedding", invalid, None),
            Err(DatabaseError::InvalidQuery(_))
        ));
    }
    assert!(matches!(
        db.table("documents")
            .where_vector_similar_to("embedding", &[1.0], 1.5, true),
        Err(DatabaseError::InvalidQuery(_))
    ));
    assert!(matches!(
        db.table("documents")
            .where_vector_distance_less_than("embedding", &[1.0], f64::NAN),
        Err(DatabaseError::InvalidQuery(_))
    ));
}

#[tokio::test]
async fn nested_vector_predicates_compile_with_the_outer_query_grammar() {
    for driver in ["pgsql", "mariadb"] {
        let db = connection(driver, None);
        let mut query = db
            .table("documents")
            .r#where("id", "<", 3)
            .where_nested(|group| {
                group
                    .where_vector_distance_less_than("embedding", &[1.0, 0.0], 0.3)
                    .or_where_vector_distance_less_than("embedding", &[0.0, 1.0], 0.0)
            });
        let sql = query.to_sql().unwrap();
        assert!(sql.contains("< ? and ("));
        assert!(sql.contains(" <= ? or "));
        assert!(sql.ends_with(" <= ?)"));
        assert_eq!(
            query.get_bindings().unwrap(),
            [
                3.into(),
                "[1.0,0.0]".into(),
                Value::Real(0.3),
                "[0.0,1.0]".into(),
                Value::Real(0.0),
            ],
        );
    }
    let sqlite = connection("sqlite", None);
    assert!(matches!(
        sqlite
            .table("documents")
            .where_nested(|group| group.where_vector_distance_less_than("embedding", &[1.0], 0.3))
            .to_sql(),
        Err(DatabaseError::UnsupportedOperation(
            "vector distance queries"
        ))
    ));
}

fn fixture(driver: &str, setting: &str) -> Option<Connection> {
    match std::env::var(setting) {
        Ok(url) => Some(connection(driver, Some(&url))),
        Err(error) if std::env::var_os("OPENRUST_REQUIRE_VECTOR_TEST").is_some() => {
            panic!("{setting}: {error}")
        }
        Err(_) => None,
    }
}

fn ids(rows: &[Row]) -> Vec<i64> {
    rows.iter()
        .map(|row| match row.get("id").unwrap() {
            Value::Integer(id) => *id,
            value => panic!("unexpected ID: {value:?}"),
        })
        .collect()
}

async fn verify_distances(db: &Connection) {
    let vector = [1.0, 0.0];
    let rows = db
        .table("documents")
        .select(["id"])
        .select_vector_distance("embedding", &vector, Some("distance"))
        .unwrap()
        .where_vector_distance_less_than("embedding", &vector, 0.3)
        .unwrap()
        .order_by_vector_distance("embedding", &vector)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows), [2, 1]);
    let distances: Vec<_> = rows
        .iter()
        .map(|row| match row.get("distance").unwrap() {
            Value::Real(value) => *value,
            value => panic!("unexpected distance: {value:?}"),
        })
        .collect();
    assert!(distances[0].abs() < 1e-6);
    assert!((distances[1] - 0.2).abs() < 1e-6);
    let matches = db
        .table("documents")
        .select(["id"])
        .where_vector_similar_to("embedding", &vector, 0.7, false)
        .unwrap()
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&matches), [1, 2]);
    let exact = db
        .table("documents")
        .select(["id"])
        .where_vector_similar_to("embedding", &vector, 1.0, true)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&exact), [2]);
    let either = db
        .table("documents")
        .select(["id"])
        .r#where("id", "=", 3)
        .or_where_vector_distance_less_than("embedding", &vector, 0.0)
        .unwrap()
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&either), [2, 3]);
}

async fn verify_model_queries(db: &Connection) {
    let vector = [1.0, 0.0];
    let orthogonal = [0.0, 1.0];
    let rows = db
        .table("documents")
        .select(["id"])
        .r#where("id", "<", 3)
        .where_nested(|group| {
            group
                .where_vector_distance_less_than("embedding", &vector, 0.3)
                .or_where_vector_distance_less_than("embedding", &orthogonal, 0.0)
        })
        .order_by("id", "asc")
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows), [1, 2]);

    let mut query = Document::query_on(db)
        .select(["id"])
        .select_vector_distance("embedding", &vector, Some("distance"))
        .unwrap()
        .where_vector_similar_to("embedding", &vector, 0.7, true)
        .unwrap();
    // The orthogonal document satisfies the OR arm but is outside the global scope.
    query.or_where_vector_distance_less_than("embedding", &orthogonal, 0.0);
    let rows = query.get().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get_attribute("id"), Some(&2.into()));
    assert_eq!(rows[1].get_attribute("id"), Some(&1.into()));
    assert!(
        matches!(rows[1].get_attribute("distance"), Some(Value::Real(distance)) if (*distance - 0.2).abs() < 1e-6)
    );

    let mut query = Document::query_on(db).select(["id"]).order_by("id", "asc");
    query
        .where_vector_distance_less_than("embedding", &vector, 0.3)
        .where_nested(|group| group.or_where_vector_distance_less_than("embedding", &vector, 0.3));
    let rows = query.get().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get_attribute("id"), Some(&1.into()));
}

#[tokio::test]
async fn vector_queries_run_on_pgvector() {
    let Some(db) = fixture("pgsql", "OPENRUST_TEST_PGVECTOR_URL") else {
        return;
    };
    verify_distances(&db).await;
    verify_model_queries(&db).await;
    db.transaction(
        |transaction| {
            Box::pin(async move {
                let mut schema = transaction.get_schema_builder();
                let error = schema
                    .table("documents", |table| {
                        table.integer("online_probe");
                        table
                            .vector_index("embedding", Some("online_probe_idx"))
                            .online();
                    })
                    .await
                    .unwrap_err();
                assert!(matches!(
                    error,
                    DatabaseError::UnsupportedSchema(
                        "PostgreSQL online indexes require autocommit"
                    )
                ));
                assert!(!schema.has_column("documents", "online_probe").await?);
                Ok(())
            })
        },
        1,
    )
    .await
    .unwrap();

    let mut session = db.get_schema_builder().session().await.unwrap();
    let mut schema = session.get_schema_builder();
    schema
        .table("documents", |table| {
            table
                .vector_index("embedding", Some("session_online_probe"))
                .online();
        })
        .await
        .unwrap();
    assert!(
        schema
            .has_index("documents", "session_online_probe")
            .await
            .unwrap()
    );
    schema
        .table("documents", |table| {
            table.drop_vector_index("session_online_probe")
        })
        .await
        .unwrap();
    session.close().await.unwrap();
    let rows = db
        .table("documents")
        .where_vector_similar_to("embedding", &[1.0, 0.0], 0.7, true)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows), [2, 1]);
    let Some(Value::Text(text)) = rows[0].get("embedding") else {
        panic!("PostgreSQL vector column did not decode as text");
    };
    assert_eq!(serde_json::from_str::<Vec<f32>>(text).unwrap(), [1.0, 0.0]);
}

#[tokio::test]
async fn vector_queries_run_on_mariadb() {
    let Some(db) = fixture("mariadb", "OPENRUST_TEST_MARIADB_VECTOR_URL") else {
        return;
    };
    verify_distances(&db).await;
    verify_model_queries(&db).await;
    let rows = db
        .table("documents")
        .where_vector_similar_to("embedding", &[1.0, 0.0], 0.7, true)
        .unwrap()
        .get()
        .await
        .unwrap();
    assert_eq!(ids(&rows), [2, 1]);
    assert!(
        matches!(rows[0].get("embedding"), Some(Value::Bytes(bytes)) if
            bytes == &[0, 0, 128, 63, 0, 0, 0, 0]
        )
    );
}
