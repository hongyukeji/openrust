//! 配置具名 Guard、用户 Provider 与请求内认证生命周期。

mod contracts;
mod database;
mod manager;
mod proof;
mod registration;
mod remember;
mod session;

pub use contracts::{
    AuthUser, Authenticatable, Credentials, GenericUser, Guard, StatefulGuard, UserProvider,
};
pub use database::DatabaseUserProvider;
pub use manager::{
    AuthDefaults, AuthDrivers, AuthManager, GuardConfig, GuardContext, UserProviderConfig,
};
pub use proof::{
    FactorManagementContext, FactorReceipt, FactorVerificationMethod, SessionFactorProof,
};
pub use remember::{RememberTokenMutation, RememberTokenUpdate};
pub use session::{Attempting, Authenticated, Failed, Login, Logout, SessionGuard, Validated};

pub(super) use registration::register;
pub(crate) use registration::{AuthRequestContext, install_request};

#[cfg(feature = "auth-fortify")]
pub(crate) use proof::session_factor_key;
pub(crate) use registration::session_user_id;
