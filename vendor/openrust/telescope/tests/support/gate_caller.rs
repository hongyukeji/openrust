use openrust::{auth::AuthResult, support::facades::Gate};

pub(super) async fn allows(ability: &str) -> AuthResult<bool> {
    Gate::for_user(None)?.allows(ability, &[]).await
}
