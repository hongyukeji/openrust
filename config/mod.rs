use std::{collections::BTreeMap, path::Path};

use openrust::config::{ConfigError, Environment, Value};

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
mod reverb;
mod runtime;
mod services;
mod session;
mod view;

pub(crate) fn load(
    base_path: &Path,
    env: &Environment,
) -> Result<BTreeMap<String, Value>, ConfigError> {
    let app = app::load(env)?;
    let database = database::load(base_path, env, app.name)?;
    let queue = queue::load(env, &database["default"])?;
    Ok(BTreeMap::from([
        ("auth".to_owned(), auth::load(env)?),
        ("broadcasting".to_owned(), broadcasting::load(env)?),
        ("cache".to_owned(), cache::load(base_path, env, app.name)?),
        ("concurrency".to_owned(), concurrency::load(env)?),
        ("cors".to_owned(), cors::load()),
        ("database".to_owned(), database),
        (
            "filesystems".to_owned(),
            filesystems::load(base_path, env, &app)?,
        ),
        ("grpc".to_owned(), grpc::load(env)?),
        ("hashing".to_owned(), hashing::load(env)?),
        ("images".to_owned(), images::load(env)?),
        ("logging".to_owned(), logging::load(base_path, env)?),
        ("mail".to_owned(), mail::load(base_path, env, &app.url)?),
        ("queue".to_owned(), queue),
        ("reverb".to_owned(), reverb::load(env)?),
        ("runtime".to_owned(), runtime::load(env)?),
        ("services".to_owned(), services::load(env)?),
        (
            "session".to_owned(),
            session::load(base_path, env, app.name)?,
        ),
        ("view".to_owned(), view::load(base_path, env)?),
        ("app".to_owned(), app.config),
    ]))
}
