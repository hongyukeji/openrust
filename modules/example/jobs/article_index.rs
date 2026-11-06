//! 文章变更后异步维护按所有者分隔的派生缓存；重复投递可安全重做。

use std::time::Duration;

use crate::modules::example::models::article;
use openrust::cache::Cache;
use openrust::core::Result;
use openrust::database::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use openrust::prelude::Db;
use openrust::queue::{Job, JobContext};
use openrust::support::BoxFuture;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArticleIndexJob {
    pub(crate) owner_id: i64,
    pub(crate) article_id: i64,
    pub(crate) action: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArticleIndexEntry {
    pub(crate) title: String,
}

pub(crate) const ARTICLE_STORE: &str = "example-prefix";
pub(crate) const ARTICLE_CACHE_TTL: Duration = Duration::from_secs(600);

pub(crate) fn article_tag(owner_id: i64, article_id: i64) -> String {
    format!("example-article-title:{owner_id}:{article_id}")
}

pub(crate) fn cache_key(owner_id: i64, article_id: i64) -> String {
    format!("example:article:index:{owner_id}:{article_id}")
}

impl Job for ArticleIndexJob {
    const NAME: &'static str = "example_article_index.v1";

    fn handle(&self, context: JobContext) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            let cache = context.service::<Cache>().await?;
            let store = cache.store(ARTICLE_STORE).await?;
            let key = cache_key(self.owner_id, self.article_id);
            let tagged_store = cache.default_store().await?;
            let tag = article_tag(self.owner_id, self.article_id);
            if self.action == "deleted" {
                store.remove(&key).await?;
                return tagged_store.invalidate_tags(&[&tag]).await;
            }
            let db = context.service::<Db>().await?;
            let article = article::Entity::find()
                .filter(article::Column::OwnerId.eq(self.owner_id))
                .filter(article::Column::Id.eq(self.article_id))
                .one(&db.primary_connection()?)
                .await
                .map_err(openrust::Error::internal)?;
            if let Some(article) = article {
                store
                    .set(
                        &key,
                        &ArticleIndexEntry {
                            title: article.title,
                        },
                        Some(ARTICLE_CACHE_TTL),
                    )
                    .await?;
            } else {
                store.remove(&key).await?;
            }
            tagged_store.invalidate_tags(&[&tag]).await
        })
    }
}
