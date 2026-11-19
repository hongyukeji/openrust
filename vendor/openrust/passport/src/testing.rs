//! Explicit request-local testing substitutions; never a process-global resource server mock.
use crate::{AccessToken, ClientRecord, Passport, guards::TokenGuard};
use openrust::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    contracts::auth::Authenticatable,
    http::Request,
    support::facades::Auth,
};
#[derive(Clone)]
pub(crate) struct TestingToken(pub AccessToken);
#[allow(non_snake_case)]
impl Passport {
    pub async fn actingAs(
        &self,
        request: &mut Request,
        user: Box<dyn Authenticatable>,
        scopes: &[String],
        guard: &str,
        client: Option<&ClientRecord>,
    ) -> AuthResult<()> {
        let user_id = user.get_auth_identifier().to_string();
        let client_id = match client {
            Some(client) => client_id(client).await?,
            None => String::new(),
        };
        Auth::guard(request, Some(guard))?.set_user(user);
        Auth::shouldUse(request, guard)?;
        request.parts_mut().extensions.remove::<TestingToken>();
        TokenGuard::store(
            request,
            guard,
            AccessToken {
                oauth_access_token_id: String::new(),
                oauth_client_id: client_id,
                oauth_user_id: Some(user_id),
                oauth_scopes: Some(scopes.to_vec()),
                transient: false,
            },
        );
        Ok(())
    }
    pub async fn actingAsClient(
        &self,
        request: &mut Request,
        client: &ClientRecord,
        scopes: &[String],
        guard: &str,
    ) -> AuthResult<()> {
        if Auth::guard(request, Some(guard))?.has_user() {
            return Err(AuthError::Configuration(
                "actingAsClient requires a request without a cached user",
            ));
        }
        Auth::shouldUse(request, guard)?;
        request
            .parts_mut()
            .extensions
            .insert(TestingToken(AccessToken {
                oauth_access_token_id: String::new(),
                oauth_client_id: client_id(client).await?,
                oauth_user_id: None,
                oauth_scopes: Some(scopes.to_vec()),
                transient: false,
            }));
        Ok(())
    }
}
async fn client_id(client: &ClientRecord) -> AuthResult<String> {
    let value = client.getKey().await.ok_or(AuthError::InvalidIdentity)?;
    AuthIdentifier::try_from(&value).map(|v| v.to_string())
}
