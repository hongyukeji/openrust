//! 将套件密码重置凭据绑定到真实模型身份；不改变普通Fortify应用的默认存储。
use openrust::{
    auth::{AuthError, AuthIdentifier, passwords::PasswordBrokerManager},
    contracts::{
        auth::Authenticatable, config::Repository, container::Container as _,
        foundation::Application as _,
    },
    database::{Value, eloquent::Model},
    foundation::Application,
    support::ProviderResult,
};

/// 所选用户模型的重置链接绑定；同一应用重复装配保持幂等。
pub fn bind_password_tokens<M: Model + Authenticatable>(app: &Application) -> ProviderResult {
    let config = app.container().make::<dyn Repository>()?;
    let broker = config
        .get("fortify.passwords")
        .and_then(|value| value.as_str().map(str::to_owned));
    app.container()
        .make::<PasswordBrokerManager>()?
        .bind_user_tokens_using::<M>(broker.as_deref(), "openrust.user.credentials.v1", |user| {
            let identity =
                AuthIdentifier::try_from(user.get_key().ok_or(AuthError::InvalidIdentity)?)?;
            let version = match user.get_auth_attribute("credential_version")? {
                None | Some(Value::Null) => None,
                Some(Value::Integer(value)) if value >= 0 => Some(value as u64),
                Some(Value::Unsigned(value)) => Some(value),
                _ => return Err(AuthError::Configuration("密码重置凭据版本无效")),
            };
            let connection = user.get_connection()?;
            let email = user
                .as_password_reset()
                .ok_or(AuthError::Configuration("用户必须支持密码重置"))?
                .get_email_for_password_reset();
            Ok(serde_json::to_string(&(
                connection.get_name(),
                connection.get_database_name(),
                connection.get_table_prefix(),
                connection.get_config("schema"),
                connection.get_config("search_path"),
                user.get_table(),
                identity,
                email,
                user.get_auth_password(),
                version,
            ))?)
        })?;
    Ok(())
}
