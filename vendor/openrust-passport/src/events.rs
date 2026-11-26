//! Typed Passport repository events, delivered through the application's dispatcher.
use openrust::contracts::events::Event;
pub struct AccessTokenCreated {
    pub token_id: String,
    pub user_id: Option<String>,
    pub client_id: String,
}
pub struct RefreshTokenCreated {
    pub refresh_token_id: String,
    pub access_token_id: String,
}
pub struct AccessTokenRevoked {
    pub token_id: String,
}
macro_rules! event {
    ($name:ident) => {
        impl $name {
            pub const NAME: &'static str =
                concat!("openrust_passport::events::", stringify!($name));
        }
        impl Event for $name {
            fn name(&self) -> &str {
                Self::NAME
            }
        }
    };
}
event!(AccessTokenCreated);
event!(RefreshTokenCreated);
event!(AccessTokenRevoked);
