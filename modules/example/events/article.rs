//! 文章变更事件写入模块事件表，和文章操作使用同一事务。

use crate::modules::example::models::article_event;
use openrust::Error;
use openrust::db::sea_orm::{ActiveModelTrait, ActiveValue::Set};
use openrust::events::EventsServiceProvider;

#[derive(Clone, Copy)]
pub(crate) struct ArticleChanged {
    pub(crate) owner_id: i64,
    pub(crate) article_id: i64,
    pub(crate) action: &'static str,
}

pub(crate) fn provider() -> EventsServiceProvider {
    let mut provider = EventsServiceProvider::new();
    provider.listen::<ArticleChanged, _>(|event, context| {
        Box::pin(async move {
            let transaction = context.transaction().ok_or_else(|| {
                Error::custom(
                    "example_article_event_requires_transaction",
                    "文章变更事件须与文章写入共享事务",
                )
            })?;
            article_event::ActiveModel {
                owner_id: Set(event.owner_id),
                article_id: Set(event.article_id),
                action: Set(event.action.to_owned()),
                ..Default::default()
            }
            .insert(transaction)
            .await
            .map_err(Error::internal)?;
            Ok(())
        })
    });
    #[cfg(feature = "queue")]
    provider.listen_queued::<ArticleChanged, crate::modules::example::jobs::article_index::ArticleIndexJob, _>(
        "example",
        |event| {
            Ok(crate::modules::example::jobs::article_index::ArticleIndexJob {
                owner_id: event.owner_id,
                article_id: event.article_id,
                action: event.action.to_owned(),
            })
        },
    );
    provider
}
