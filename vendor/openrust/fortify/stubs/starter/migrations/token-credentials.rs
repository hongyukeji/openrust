use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
#[cfg(feature = "sanctum")]
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    database::DatabaseManager,
    sanctum::{PersonalAccessTokens, Sanctum},
    support::facades::{App, Facade as _},
};

#[cfg(feature = "sanctum")]
fn tokens() -> DatabaseResult<PersonalAccessTokens> {
    let app = App::get_facade_root()
        .map_err(|_| DatabaseError::InvalidQuery("无法解析令牌迁移所属应用"))?;
    let database = app
        .container()
        .make::<DatabaseManager>()
        .map_err(|_| DatabaseError::InvalidQuery("无法解析令牌迁移数据库"))?;
    let sanctum = app
        .container()
        .make::<Sanctum>()
        .map_err(|_| DatabaseError::InvalidQuery("无法解析令牌迁移模型"))?;
    sanctum
        .personal_access_tokens(&database, "")
        .map_err(|_| DatabaseError::InvalidQuery("无法解析令牌迁移连接"))
}
pub struct Definition;
impl Migration for Definition {
    fn get_connection(
        &self,
        _: &dyn openrust::contracts::config::Repository,
    ) -> DatabaseResult<Option<String>> {
        #[cfg(feature = "sanctum")]
        {
            Ok(Some(tokens()?.get_connection_name().into()))
        }
        #[cfg(not(feature = "sanctum"))]
        {
            Ok(None)
        }
    }
    fn should_run<'a>(
        &'a self,
        _: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<bool>> {
        Box::pin(async { Ok(cfg!(feature = "sanctum")) })
    }
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            #[cfg(feature = "sanctum")]
            {
                context
                    .get_schema_builder()
                    .table(&tokens()?.get_table(), |table| {
                        table.string("credential_stamp", 64).nullable();
                    })
                    .await
            }
            #[cfg(not(feature = "sanctum"))]
            {
                let _ = context;
                Err(DatabaseError::InvalidQuery(
                    "启用Sanctum后才能执行令牌凭据迁移",
                ))
            }
        })
    }
    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            #[cfg(feature = "sanctum")]
            {
                context
                    .get_schema_builder()
                    .table(&tokens()?.get_table(), |table| {
                        table.drop_column(&["credential_stamp"]);
                    })
                    .await
            }
            #[cfg(not(feature = "sanctum"))]
            {
                let _ = context;
                Err(DatabaseError::InvalidQuery(
                    "请重新启用Sanctum后回滚令牌凭据迁移，不会删除迁移账本",
                ))
            }
        })
    }
}
