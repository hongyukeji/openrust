//! 验证受保护查询在真实数据库上的 AND 组合与关联隔离。
#![cfg(all(feature = "auth", any(feature = "db-postgres", feature = "db-mysql")))]

use std::error::Error as StdError;

use openrust::{
    AppBuilder,
    auth::{
        AuthCredential, AuthLayer, AuthMode, AuthPlugin, AuthPrincipal, Authenticator, CurrentUser,
        QueryRule, QueryScope,
    },
    config::FrameworkConfig,
    core::{BoxFuture, Result},
    database::sea_orm_migration::{MigrationTrait, MigratorTrait},
    db::{
        BatchDirection, BatchOptions, BatchReader, Db,
        sea_orm::{self, ActiveValue, Condition, ConnectionTrait, Statement},
    },
    http::{
        AppBuilderHttpExt, HttpPlugin, Inject, Json, Routes,
        axum::{
            body::{Body, to_bytes},
            http::{Request, StatusCode},
        },
        build_router,
    },
    lifecycle::finalize,
};
use sea_orm::entity::prelude::*;
use testing::TestDatabase;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn StdError>>;

mod resource {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "query_resources")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub owner_id: i64,
        pub title: String,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(has_many = "super::comment::Entity")]
        Comments,
    }
    impl Related<super::comment::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Comments.def()
        }
    }
    impl ActiveModelBehavior for ActiveModel {}
}

mod comment {
    use sea_orm::entity::prelude::*;
    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "query_comments")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub resource_id: i64,
        pub owner_id: i64,
        pub body: String,
    }
    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::resource::Entity",
            from = "Column::ResourceId",
            to = "super::resource::Column::Id",
            on_update = "Cascade",
            on_delete = "Cascade"
        )]
        Resource,
    }
    impl Related<super::resource::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Resource.def()
        }
    }
    impl ActiveModelBehavior for ActiveModel {}
}

mod composite {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "batch_composite")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub group_id: i32,
        #[sea_orm(primary_key, auto_increment = false)]
        pub position: i32,
        pub label: String,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}
}

struct EmptyMigrator;
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

struct TestAuthenticator;
impl Authenticator for TestAuthenticator {
    fn authenticate<'a>(
        &'a self,
        credential: AuthCredential<'a>,
    ) -> BoxFuture<'a, Result<AuthPrincipal>> {
        Box::pin(async move {
            match credential.value() {
                Some("owner-one") => Ok(AuthPrincipal::new(1, Some(vec!["records:read".into()]))),
                Some("owner-two") => Ok(AuthPrincipal::new(2, Some(vec!["records:read".into()]))),
                _ => Err(openrust::Error::Unauthorized),
            }
        })
    }
}

async fn read_authorized_batches(
    user: &CurrentUser,
    db: &Db,
) -> Result<(Vec<i64>, bool, usize, usize)> {
    let resources = QueryRule::<resource::Entity>::new(|user| {
        Ok(QueryScope::new(resource::Column::OwnerId.eq(user.id())))
    });
    let cancellation = openrust::core::Cancellation::new();
    let cancel_after_first = cancellation.clone();
    let mut ids = Vec::new();
    let report = resources
        .select(user, Condition::all())?
        .into_batches()
        .run(
            db,
            BatchOptions::new(1)?,
            BatchDirection::Ascending,
            &cancellation,
            |rows| {
                ids.extend(rows.into_iter().map(|row| row.id));
                cancel_after_first.cancel();
                async { Ok(()) }
            },
        )
        .await?;
    let cursor = report
        .cursor()
        .ok_or_else(|| openrust::Error::custom("batch_cursor_missing", "分批读取未返回游标"))?;
    let resumed = resources
        .select(user, Condition::all())?
        .into_batches()
        .after(cursor)
        .run(
            db,
            BatchOptions::new(1)?,
            BatchDirection::Ascending,
            &openrust::core::Cancellation::new(),
            |rows| {
                ids.extend(rows.into_iter().map(|row| row.id));
                async { Ok(()) }
            },
        )
        .await?;
    Ok((
        ids,
        report.is_cancelled(),
        report.batches() + resumed.batches(),
        resumed.rows(),
    ))
}

async fn verify_composite_batches(db: &Db, backend: sea_orm::DbBackend) -> TestResult {
    let connection = db.primary_connection()?;
    for sql in [
        "CREATE TABLE batch_composite (group_id INT NOT NULL, position INT NOT NULL, label VARCHAR(64) NOT NULL, PRIMARY KEY (group_id, position))",
        "INSERT INTO batch_composite (group_id, position, label) VALUES (1, 1, 'a'), (1, 3, 'c'), (2, 1, 'd'), (2, 2, 'e')",
    ] {
        connection
            .execute_raw(Statement::from_string(backend, sql.to_owned()))
            .await?;
    }
    let mut ascending = Vec::new();
    BatchReader::new(composite::Entity::find())
        .run(
            db,
            BatchOptions::new(1)?,
            BatchDirection::Ascending,
            &openrust::core::Cancellation::new(),
            |rows| {
                ascending.extend(rows.into_iter().map(|row| (row.group_id, row.position)));
                async { Ok(()) }
            },
        )
        .await?;
    assert_eq!(ascending, [(1, 1), (1, 3), (2, 1), (2, 2)]);
    let mut descending = Vec::new();
    BatchReader::new(composite::Entity::find())
        .run(
            db,
            BatchOptions::new(2)?,
            BatchDirection::Descending,
            &openrust::core::Cancellation::new(),
            |rows| {
                descending.extend(rows.into_iter().map(|row| (row.group_id, row.position)));
                async { Ok(()) }
            },
        )
        .await?;
    assert_eq!(descending, [(2, 2), (2, 1), (1, 3), (1, 1)]);
    let mut failed_calls = 0;
    let failed = BatchReader::new(composite::Entity::find())
        .run(
            db,
            BatchOptions::new(1)?,
            BatchDirection::Ascending,
            &openrust::core::Cancellation::new(),
            |_| {
                failed_calls += 1;
                async {
                    Err(openrust::Error::custom(
                        "batch_callback_failed",
                        "批次回调失败",
                    ))
                }
            },
        )
        .await;
    assert!(failed.is_err());
    assert_eq!(failed_calls, 1);
    Ok(())
}

async fn exercise(user: CurrentUser, Inject(db): Inject<Db>) -> Result<Json<String>> {
    let connection = db.connection()?;
    let resources = QueryRule::<resource::Entity>::new(|user| {
        Ok(QueryScope::new(resource::Column::OwnerId.eq(user.id())))
    });
    let comments = QueryRule::<comment::Entity>::new(|user| {
        Ok(QueryScope::new(comment::Column::OwnerId.eq(user.id())))
    });
    let requested = Condition::any()
        .add(resource::Column::Id.eq(1))
        .add(resource::Column::Id.eq(2));
    let listed = resources
        .select(&user, requested.clone())?
        .order_by_asc(resource::Column::Id)
        .all(&connection)
        .await?;
    let ids = listed
        .iter()
        .map(|row| row.id.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let (batch_ids, cancelled, batches, resumed) = read_authorized_batches(&user, &db).await?;
    let foreign = resources
        .select(&user, Condition::all().add(resource::Column::Id.eq(2)))?
        .one(&connection)
        .await?;

    let (page, total) = resources
        .select(&user, Condition::all())?
        .page(&connection, 0, 1)
        .await?;
    let update = resources
        .update_many(
            &user,
            resource::ActiveModel {
                title: ActiveValue::Set("changed".into()),
                ..Default::default()
            },
            Condition::any()
                .add(resource::Column::Id.eq(2))
                .add(resource::Column::Id.eq(3)),
            &connection,
        )
        .await?;

    let parent = resources
        .select(&user, Condition::all().add(resource::Column::Id.eq(1)))?
        .one(&connection)
        .await?
        .ok_or_else(|| openrust::Error::NotFound("resource".into()))?;
    let related = resources
        .related(&user, &parent, &comments, Condition::all(), &connection)
        .await?
        .all(&connection)
        .await?;
    let delete = resources
        .delete_many(
            &user,
            Condition::any()
                .add(resource::Column::Id.eq(2))
                .add(resource::Column::Id.eq(3)),
            &connection,
        )
        .await?;
    Ok(Json(format!(
        "ids={ids};batch={batch_ids:?};cancelled={cancelled};batches={batches};resumed={resumed};foreign={};page={};total={total};updated={};related={};deleted={}",
        foreign.is_some(),
        page.len(),
        update.rows_affected,
        related.len(),
        delete.rows_affected
    )))
}

#[tokio::test]
async fn query_scope_filters_mutations_and_relations_on_real_database() -> TestResult {
    let Ok(admin_url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping query authorization integration: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let isolated = TestDatabase::create::<EmptyMigrator>(&admin_url).await?;
    let connection = sea_orm::Database::connect(isolated.url()).await?;
    let backend = connection.get_database_backend();
    for sql in [
        "CREATE TABLE query_resources (id BIGINT PRIMARY KEY, owner_id BIGINT NOT NULL, title VARCHAR(255) NOT NULL)",
        "CREATE TABLE query_comments (id BIGINT PRIMARY KEY, resource_id BIGINT NOT NULL, owner_id BIGINT NOT NULL, body VARCHAR(255) NOT NULL)",
        "INSERT INTO query_resources (id, owner_id, title) VALUES (1, 1, 'one'), (2, 2, 'two'), (3, 1, 'three')",
        "INSERT INTO query_comments (id, resource_id, owner_id, body) VALUES (1, 1, 1, 'visible'), (2, 1, 2, 'private'), (3, 2, 1, 'other parent')",
    ] {
        connection
            .execute_raw(Statement::from_string(backend, sql.to_owned()))
            .await?;
    }
    let db = Db::default();
    db.initialize(connection.clone())?;
    verify_composite_batches(&db, backend).await?;
    let mut builder = AppBuilder::new(FrameworkConfig::default(), figment::Figment::new());
    builder
        .plugin(HttpPlugin)
        .plugin(AuthPlugin::with_authenticator(TestAuthenticator))
        .insert(db)
        .routes(|routes: Routes| {
            routes
                .post("/exercise", exercise)
                .layer(AuthLayer::required(AuthMode::PersonalAccessToken))
        });
    let app = finalize(builder).await?;
    let router = build_router(&app)?;
    for (token, expected) in [(
        "owner-one",
        "ids=1;batch=[1, 3];cancelled=true;batches=2;resumed=1;foreign=false;page=1;total=2;updated=1;related=1;deleted=1",
    )] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/exercise")
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())?,
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let actual: serde_json::Value = serde_json::from_slice(&body)?;
        assert_eq!(actual["success"], true);
        assert_eq!(actual["data"], expected);
    }
    drop(connection);
    drop(isolated);
    Ok(())
}
