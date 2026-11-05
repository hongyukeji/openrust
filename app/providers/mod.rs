//! 应用 Provider 类型编译入口。

pub mod app;
#[cfg(feature = "auth-fortify")]
mod fortify;
pub use app::AppServiceProvider;

#[cfg(feature = "auth-fortify")]
mod authentication_profile;
#[cfg(feature = "auth-fortify")]
pub use authentication_profile::AuthenticationProfileProvider;
