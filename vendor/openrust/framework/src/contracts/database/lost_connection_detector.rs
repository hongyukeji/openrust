use std::error::Error;

/// Identifies a lost backend connection from an operation's error chain.
pub trait LostConnectionDetector: Send + Sync {
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn causedByLostConnection(&self, error: &(dyn Error + 'static)) -> bool;
}
