//! Native adapters for Passport's protocol repositories.
mod user_repository;
pub use user_repository::{PasswordGrantResolver, UserRepository};
