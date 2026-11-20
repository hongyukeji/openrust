//! Native WebAuthn ceremonies and account persistence for OpenRust.
mod webauthn;
pub use webauthn::{AuthenticationOptions, RegistrationOptions, StoredCredential, WebAuthn};

#[derive(Debug, thiserror::Error)]
pub enum PasskeyError {
    #[error("Invalid passkey configuration: {0}")]
    Configuration(&'static str),
    #[error("Invalid credential format.")]
    Format,
    #[error("Unable to verify passkey. Please try again.")]
    Verification,
    #[error("Passkey session expired. Please try again.")]
    Expired,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type PasskeyResult<T> = Result<T, PasskeyError>;

pub mod contracts;
mod passkey;
mod passkeys;
pub use contracts::PasskeyUser as PasskeyAuthenticatable;
pub use passkey::Passkey;
pub use passkeys::{AuthorizeLoginCallback, Passkeys};

pub(crate) fn internal(
    error: impl std::error::Error + Send + Sync + 'static,
) -> openrust::http::HttpError {
    openrust::http::HttpError::Internal {
        source: Box::new(error),
    }
}

pub mod actions;
pub mod events;
pub(crate) fn invalid(message: &str) -> openrust::http::HttpError {
    let mut errors = openrust::support::MessageBag::default();
    let message = openrust::support::facades::Lang::get(message, &Default::default(), None)
        .unwrap_or_else(|_| message.into());
    errors.add("credential", message);
    openrust::http::HttpError::Validation(Box::new(openrust::validation::ValidationException::new(
        errors,
    )))
}

pub mod http;

mod provider;
pub use provider::PasskeysServiceProvider;

mod routes;
