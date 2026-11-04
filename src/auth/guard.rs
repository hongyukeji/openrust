//! 配置具名 Guard、用户 Provider 与请求内认证生命周期。

mod contracts;
mod database;
mod manager;
mod registration;
mod session;

pub use contracts::{
    AuthUser, Authenticatable, Credentials, GenericUser, Guard, StatefulGuard, UserProvider,
};
pub use database::DatabaseUserProvider;
pub use manager::{
    AuthDefaults, AuthDrivers, AuthManager, GuardConfig, GuardContext, UserProviderConfig,
};
pub use session::{Attempting, Authenticated, Failed, Login, Logout, SessionGuard, Validated};

pub(super) use registration::register;
pub(crate) use registration::{AuthRequestContext, install_request};

pub(crate) use registration::session_user_id;
