use super::{Repository, encode, processes::string_list};
use crate::{Error, Result};
use openrust::redis::Value;
#[derive(Clone)]
pub struct TagRepository(pub(super) Repository);
impl Repository {
    pub fn tags(&self) -> TagRepository {
        TagRepository(self.clone())
    }
}
impl TagRepository {
    pub fn new(repository: Repository) -> Self {
        Self(repository)
    }
    fn key(&self, tag: &str) -> Result<String> {
        if tag.is_empty() || tag.len() > 1024 {
            return Err(Error::InvalidRecord);
        }
        Ok(self.0.key(&format!("tag:{}", encode(tag))))
    }
    pub async fn monitoring(&self) -> Result<Vec<String>> {
        let mut tags = string_list(
            self.0
                .command("SMEMBERS", &[self.0.key("monitoring")])
                .await?,
        )?;
        tags.sort();
        Ok(tags)
    }
    pub async fn monitored(&self, tags: &[String]) -> Result<Vec<String>> {
        let monitored = self.monitoring().await?;
        Ok(tags
            .iter()
            .filter(|tag| monitored.contains(tag))
            .cloned()
            .collect())
    }
    pub async fn monitor(&self, tag: &str) -> Result<()> {
        self.key(tag)?;
        self.0
            .command("SADD", &[self.0.key("monitoring"), tag.into()])
            .await?;
        Ok(())
    }
    pub async fn stop_monitoring(&self, tag: &str) -> Result<()> {
        self.key(tag)?;
        self.0
            .command("SREM", &[self.0.key("monitoring"), tag.into()])
            .await?;
        Ok(())
    }
    pub async fn add(&self, id: &str, tags: &[String]) -> Result<()> {
        self.add_with_expiry(id, tags, None).await
    }
    pub async fn add_temporary(&self, minutes: u32, id: &str, tags: &[String]) -> Result<()> {
        self.add_with_expiry(id, tags, Some(minutes.saturating_mul(60)))
            .await
    }
    async fn add_with_expiry(&self, id: &str, tags: &[String], expiry: Option<u32>) -> Result<()> {
        if !crate::label(id) {
            return Err(Error::InvalidRecord);
        }
        let time = self.0.now().await?;
        for tag in tags {
            let key = self.key(tag)?;
            self.0.eval("redis.call('ZADD',KEYS[1],ARGV[1],ARGV[2]);if tonumber(ARGV[3])>0 then redis.call('EXPIRE',KEYS[1],ARGV[3]) end;return 1",vec![key],vec![time.to_string(),id.into(),expiry.unwrap_or(0).to_string()]).await?;
        }
        Ok(())
    }
    pub async fn count(&self, tag: &str) -> Result<u64> {
        match self.0.command("ZCARD", &[self.key(tag)?]).await? {
            Value::Int(v) => u64::try_from(v).map_err(|_| Error::Redis),
            _ => Err(Error::Redis),
        }
    }
    pub async fn jobs(&self, tag: &str) -> Result<Vec<String>> {
        string_list(
            self.0
                .command("ZRANGE", &[self.key(tag)?, "0".into(), "-1".into()])
                .await?,
        )
    }
    pub async fn paginate(&self, tag: &str, start: usize, limit: usize) -> Result<Vec<String>> {
        if !(1..=1000).contains(&limit) {
            return Err(Error::InvalidRecord);
        }
        string_list(
            self.0
                .command(
                    "ZREVRANGE",
                    &[
                        self.key(tag)?,
                        start.to_string(),
                        start.saturating_add(limit - 1).to_string(),
                    ],
                )
                .await?,
        )
    }
    pub async fn forget_jobs(&self, tags: &[String], ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        for tag in tags {
            let mut args = vec![self.key(tag)?];
            args.extend_from_slice(ids);
            self.0.command("ZREM", &args).await?;
        }
        Ok(())
    }
    pub async fn forget(&self, tag: &str) -> Result<()> {
        self.0.command("DEL", &[self.key(tag)?]).await?;
        Ok(())
    }
}
