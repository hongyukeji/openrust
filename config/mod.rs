use std::{collections::BTreeMap, path::Path};

use openrust::config::{ConfigError, Environment, Value};

mod api;
pub(crate) mod app;
mod auth;
mod broadcasting;
mod cache;
mod concurrency;
mod cors;
mod database;
mod env;
mod filesystems;
mod grpc;
mod hashing;
mod images;
mod logging;
mod mail;
mod queue;
mod services;
mod session;
mod sms;
mod view;

/*
 * 按依赖顺序装配应用配置。
 *
 * 先加载应用配置，为缓存、数据库和会话提供应用名称，为文件存储和邮件提供 URL。
 * 数据库配置加载后，再用默认数据库连接装配队列配置。
 */
pub(crate) fn load(
    base_path: &Path,
    env: &Environment,
) -> Result<BTreeMap<String, Value>, ConfigError> {
    let app = app::load(env)?;
    let database = database::load(base_path, env, app.name)?;
    let queue = queue::load(env, &database["default"])?;
    Ok(BTreeMap::from([
        ("api".to_owned(), api::load(env)?),
        ("auth".to_owned(), auth::load(env)?),
        ("broadcasting".to_owned(), broadcasting::load(env)?),
        ("cache".to_owned(), cache::load(base_path, env, app.name)?),
        ("concurrency".to_owned(), concurrency::load(env)?),
        ("cors".to_owned(), cors::load(env)?),
        ("database".to_owned(), database),
        (
            "filesystems".to_owned(),
            filesystems::load(base_path, env, &app)?,
        ),
        ("grpc".to_owned(), grpc::load(env)?),
        ("hashing".to_owned(), hashing::load(env)?),
        ("images".to_owned(), images::load(env)?),
        (
            "logging".to_owned(),
            logging::load(base_path, env, app.name)?,
        ),
        ("mail".to_owned(), mail::load(base_path, env, &app.url)?),
        ("queue".to_owned(), queue),
        ("services".to_owned(), services::load(env)?),
        (
            "session".to_owned(),
            session::load(base_path, env, app.name)?,
        ),
        ("sms".to_owned(), sms::load(env)?),
        ("view".to_owned(), view::load(base_path, env)?),
        ("app".to_owned(), app.config),
    ]))
}
