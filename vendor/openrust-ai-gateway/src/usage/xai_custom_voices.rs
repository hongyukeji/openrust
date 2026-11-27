use super::UsageStore;
use crate::managed_keys::text;
use openrust::database::{DatabaseError, DatabaseResult, Row, Value as DbValue, query::Record};

const TABLE: &str = "ai_gateway_xai_custom_voices";

pub(crate) struct Voice {
    pub(crate) id: String,
    pub(crate) position: String,
    pub(crate) owner: String,
    pub(crate) scope: String,
    pub(crate) upstream_id: String,
}

impl Voice {
    fn decode(row: Row) -> DatabaseResult<Self> {
        let field = |name| text(&row, name);
        Ok(Self {
            id: field("id")?,
            position: field("position")?,
            owner: field("owner")?,
            scope: field("scope")?,
            upstream_id: field("upstream_id")?,
        })
    }
}

impl UsageStore {
    pub(crate) async fn reserve_xai_voice(&self, voice: &Voice) -> DatabaseResult<()> {
        let record = Record::from([
            ("id".into(), voice.id.as_str().into()),
            ("position".into(), voice.position.as_str().into()),
            ("owner".into(), voice.owner.as_str().into()),
            ("scope".into(), voice.scope.as_str().into()),
        ]);
        if self.connection.table(TABLE).insert(&[record]).await? {
            return Ok(());
        }
        Err(DatabaseError::InvalidQuery("自定义音色登记失败"))
    }

    pub(crate) async fn attach_xai_voice(&self, voice: &Voice) -> DatabaseResult<()> {
        let changed = self
            .connection
            .table(TABLE)
            .r#where("position", "=", voice.position.as_str())
            .where_null("upstream_id")
            .update(Record::from([(
                "upstream_id".into(),
                voice.upstream_id.as_str().into(),
            )]))
            .await?;
        if changed != 1 {
            return Err(DatabaseError::InvalidQuery("自定义音色登记状态已变更"));
        }
        Ok(())
    }

    pub(crate) async fn xai_voice(&self, owner: &str, id: &str) -> DatabaseResult<Option<Voice>> {
        let voice = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", id)
            .r#where("owner", "=", owner)
            .where_not_null("upstream_id")
            .first()
            .await?
            .map(Voice::decode)
            .transpose()?;
        Ok(voice.filter(|voice| voice.id == id && voice.owner == owner))
    }

    pub(crate) async fn xai_voice_page(
        &self,
        owner: &str,
        scopes: Vec<DbValue>,
        cursor: Option<&str>,
        limit: u64,
    ) -> DatabaseResult<Vec<Voice>> {
        let mut query = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("owner", "=", owner)
            .where_not_null("upstream_id")
            .where_in("scope", scopes);
        if let Some(cursor) = cursor {
            query = query.r#where("position", "<", cursor);
        }
        query
            .order_by("position", "desc")
            .limit(limit)
            .get()
            .await?
            .into_iter()
            .map(Voice::decode)
            .filter(|voice| {
                voice.as_ref().is_err() || voice.as_ref().is_ok_and(|voice| voice.owner == owner)
            })
            .collect()
    }

    pub(crate) async fn delete_xai_voice(&self, voice: &Voice) -> DatabaseResult<()> {
        self.connection
            .table(TABLE)
            .r#where("position", "=", voice.position.as_str())
            .r#where("owner", "=", voice.owner.as_str())
            .delete()
            .await?;
        Ok(())
    }
}
