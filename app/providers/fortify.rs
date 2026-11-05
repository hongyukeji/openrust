//! 应用通过 `Fortify` 公开回调选择同一 `Provider` 中的组合登录标识。

use openrust::auth::fortify::{FortifyCallbacks, FortifyConfig};
use openrust::auth::{AuthManager, AuthUser, Credentials, UserProvider};
use openrust::core::RegisterContext;
use openrust::{Error, Result};
use serde_json::{Map, Value};
use std::{sync::Arc, time::Duration};

pub(super) async fn register(app: &mut RegisterContext<'_>) -> Result<()> {
    app.instance(Arc::new(openrust::auth::fortify::RequireFactorAuthority))
        .await?;
    if !app.is_bound::<dyn openrust::auth::ApiLoginAuditSink>() {
        app.singleton::<dyn openrust::auth::ApiLoginAuditSink, _>(|resolver| {
            Box::pin(async move {
                let db = resolver.resolve::<openrust::db::Db>().await?;
                let audit = resolver.resolve::<openrust::audit::Audit>().await?;
                Ok(
                    Arc::new(openrust::auth::fortify::DatabaseApiLoginAudit::new(
                        (*db).clone(),
                        (*audit).clone(),
                    )) as Arc<dyn openrust::auth::ApiLoginAuditSink>,
                )
            })
        })
        .await?;
    }

    app.scoped::<FortifyCallbacks, _>(|resolver| {
        Box::pin(async move {
            let config = resolver.section::<FortifyConfig>("fortify")?;
            let manager = resolver.resolve::<AuthManager>().await?;
            let guard_name = if resolver.is_bound::<openrust::auth::fortify::FortifyJwtApi>()? {
                resolver
                    .section::<openrust::auth::fortify::FortifyJwtApiConfig>("fortify-api")?
                    .guard
            } else {
                config.guard.clone()
            };
            let guards = resolver
                .section::<std::collections::BTreeMap<String, openrust::auth::GuardConfig>>(
                    "auth.guards",
                )?;
            let provider_name = guards
                .get(&guard_name)
                .and_then(|guard| guard.provider.as_deref())
                .ok_or(Error::Unauthorized)?;
            // 首因子与密码确认只解析同源Provider，不创建Web Session Guard。
            let provider = manager
                .create_user_provider(Some(provider_name))
                .await?
                .ok_or(Error::Unauthorized)?;
            let rehash = resolver.section::<bool>("hashing.rehash_on_login")?;
            let login_provider = provider.clone();
            let username = config.username;
            let callbacks = FortifyCallbacks::new()
                .authenticate_using(Arc::new(move |_, credentials| {
                    let provider = login_provider.clone();
                    let username = username.clone();
                    Box::pin(async move {
                        let deadline =
                            openrust::tokio::time::Instant::now() + Duration::from_millis(200);
                        let result =
                            authenticate(provider.as_ref(), credentials, &username, rehash).await;
                        openrust::tokio::time::sleep_until(deadline).await;
                        result
                    })
                }))
                .confirm_passwords_using(Arc::new(move |user, password| {
                    let provider = provider.clone();
                    Box::pin(async move {
                        let mut attributes = Map::new();
                        attributes.insert(
                            "password".into(),
                            password.map_or(Value::Null, Value::String),
                        );
                        provider
                            .validate_credentials(user.as_ref(), &Credentials::new(attributes))
                            .await
                    })
                }));
            Ok(Arc::new(callbacks))
        })
    })
    .await
}

async fn authenticate(
    provider: &dyn UserProvider,
    credentials: &Credentials,
    field: &str,
    rehash: bool,
) -> Result<Option<AuthUser>> {
    let Some(account) = credentials
        .attributes()
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let Some(user) = select_account(provider, account).await? else {
        return Ok(None);
    };
    if !provider
        .validate_credentials(user.as_ref(), credentials)
        .await?
    {
        return Ok(None);
    }
    if rehash {
        provider
            .rehash_password_if_required(user.as_ref(), credentials, false)
            .await?;
    }
    Ok(Some(user))
}

async fn select_account(provider: &dyn UserProvider, account: &str) -> Result<Option<AuthUser>> {
    let mut selected: Option<AuthUser> = None;
    for field in ["username", "email", "phone"] {
        // 本应用不猜手机号区域，只匹配已经以E.164保存的值；同值也仍可作为用户名。
        if field == "phone" && !is_e164(account) {
            continue;
        }
        let credentials = Credentials::new(Map::from_iter([(
            field.into(),
            Value::String(account.into()),
        )]));
        let Some(user) = provider.retrieve_by_credentials(&credentials).await? else {
            continue;
        };
        // 明确选择大小写敏感的逐字匹配，防不同后端collation改变应用账户归属。
        if user.auth_attribute(field)?.as_ref().and_then(Value::as_str) != Some(account) {
            continue;
        }
        if let Some(previous) = &selected {
            if previous.auth_identifier()? != user.auth_identifier()? {
                return Ok(None);
            }
        } else {
            selected = Some(user);
        }
    }
    Ok(selected)
}

fn is_e164(account: &str) -> bool {
    let Some(digits) = account.strip_prefix('+') else {
        return false;
    };
    (1..=15).contains(&digits.len())
        && !digits.starts_with('0')
        && digits.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use openrust::auth::{Authenticatable, GenericUser};
    use openrust::core::BoxFuture;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Accounts {
        users: Vec<AuthUser>,
        validations: AtomicUsize,
    }
    impl UserProvider for Accounts {
        fn retrieve_by_id<'a>(&'a self, id: &'a Value) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async move {
                for user in &self.users {
                    if user.auth_identifier()? == *id {
                        return Ok(Some(user.clone()));
                    }
                }
                Ok(None)
            })
        }
        fn retrieve_by_token<'a>(
            &'a self,
            _: &'a Value,
            _: &'a str,
        ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async { Ok(None) })
        }
        fn update_remember_token<'a>(
            &'a self,
            _: &'a dyn Authenticatable,
            _: &'a str,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Err(Error::Unauthorized) })
        }
        fn retrieve_by_credentials<'a>(
            &'a self,
            credentials: &'a Credentials,
        ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
            Box::pin(async move {
                for user in &self.users {
                    for (field, value) in credentials.attributes() {
                        if let (Some(actual), Some(expected)) = (
                            user.auth_attribute(field)?.as_ref().and_then(Value::as_str),
                            value.as_str(),
                        ) {
                            // 模拟MySQL常见不区分大小写的collation，应用必须自行复核。
                            if actual.eq_ignore_ascii_case(expected) {
                                return Ok(Some(user.clone()));
                            }
                        }
                    }
                }
                Ok(None)
            })
        }
        fn validate_credentials<'a>(
            &'a self,
            _: &'a dyn Authenticatable,
            credentials: &'a Credentials,
        ) -> BoxFuture<'a, Result<bool>> {
            Box::pin(async move {
                self.validations.fetch_add(1, Ordering::SeqCst);
                Ok(credentials
                    .attributes()
                    .get("password")
                    .and_then(Value::as_str)
                    == Some("correct"))
            })
        }
        fn rehash_password_if_required<'a>(
            &'a self,
            _: &'a dyn Authenticatable,
            _: &'a Credentials,
            _: bool,
        ) -> BoxFuture<'a, Result<()>> {
            Box::pin(async { Ok(()) })
        }
    }
    fn accounts(values: &[Value]) -> Accounts {
        Accounts {
            users: values
                .iter()
                .map(|value| {
                    Arc::new(GenericUser::new(
                        value.as_object().cloned().unwrap_or_default(),
                    )) as AuthUser
                })
                .collect(),
            validations: AtomicUsize::new(0),
        }
    }
    fn credentials(account: &str, password: &str) -> Credentials {
        Credentials::new(Map::from_iter([
            ("account".into(), Value::String(account.into())),
            ("password".into(), Value::String(password.into())),
        ]))
    }
    #[tokio::test]
    async fn optional_contacts_and_three_entries_use_one_typed_identity() -> Result<()> {
        let provider = accounts(&[
            serde_json::json!({"id":"uuid-only","username":"local-only","email":null,"phone":null}),
            serde_json::json!({"id":42,"username":"Member","email":"Member@example.test","phone":"+8613800000000"}),
        ]);
        let local = authenticate(
            &provider,
            &credentials("local-only", "correct"),
            "account",
            false,
        )
        .await?
        .ok_or(Error::Unauthorized)?;
        assert_eq!(local.auth_identifier()?, serde_json::json!("uuid-only"));
        for entry in ["Member", "Member@example.test", "+8613800000000"] {
            let user = authenticate(&provider, &credentials(entry, "correct"), "account", true)
                .await?
                .ok_or(Error::Unauthorized)?;
            assert_eq!(user.auth_identifier()?, serde_json::json!(42));
        }
        assert!(
            authenticate(
                &provider,
                &credentials("member", "correct"),
                "account",
                false
            )
            .await?
            .is_none()
        );
        assert!(
            authenticate(&provider, &credentials("Member", "wrong"), "account", false)
                .await?
                .is_none()
        );
        assert!(
            authenticate(
                &provider,
                &credentials("13800000000", "correct"),
                "account",
                false
            )
            .await?
            .is_none()
        );
        assert!(!is_e164("+013800000000"));
        assert!(!is_e164("+86 13800000000"));
        assert!(!is_e164("+１２３４５６７８９"));
        Ok(())
    }
    #[tokio::test]
    async fn cross_field_conflict_is_rejected_before_password_and_ids_are_not_coerced() -> Result<()>
    {
        let provider = accounts(&[
            serde_json::json!({"id":42,"username":"collision@example.test","email":null,"phone":null}),
            serde_json::json!({"id":"42","username":null,"email":"collision@example.test","phone":null}),
        ]);
        assert!(
            authenticate(
                &provider,
                &credentials("collision@example.test", "correct"),
                "account",
                false
            )
            .await?
            .is_none()
        );
        assert_eq!(provider.validations.load(Ordering::SeqCst), 0);
        assert!(
            authenticate(&provider, &credentials("", "correct"), "account", false)
                .await?
                .is_none()
        );
        assert_eq!(provider.validations.load(Ordering::SeqCst), 0);
        Ok(())
    }
}
