//! Typed Passport repository events, delivered through the application's dispatcher.
use openrust::contracts::events::Event;
#[allow(non_snake_case)]
pub struct AccessTokenCreated {
    pub tokenId: String,
    pub userId: Option<String>,
    pub clientId: String,
}
#[allow(non_snake_case)]
pub struct RefreshTokenCreated {
    pub refreshTokenId: String,
    pub accessTokenId: String,
}
#[allow(non_snake_case)]
pub struct AccessTokenRevoked {
    pub tokenId: String,
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
