//! 验证生成应用使用公开查询构造器对 SQLite 联表与分页目标执行写入。

use openrust::{
    Error, Result,
    database::{
        Db, JoinKind, SortDirection,
        sea_orm::ConnectionTrait,
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    testing::TestDatabase,
};

struct EmptyMigrator;

impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

#[tokio::test]
#[ignore = "SQLite 实库留待发布前当前版与最低版本矩阵"]
async fn joined_and_limited_writes_select_only_the_intended_rows() -> Result<()> {
    let database = TestDatabase::sqlite_file::<EmptyMigrator>().await?;
    let connection = database.connection();
    let outcome = tokio::spawn(async move {
        let db = Db::default();
        db.initialize(connection?)?;
        let connection = db.primary_connection()?;
        for sql in [
            "CREATE TABLE tasks (id INTEGER PRIMARY KEY, completed INTEGER NOT NULL)",
            "CREATE TABLE permitted_tasks (task_id INTEGER NOT NULL)",
            "INSERT INTO tasks VALUES (1, 0), (2, 0), (3, 0)",
            "INSERT INTO permitted_tasks VALUES (2), (3)",
        ] {
            connection
                .execute_unprepared(sql)
                .await
                .map_err(Error::internal)?;
        }

        let tasks = db.table("tasks")?;
        assert_eq!(
            tasks
                .clone()
                .join("permitted_tasks", "id", "task_id", JoinKind::Inner)?
                .order_by("id", SortDirection::Desc)?
                .limit(1)
                .update(vec![("completed", 1_i64.into())])
                .await?,
            1
        );
        assert_eq!(
            tasks
                .clone()
                .where_eq("id", 3_i64)?
                .where_eq("completed", 1_i64)?
                .count()
                .await?,
            1
        );
        assert_eq!(
            tasks.clone().where_eq("completed", 1_i64)?.count().await?,
            1
        );
        assert_eq!(tasks.clone().limit(0).delete().await?, 0);
        assert_eq!(
            tasks
                .clone()
                .join("permitted_tasks", "id", "task_id", JoinKind::Inner)?
                .where_eq("id", 2_i64)?
                .delete()
                .await?,
            1
        );
        assert_eq!(tasks.clone().where_eq("id", 1_i64)?.count().await?, 1);
        assert_eq!(tasks.where_eq("id", 2_i64)?.count().await?, 0);
        Ok::<(), Error>(())
    })
    .await
    .map_err(Error::internal)
    .and_then(std::convert::identity);
    let cleanup = database.close().await;
    match (outcome, cleanup) {
        (Ok(()), cleanup) => cleanup,
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup_error)) => {
            eprintln!("SQLite 写目标测试清理失败：{}", cleanup_error.code());
            Err(error)
        }
    }
}
