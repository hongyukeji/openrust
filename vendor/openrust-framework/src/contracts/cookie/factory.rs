use crate::cookie::{Cookie, CookieError, CookieOptions};

/// Creates cookies using configured defaults and explicit overrides.
pub trait Factory: Send + Sync {
    fn make(
        &self,
        name: &str,
        value: &str,
        minutes: i64,
        options: &CookieOptions,
    ) -> Result<Cookie, CookieError>;

    fn forever(
        &self,
        name: &str,
        value: &str,
        options: &CookieOptions,
    ) -> Result<Cookie, CookieError>;

    fn forget(
        &self,
        name: &str,
        path: Option<&str>,
        domain: Option<&str>,
    ) -> Result<Cookie, CookieError>;
}
