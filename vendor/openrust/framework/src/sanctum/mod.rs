//! Personal access tokens for Laravel-style API authentication.

mod personal_access_token;

pub use personal_access_token::{NewAccessToken, PersonalAccessToken, PersonalAccessTokens};
