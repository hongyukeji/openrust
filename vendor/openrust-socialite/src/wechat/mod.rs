//! WeChat login entries, grouped by application type.
mod mini_app;
mod official_account;
mod web;

pub use mini_app::{MiniApp, Session};
pub use official_account::WeixinProvider;
pub use web::WeixinWebProvider;
