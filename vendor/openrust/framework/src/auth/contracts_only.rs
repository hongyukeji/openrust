//! 共享账户契约；具名 Guard、认证 Provider 与登录流程需显式启用 `auth` feature。

pub use crate::auth_account::{
    AuthUser, Authenticatable, GenericUser, RememberTokenMutation, RememberTokenUpdate,
};
