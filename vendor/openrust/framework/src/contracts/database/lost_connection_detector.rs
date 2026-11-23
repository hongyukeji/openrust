use std::error::Error;

/// Identifies a lost backend connection from an operation's error chain.
pub trait LostConnectionDetector: Send + Sync {
    fn caused_by_lost_connection(&self, error: &(dyn Error + 'static)) -> bool;
}
