#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid billing configuration: {0}")]
    Configuration(&'static str),
    #[error("invalid billing input: {0}")]
    InvalidInput(&'static str),
    #[error("billing request rejected (HTTP {0}); inspect account dashboard before retrying")]
    Rejected(u16),
    #[error("billing outcome unknown; reconcile the original operation before retrying")]
    OutcomeUnknown,
    #[error("invalid webhook signature or event")]
    Verification,
    #[error("billing database operation failed")]
    Database,
    #[error("billing resource does not belong to this customer")]
    Ownership,
    #[error("billing resource not found")]
    NotFound,
    #[error("webhook identity reused with different content")]
    EventConflict,
    #[error("operation is unsupported by this provider")]
    Unsupported,
}
pub type Result<T> = std::result::Result<T, Error>;
