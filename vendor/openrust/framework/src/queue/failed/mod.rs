//! Failed job storage contracts.

mod failed_job_provider_interface;

pub use failed_job_provider_interface::FailedJobProviderInterface;

/// A failed job with its original payload and failure details.
pub struct FailedJob {
    _private: (),
}
