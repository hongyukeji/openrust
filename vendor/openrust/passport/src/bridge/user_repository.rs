use crate::{ClientRecord, OAuthError, Passport};
use openrust::{
    auth::{AuthFuture, AuthIdentifier, AuthManager, AuthResult, Credentials},
    contracts::container::Container as _,
    database::Value,
    support::facades::Config,
};
/// Explicit compiled callback replacing PHP's optional model method discovery.
pub type PasswordGrantResolver = for<'a> fn(
    &'a str,
    &'a str,
    &'a ClientRecord,
) -> AuthFuture<'a, AuthResult<Option<AuthIdentifier>>>;

pub struct UserRepository;
#[allow(non_snake_case)]
impl UserRepository {
    pub async fn getUserEntityByUserCredentials(
        username: &str,
        password: &str,
        _grantType: &str,
        clientEntity: &ClientRecord,
    ) -> Result<Option<AuthIdentifier>, OAuthError> {
        let provider = match clientEntity
            .getAttributeValue("provider")
            .await
            .map_err(internal)?
        {
            Some(Value::Text(name)) if !name.is_empty() && name != "0" => name,
            _ => Config::get("auth.guards.api.provider")
                .map_err(internal)?
                .and_then(|v| v.as_str().map(str::to_owned))
                .ok_or_else(|| {
                    OAuthError::server_error("Unable to determine authentication provider")
                })?,
        };
        let container = openrust::container::Container::getInstance();
        let passport = container.make::<Passport>().map_err(internal)?;
        if let Some(resolver) = passport.password_resolver(&provider) {
            return resolver(username, password, clientEntity)
                .await
                .map_err(internal);
        }
        // The compiled UserProvider performs the native equivalents of findForPassport
        // and validateForPassportPasswordGrant; the default provider queries email.
        let provider = container
            .make::<AuthManager>()
            .map_err(internal)?
            .createUserProvider(&provider)
            .map_err(internal)?;
        let credentials = Credentials::new([("email", Value::Text(username.into()))], password);
        let Some(user) = provider
            .retrieve_by_credentials(&credentials)
            .await
            .map_err(internal)?
        else {
            return Ok(None);
        };
        if provider
            .validate_credentials(user.as_ref(), &credentials)
            .await
            .map_err(internal)?
        {
            Ok(Some(user.get_auth_identifier()))
        } else {
            Ok(None)
        }
    }
}
fn internal(_: impl std::fmt::Display) -> OAuthError {
    OAuthError::server_error("Password grant user provider unavailable")
}
