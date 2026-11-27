use std::time::Duration;

use aws_config::identity::IdentityCache;
use aws_sdk_bedrockruntime::config::{Credentials, ProvideCredentials};
use aws_sdk_sts::{
    config::{Builder, retry::RetryConfig, timeout::TimeoutConfig},
    operation::get_caller_identity::GetCallerIdentityOutput,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{AiError, AiResult, StepTextGateway};

use super::BedrockProvider;

#[derive(Default)]
pub(super) struct ReplayPrincipal(Mutex<Option<([u8; 32], String)>>);

#[derive(Default)]
pub(super) struct ReplayAuthentication {
    pub(super) scope: Option<String>,
    credential: Option<Credentials>,
}

const BINDING: &str = "bedrock_replay_binding";

pub(super) enum ReplayBinding {
    Scope(String),
    Credential(String),
}

impl ReplayBinding {
    pub(super) fn record(self, content: &mut Option<Value>) {
        if let Some(Value::Object(content)) = content {
            let value = match self {
                Self::Scope(scope) => json!({"scope":scope}),
                Self::Credential(credential) => json!({"credential":credential}),
            };
            content.insert(BINDING.into(), value);
        }
    }
}

pub(super) fn step_scope(content: Option<&Value>) -> Option<&str> {
    content?.get(BINDING)?.get("scope")?.as_str()
}

impl ReplayAuthentication {
    pub(super) fn pin(
        &self,
        config: aws_sdk_bedrockruntime::config::Builder,
    ) -> aws_sdk_bedrockruntime::config::Builder {
        match &self.credential {
            // 身份核对和推理使用同一份凭证，避免签名历史漂移到其他调用者。
            Some(credential) => config
                .credentials_provider(credential.clone())
                .identity_cache(IdentityCache::no_cache()),
            None => config,
        }
    }

    pub(super) fn binding(&self) -> Option<ReplayBinding> {
        if let Some(scope) = &self.scope {
            return Some(ReplayBinding::Scope(scope.clone()));
        }
        self.credential_binding().map(ReplayBinding::Credential)
    }

    fn credential_binding(&self) -> Option<String> {
        self.credential
            .as_ref()
            .map(|credential| format!("{:x}", Sha256::digest(credential_fingerprint(credential))))
    }

    pub(super) fn allows(&self, content: Option<&Value>) -> bool {
        let Some(binding) = content.and_then(|content| content.get(BINDING)) else {
            // 旧审批快照使用独立的显式凭证范围，由恢复入口验证。
            return content.is_none() || (self.credential.is_none() && self.scope.is_some());
        };
        if let Some(scope) = binding["scope"].as_str() {
            return self.scope.as_deref() == Some(scope);
        }
        binding["credential"]
            .as_str()
            .is_some_and(|value| self.credential_binding().as_deref() == Some(value))
    }
}

impl BedrockProvider {
    pub(super) async fn replay_authentication(
        &self,
        enabled: bool,
    ) -> AiResult<ReplayAuthentication> {
        if !enabled {
            return Ok(ReplayAuthentication::default());
        }
        if let Some(scope) = self.approval_replay_scope()? {
            // 显式凭证沿用原有作用域，已签发的回放状态可继续使用。
            return Ok(ReplayAuthentication {
                scope: Some(scope),
                credential: None,
            });
        }

        let iam = self.iam_config().await?;
        let provider = iam
            .credentials_provider()
            .ok_or(AiError::MissingCredential)?;
        let credential =
            tokio::time::timeout(Duration::from_secs(10), provider.provide_credentials())
                .await
                .map_err(|_| AiError::Timeout)?
                .map_err(|_| AiError::MissingCredential)?;
        let fingerprint = credential_fingerprint(&credential);
        let mut cached = self.replay_principal.0.lock().await;
        let scope = if let Some((previous, scope)) = cached.as_ref()
            && previous == &fingerprint
        {
            Some(scope.clone())
        } else {
            let config = Builder::from(iam)
                .credentials_provider(credential.clone())
                .identity_cache(IdentityCache::no_cache())
                .retry_config(RetryConfig::standard().with_max_attempts(1))
                .timeout_config(
                    TimeoutConfig::builder()
                        .operation_timeout(Duration::from_secs(5))
                        .build(),
                )
                .build();
            let identity = aws_sdk_sts::Client::from_conf(config)
                .get_caller_identity()
                .send()
                .await;
            let scope = identity.ok().and_then(|identity| {
                let principal = principal(&identity, &fingerprint)?;
                let value = json!([
                    "bedrock-iam-principal-v1",
                    self.connection.name(),
                    self.region().as_ref(),
                    self.settings.url,
                    principal,
                ]);
                Some(format!("{:x}", Sha256::digest(value.to_string())))
            });
            *cached = scope.as_ref().map(|scope| (fingerprint, scope.clone()));
            scope
        };
        Ok(ReplayAuthentication {
            scope,
            credential: Some(credential),
        })
    }
}

fn credential_fingerprint(credential: &Credentials) -> [u8; 32] {
    let mut digest = Sha256::new();
    for value in [
        credential.access_key_id(),
        credential.secret_access_key(),
        credential.session_token().unwrap_or_default(),
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.finalize().into()
}

fn principal(identity: &GetCallerIdentityOutput, fingerprint: &[u8; 32]) -> Option<Value> {
    let account = identity.account()?;
    if account.len() != 12 || !account.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let arn = identity.arn()?;
    let user = identity.user_id().filter(|user| !user.is_empty())?;
    let parts = arn.splitn(6, ':').collect::<Vec<_>>();
    let ["arn", partition, service, "", owner, resource] = parts.as_slice() else {
        return None;
    };
    if *owner != account || partition.is_empty() {
        return None;
    }

    match *service {
        "sts" if resource.starts_with("assumed-role/") => {
            let role = resource.strip_prefix("assumed-role/")?;
            let (name, session_name) = role.rsplit_once('/')?;
            let (id, session_id) = user.split_once(':')?;
            if name.is_empty()
                || session_name.is_empty()
                || session_name != session_id
                || !id.starts_with("AROA")
                || !id.bytes().all(|byte| byte.is_ascii_alphanumeric())
            {
                return None;
            }
            Some(json!([partition, account, name, id]))
        }
        "iam" if *resource == "root" || resource.starts_with("user/") => Some(json!([arn, user])),
        "sts" if resource.starts_with("federated-user/") => Some(json!([arn, user, fingerprint])),
        _ => None,
    }
}
