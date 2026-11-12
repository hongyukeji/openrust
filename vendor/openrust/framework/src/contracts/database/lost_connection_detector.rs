use std::error::Error;

/// Identifies a lost backend connection from an operation's error chain.
pub trait LostConnectionDetector: Send + Sync {
    #[allow(non_snake_case, reason = "Matches Laravel's detector contract")]
    fn causedByLostConnection(&self, error: &(dyn Error + 'static)) -> bool;
}
