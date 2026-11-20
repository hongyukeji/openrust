/// Errors deliberately exclude raw requests, responses, personal data and credentials.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid payment configuration: {0}")]
    Configuration(&'static str),
    #[error("Invalid payment input: {0}")]
    InvalidInput(&'static str),
    #[error("Payment cryptography failed")]
    Cryptography,
    #[error("Payment signature or timestamp is invalid")]
    Verification,
    #[error("Payment response is malformed")]
    Protocol,
    #[error("The payment provider rejected the request")]
    Rejected,
    #[error(
        "Payment result is unknown; query using the original order/refund number before retrying"
    )]
    OutcomeUnknown,
    #[error("The callback does not match the merchant or expected order")]
    OrderMismatch,
    #[error("The payment notification transaction failed")]
    Database,
    #[error("Payment notification identity was reused with different contents")]
    NotificationConflict,
    #[error("Payment application context is unavailable")]
    Context,
}
pub type Result<T> = std::result::Result<T, Error>;
