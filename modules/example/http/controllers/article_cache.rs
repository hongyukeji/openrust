//! 命名缓存只保存可重建标题；每次请求先在主库检查账户权限。

use crate::modules::example::jobs::article_index::{
    ARTICLE_CACHE_TTL, ARTICLE_STORE, ArticleIndexEntry, article_tag, cache_key,
};
use crate::modules::example::models::article;
use openrust::auth::CurrentUser;
use openrust::cache::Cache;
use openrust::db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QuerySelect};
use openrust::http::{Inject, Json, Path};
use openrust::{Db, Error, Result};
use std::time::Duration;

#[derive(serde::Serialize)]
pub(crate) struct ArticleTitle {
    title: String,
    cache_hit: bool,
    consistency: &'static str,
}
#[derive(serde::Serialize)]
pub(crate) struct ArticleSWRTitle {
    title: String,
    consistency: &'static str,
}
pub(crate) async fn show(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(cache): Inject<Cache>,
    Path(id): Path<i64>,
) -> Result<Json<ArticleTitle>> {
    let authorized = article::Entity::find_by_id(id)
        .select_only()
        .column(article::Column::Id)
        .filter(article::Column::OwnerId.eq(user.id()))
        .into_tuple::<(i64,)>()
        .one(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?;
    if authorized.is_none() {
        return Err(Error::NotFound("article not found".into()));
    }
    let store = cache.store(ARTICLE_STORE).await?;
    let key = cache_key(user.id(), id);
    let (entry, cache_hit) = if let Some(entry) = store.get::<ArticleIndexEntry>(&key).await? {
        (entry, true)
    } else {
        let model = article::Entity::find_by_id(id)
            .filter(article::Column::OwnerId.eq(user.id()))
            .one(&db.primary_connection()?)
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::NotFound("article not found".into()))?;
        let entry = ArticleIndexEntry { title: model.title };
        store.set(&key, &entry, Some(ARTICLE_CACHE_TTL)).await?;
        (entry, false)
    };
    // 写后队列更新与 TTL 过期允许短暂旧标题；该数据从不承担授权。
    Ok(Json(ArticleTitle {
        title: entry.title,
        cache_hit,
        consistency: "eventual",
    }))
}

pub(crate) async fn show_swr(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Inject(cache): Inject<Cache>,
    Path(id): Path<i64>,
) -> Result<Json<ArticleSWRTitle>> {
    let owner_id = user.id();
    let authorized = article::Entity::find_by_id(id)
        .select_only()
        .column(article::Column::Id)
        .filter(article::Column::OwnerId.eq(owner_id))
        .into_tuple::<(i64,)>()
        .one(&db.primary_connection()?)
        .await
        .map_err(Error::internal)?;
    if authorized.is_none() {
        return Err(Error::NotFound("article not found".into()));
    }
    let store = cache.default_store().await?;
    let tag = article_tag(owner_id, id);
    let tagged = store.tagged(&[&tag])?;
    let entry = tagged
        .get_stale_while_revalidate(
            &cache_key(owner_id, id),
            "example-article-title-v1",
            Duration::from_secs(30),
            Duration::from_secs(120),
            move || {
                let db = db.clone();
                async move {
                    let model = article::Entity::find_by_id(id)
                        .filter(article::Column::OwnerId.eq(owner_id))
                        .one(&db.primary_connection()?)
                        .await
                        .map_err(Error::internal)?
                        .ok_or_else(|| Error::NotFound("article not found".into()))?;
                    Ok(ArticleIndexEntry { title: model.title })
                }
            },
        )
        .await?;
    Ok(Json(ArticleSWRTitle {
        title: entry.title,
        consistency: "tagged-swr-eventual",
    }))
}
