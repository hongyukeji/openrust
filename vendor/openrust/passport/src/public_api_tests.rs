//! 从扩展公开导出经过真实 Provider 注册路径构造仓储；不创建数据库连接。

use crate::{ClientFactory, ClientLookup, ClientRepository, register_client_repository_scope};
use openrust::Result;
use openrust::database::DatabaseServiceProvider;
use openrust::database::Databases;
use openrust::foundation::ProviderBuilder;
use openrust::foundation::RegisterContext;
use openrust::support::{BoxFuture, ServiceProvider};
use serde_json::json;
use std::{sync::Arc, time::Duration};

struct ReadRegistration;
impl ServiceProvider for ReadRegistration {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(register_client_repository_scope(app))
    }
}

#[tokio::test]
async fn public_constructor_and_provider_scope_registration_use_the_same_container() -> Result<()> {
    let mut providers = ProviderBuilder::new();
    providers
        .provider(DatabaseServiceProvider)
        .provider(ReadRegistration);
    let runtime = providers.start(figment::Figment::new()).await?;
    let outcome: Result<()> = async {
        let databases = runtime.container().resolve::<Databases>().await?;
        let repository =
            ClientRepository::new(&databases, None, Arc::new(ClientFactory::default()))?;
        let scope = runtime.container().scope()?;
        assert!(
            repository
                .find(
                    &crate::ClientLookupContext::from(&scope),
                    &json!("malformed")
                )
                .await?
                .is_none()
        );
        let valid = json!("019a4d4b-f21a-7191-bce9-fdc817a165aa");
        // 没有 DB URL；错误须来自真实冷 Db，不能是遗漏私有缓存绑定。
        let result = repository
            .find(&crate::ClientLookupContext::from(&scope), &valid)
            .await;
        assert!(
            matches!(result, Err(error) if error.code() == "internal_error"
            && error.status() == openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        );
        scope.close();
        assert!(
            matches!(repository.find(&crate::ClientLookupContext::from(&scope), &valid).await,
            Err(error) if error.code() == "scope_closed")
        );
        Ok(())
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}
