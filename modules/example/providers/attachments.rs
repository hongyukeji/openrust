//! 文章附件业务服务只复用当前容器的数据库和存储绑定。

use std::sync::Arc;

use crate::modules::example::services::attachments::ArticleAttachments;
use openrust::console::Command;
use openrust::filesystem::FilesystemManager;
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::support::ServiceProvider;
use openrust::{Db, Result};

pub(crate) struct ArticleAttachmentsProvider;

impl ServiceProvider for ArticleAttachmentsProvider {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.singleton::<ArticleAttachments, _>(|resolver| {
                Box::pin(async move {
                    Ok(Arc::new(ArticleAttachments::new(
                        resolver.resolve::<Db>().await?,
                        resolver.resolve::<FilesystemManager>().await?,
                        resolver.resources()?,
                    )))
                })
            })
            .await?;
            Ok(())
        })
    }

    fn boot<'a>(
        &'a self,
        app: &'a mut BootContext<'_>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.command(ReconcileAttachmentCleanup);
            Ok(())
        })
    }
}

struct ReconcileAttachmentCleanup;

impl Command for ReconcileAttachmentCleanup {
    fn name(&self) -> &'static str {
        "example attachments reconcile"
    }

    fn about(&self) -> &'static str {
        "Retry committed example article attachment cleanup intents"
    }

    #[allow(clippy::print_stdout)] // 显式维护命令向调用者报告已清理意图数量。
    fn run<'a>(
        &'a self,
        app: &'a openrust::foundation::Application,
        _args: Vec<String>,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let attachments = app.state().resolve::<ArticleAttachments>().await?;
            let completed = attachments.reconcile_pending().await?;
            println!("reconciled {completed} example attachment cleanup intents");
            Ok(())
        })
    }
}
