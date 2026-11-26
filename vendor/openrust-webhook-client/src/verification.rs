//! 验签判定账本。每个范围、配置修订及判定每分钟只保存一行。
use crate::{Error, Result, WebhookConfig, factories::validate_reference, storage::scope_key};
use openrust::database::{
    Connection, Value,
    query::{Builder, Record},
};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct DatabaseVerificationAudit {
    connection: Connection,
    scope: String,
}

#[derive(Clone, Copy)]
pub(crate) enum VerificationDecision {
    Verified,
    UnverifiedHint,
    MissingSignature,
    InvalidSignature,
    ValidatorError,
}

impl VerificationDecision {
    fn categories(self) -> (&'static str, &'static str) {
        match self {
            Self::Verified => ("verified", "signature_accepted"),
            Self::UnverifiedHint => ("unverified_hint", "missing_signature"),
            Self::MissingSignature => ("rejected", "missing_signature"),
            Self::InvalidSignature => ("rejected", "invalid_signature"),
            Self::ValidatorError => ("rejected", "validator_error"),
        }
    }
}

impl DatabaseVerificationAudit {
    pub fn new(connection: Connection, scope: &str) -> Result<Self> {
        if !crate::hints::valid_identifier(scope) {
            return Err(Error::InvalidConfig("verification_audit.scope"));
        }
        Ok(Self {
            connection,
            scope: scope.to_owned(),
        })
    }

    pub(crate) async fn record(
        &self,
        config: &WebhookConfig,
        validator: &str,
        policy: &str,
        decision: VerificationDecision,
    ) -> Result<()> {
        if config.options()["scope"].as_str() != Some(self.scope.as_str()) {
            return Err(Error::InvalidConfig("verification_audit.scope"));
        }
        validate_reference("signature_validator", validator)?;
        validate_reference("verification_policy", policy)?;
        let minute = current_minute()?;
        let (classification, evidence) = decision.categories();
        let key = scope_key(
            &serde_json::to_string(&(
                self.scope.as_str(),
                config.name(),
                config.revision(),
                validator,
                policy,
                classification,
                evidence,
                minute,
            ))
            .map_err(|_| Error::InvalidRecord)?,
        );
        let row: Record = [
            ("audit_key".into(), key.into()),
            ("scope_key".into(), scope_key(&self.scope).into()),
            ("scope".into(), self.scope.as_str().into()),
            ("profile".into(), config.name().into()),
            ("revision".into(), config.revision().into()),
            ("validator".into(), validator.into()),
            ("policy".into(), policy.into()),
            ("classification".into(), classification.into()),
            ("evidence".into(), evidence.into()),
            ("observed_minute".into(), Value::from(minute)),
        ]
        .into_iter()
        .collect();
        self.connection
            .table("webhook_verification_audits")
            .upsert(&[row], &["audit_key"], Some(&["observed_minute"]))
            .await?;
        Ok(())
    }

    /// 预演返回当前有界批次的条数，实际清理再次核对范围与截止。
    pub async fn prune(&self, days: u32, limit: u32, pretend: bool) -> Result<u64> {
        if days > 36_500 || !(1..=500).contains(&limit) {
            return Err(Error::InvalidConfig("verification_audit.retention"));
        }
        let cutoff = current_minute()? - i64::from(days) * 1_440;
        let keys = self
            .expired(cutoff)
            .use_write_pdo()
            .order_by("observed_minute", "asc")
            .order_by("audit_key", "asc")
            .limit(u64::from(limit))
            .pluck("audit_key")
            .await?;
        if keys.is_empty() || pretend {
            return Ok(keys.len() as u64);
        }
        Ok(self
            .expired(cutoff)
            .where_in("audit_key", keys)
            .delete()
            .await?)
    }

    fn expired(&self, cutoff: i64) -> Builder<'static> {
        self.connection
            .table("webhook_verification_audits")
            .r#where("scope_key", "=", scope_key(&self.scope))
            .where_exact_text("scope", &self.scope)
            .r#where("observed_minute", "<", cutoff)
    }
}

fn current_minute() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs() / 60).ok())
        .ok_or(Error::InvalidRecord)
}
