#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Driver [{0}] not supported.")]
    UnsupportedDriver(String),
    #[error("Missing or invalid Socialite provider configuration: {0}")]
    Configuration(&'static str),
    #[error("The OAuth state is invalid.")]
    InvalidState,
    #[error("The OAuth callback does not contain a code.")]
    MissingCode,
    #[error("The OAuth provider returned an invalid response.")]
    InvalidResponse,
    #[error("The OAuth provider returned HTTP {0}.")]
    ProviderStatus(u16),
    #[error("The OAuth request failed.")]
    Http(#[from] openrust::http::client::Error),
    #[error("The Socialite application service is unavailable.")]
    Unavailable(#[from] openrust::contracts::container::BindingResolutionError),
    #[error("The OAuth Session is unavailable.")]
    Session(#[from] openrust::session::SessionError),
    #[error("The OAuth request is invalid.")]
    Request(#[from] openrust::http::HttpError),
    #[error("The OAuth redirect is invalid.")]
    Redirect(#[from] openrust::routing::UrlError),
    #[error("Cryptographic random generation failed.")]
    Random,
}
pub type Result<T> = std::result::Result<T, Error>;
