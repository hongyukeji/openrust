//! Shares request-scoped data with views.

mod share_errors_from_session;

pub use share_errors_from_session::ShareErrorsFromSession;

pub(crate) use share_errors_from_session::{render_scope, request_scope, shared_errors};
