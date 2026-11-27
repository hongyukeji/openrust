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
pub(super) struct ChatAuthentication {
    pub(super) scope: Option<String>,
    pub(super) credential: Option<Credentials>,
}

impl BedrockProvider {
    pub(super) async fn chat_authentication(
        &self,
        downstream_scope: Option<&str>,
    ) -> AiResult<ChatAuthentication> {
        if downstream_scope.is_none() {
            return Ok(ChatAuthentication::default());
        }
        if let Some(scope) = self.approval_replay_scope()? {
            // 显式凭证沿用原有作用域，已签发的回放状态可继续使用。
            return Ok(ChatAuthentication {
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
        Ok(ChatAuthentication {
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
