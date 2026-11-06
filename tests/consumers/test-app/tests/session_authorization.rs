//! 仅验证独立 consumer 能调用公开 Session 截止时间接口，不模拟认证记录。
//!
//! 这里不运行登录、持久 store 或撤销行为；这些行为须由真实登录后的双库 TCP
//! 回归证明。默认无 feature 的 consumer 不编译本文件内的认证依赖。
#![cfg(feature = "session")]

use openrust::auth::{Session, SessionStoreHandle};
use openrust::core::Result;
use openrust::support::BoxFuture;
use std::time::SystemTime;

// 从独立 crate 实际类型检查 await 路径，禁止依赖框架私有 Session key。
fn persisted_authorization_deadline<'a>(
    store: &'a SessionStoreHandle,
    session: &'a Session,
    authenticated_user_id: i64,
) -> BoxFuture<'a, Result<SystemTime>> {
    Box::pin(async move { store.authorized_until(session, authenticated_user_id).await })
}

#[test]
fn session_authorization_deadline_is_publicly_callable() {
    // 只绑定签名，不构造未经认证的 Session 冒充已登录账户，也不据此声称行为通过。
    let _public_call: for<'a> fn(
        &'a SessionStoreHandle,
        &'a Session,
        i64,
    ) -> BoxFuture<'a, Result<SystemTime>> = persisted_authorization_deadline;
}
