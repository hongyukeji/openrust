use openrust::database::{DatabaseError, DatabaseResult, Row, query::Record};

use super::UsageStore;
use crate::{
    managed_keys::{now, text},
    session_affinity,
};

const TABLE: &str = "ai_gateway_gemini_caches";

pub(crate) struct Cache {
    pub(crate) id: String,
    pub(crate) owner: String,
    pub(crate) scope: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) upstream_model: String,
    pub(crate) upstream_name: String,
}

impl Cache {
    fn decode(row: &Row) -> DatabaseResult<Self> {
        Ok(Self {
            id: text(row, "id")?,
            owner: text(row, "owner")?,
            scope: text(row, "scope")?,
            provider: text(row, "provider")?,
            model: text(row, "model")?,
            upstream_model: text(row, "upstream_model")?,
            upstream_name: text(row, "upstream_name")?,
        })
    }
}

impl UsageStore {
    pub(crate) async fn reserve_gemini_cache(&self, cache: &Cache) -> DatabaseResult<()> {
        if !self
            .connection
            .table(TABLE)
            .insert(&[Record::from([
                ("id".into(), cache.id.as_str().into()),
                ("owner".into(), cache.owner.as_str().into()),
                ("scope".into(), cache.scope.as_str().into()),
                ("provider".into(), cache.provider.as_str().into()),
                ("model".into(), cache.model.as_str().into()),
                ("created_at".into(), now().into()),
                (
                    "upstream_model".into(),
                    cache.upstream_model.as_str().into(),
                ),
            ])])
            .await?
        {
            return Err(DatabaseError::InvalidQuery("Gemini 缓存登记失败"));
        }
        Ok(())
    }

    pub(crate) async fn attach_gemini_cache(&self, cache: &Cache) -> DatabaseResult<()> {
        let changed = self
            .connection
            .table(TABLE)
            .r#where("id", "=", cache.id.as_str())
            .where_null("upstream_name")
            .where_null("deleted_at")
            .update(Record::from([
                ("upstream_name".into(), cache.upstream_name.as_str().into()),
                (
                    "upstream_identity".into(),
                    session_affinity::key(&[&cache.scope, &cache.upstream_name]).into(),
                ),
            ]))
            .await?;
        if changed != 1 {
            return Err(DatabaseError::InvalidQuery("Gemini 缓存登记状态已变更"));
        }
        Ok(())
    }

    pub(crate) async fn discard_gemini_cache_reservation(&self, id: &str) -> DatabaseResult<()> {
        self.connection
            .table(TABLE)
            .r#where("id", "=", id)
            .where_null("upstream_name")
            .delete()
            .await?;
        Ok(())
    }

    pub(crate) async fn gemini_cache(
        &self,
        owner: &str,
        id: &str,
    ) -> DatabaseResult<Option<Cache>> {
        let row = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("owner", "=", owner)
            .r#where("id", "=", id)
            .where_null("deleted_at")
            .where_not_null("upstream_name")
            .first()
            .await?;
        Ok(row
            .as_ref()
            .map(Cache::decode)
            .transpose()?
            .filter(|cache| cache.id == id && cache.owner == owner))
    }

    pub(crate) async fn gemini_cache_page(
        &self,
        owner: &str,
        cursor: Option<&str>,
        limit: u64,
    ) -> DatabaseResult<Vec<Cache>> {
        let mut query = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("owner", "=", owner)
            .where_null("deleted_at")
            .where_not_null("upstream_name");
        if let Some(cursor) = cursor {
            query = query.r#where("id", "<", cursor);
        }
        query
            .order_by("id", "desc")
            .limit(limit)
            .get()
            .await?
            .iter()
            .map(Cache::decode)
            .filter(|cache| {
                cache.as_ref().is_err() || cache.as_ref().is_ok_and(|cache| cache.owner == owner)
            })
            .collect()
    }

    pub(crate) async fn delete_gemini_cache(&self, cache: &Cache) -> DatabaseResult<()> {
        self.connection
            .table(TABLE)
            .r#where("id", "=", cache.id.as_str())
            .r#where("owner", "=", cache.owner.as_str())
            .where_null("deleted_at")
            .update(Record::from([("deleted_at".into(), now().into())]))
            .await?;
        Ok(())
    }
}
