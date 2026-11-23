use super::{Session, SessionResult};
use crate::{
    contracts::session::Session as _,
    http::HttpError,
    support::facades::{Facade, Request as RequestFacade},
};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

// 所有访问借用请求已加载的 Store，禁止另建默认驱动或缓存请求数据。
impl Session {
    pub fn all() -> Result<BTreeMap<String, Value>, HttpError> {
        RequestFacade::session(|store| store.all().clone())
    }

    pub fn only(keys: &[&str]) -> Result<BTreeMap<String, Value>, HttpError> {
        RequestFacade::session(|store| store.only(keys))
    }

    pub fn except(keys: &[&str]) -> Result<BTreeMap<String, Value>, HttpError> {
        RequestFacade::session(|store| store.except(keys))
    }

    pub fn get(key: &str) -> Result<Option<Value>, HttpError> {
        RequestFacade::session(|store| store.get(key).cloned())
    }

    pub fn get_or(key: &str, default: Value) -> Result<Value, HttpError> {
        RequestFacade::session(|store| store.get(key).cloned().unwrap_or(default))
    }

    pub fn pull(key: &str) -> Result<Option<Value>, HttpError> {
        RequestFacade::session(|store| store.pull(key))
    }

    pub fn remove(key: &str) -> Result<Option<Value>, HttpError> {
        RequestFacade::session(|store| store.pull(key))
    }

    pub fn exists(key: &str) -> Result<bool, HttpError> {
        RequestFacade::session(|store| store.exists(key))
    }

    pub fn missing(key: &str) -> Result<bool, HttpError> {
        RequestFacade::session(|store| store.missing(key))
    }

    pub fn has(key: &str) -> Result<bool, HttpError> {
        RequestFacade::session(|store| store.has(key))
    }

    pub fn has_any(keys: &[&str]) -> Result<bool, HttpError> {
        RequestFacade::session(|store| store.has_any(keys))
    }

    pub fn forget(keys: &[&str]) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.forget(keys))
    }

    pub fn flush() -> Result<(), HttpError> {
        RequestFacade::session(|store| store.flush())
    }

    pub fn reflash() -> Result<(), HttpError> {
        RequestFacade::session(|store| store.reflash())
    }

    pub fn keep(keys: &[&str]) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.keep(keys))
    }

    pub fn has_old_input(key: Option<&str>) -> Result<bool, HttpError> {
        RequestFacade::session(|store| store.has_old_input(key))
    }

    pub fn get_old_input(key: Option<&str>) -> Result<Option<Value>, HttpError> {
        RequestFacade::session(|store| store.get_old_input(key).cloned())
    }

    pub fn get_id() -> Result<String, HttpError> {
        RequestFacade::session(|store| store.get_id().to_owned())
    }

    pub fn id() -> Result<String, HttpError> {
        RequestFacade::session(|store| store.get_id().to_owned())
    }

    pub fn get_name() -> Result<String, HttpError> {
        RequestFacade::session(|store| store.get_name().to_owned())
    }

    pub fn is_started() -> Result<bool, HttpError> {
        RequestFacade::session(|store| store.is_started())
    }

    pub fn token() -> Result<Option<String>, HttpError> {
        RequestFacade::session(|store| store.token().map(str::to_owned))
    }

    pub fn previous_url() -> Result<Option<String>, HttpError> {
        RequestFacade::session(|store| store.previous_url().map(str::to_owned))
    }

    pub fn has_previous_uri() -> Result<bool, HttpError> {
        RequestFacade::session(|store| store.previous_url().is_some())
    }

    pub fn previous_route() -> Result<Option<String>, HttpError> {
        RequestFacade::session(|store| store.previous_route().map(str::to_owned))
    }

    pub fn put(key: &str, value: Value) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.put(key, value))?.map_err(Into::into)
    }

    pub fn put_many(values: &[(String, Value)]) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.put_many(values))?.map_err(Into::into)
    }

    pub fn replace(values: &[(String, Value)]) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.replace(values))?.map_err(Into::into)
    }

    pub fn push(key: &str, value: Value) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.push(key, value))?.map_err(Into::into)
    }

    pub fn increment(key: &str, amount: i64) -> Result<Value, HttpError> {
        RequestFacade::session(|store| store.increment(key, amount))?.map_err(Into::into)
    }

    pub fn decrement(key: &str, amount: i64) -> Result<Value, HttpError> {
        RequestFacade::session(|store| store.decrement(key, amount))?.map_err(Into::into)
    }

    pub fn flash(key: &str, value: Value) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.flash(key, value))?.map_err(Into::into)
    }

    pub fn now(key: &str, value: Value) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.now(key, value))?.map_err(Into::into)
    }

    pub fn age_flash_data() -> Result<(), HttpError> {
        RequestFacade::session(|store| store.age_flash_data())
    }

    pub fn flash_input(input: Value) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.flash_input(input))?.map_err(Into::into)
    }

    pub fn set_id(id: Option<&str>) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.set_id(id))?.map_err(Into::into)
    }

    pub fn set_name(name: &str) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.set_name(name))?.map_err(Into::into)
    }

    pub fn regenerate_token() -> Result<(), HttpError> {
        RequestFacade::session(|store| store.regenerate_token())?.map_err(Into::into)
    }

    pub fn set_previous_url(url: &str) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.set_previous_url(url))?.map_err(Into::into)
    }

    pub fn set_previous_route(name: Option<&str>) -> Result<(), HttpError> {
        RequestFacade::session(|store| store.set_previous_route(name))?.map_err(Into::into)
    }

    pub fn password_confirmed() -> Result<(), HttpError> {
        RequestFacade::session(|store| store.password_confirmed())?.map_err(Into::into)
    }

    pub fn remember(
        key: &str,
        calculate: impl FnOnce() -> SessionResult<Value>,
    ) -> Result<Value, HttpError> {
        RequestFacade::session(|store| store.remember(key, calculate))?.map_err(Into::into)
    }

    pub fn cache() -> Result<Arc<crate::cache::Repository>, HttpError> {
        RequestFacade::session(|store| store.cache())?.map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })
    }

    pub fn previous_uri() -> Result<Option<crate::support::Uri>, HttpError> {
        RequestFacade::session(|store| store.previous_uri())?.map_err(|source| {
            HttpError::Internal {
                source: Box::new(source),
            }
        })
    }

    pub fn is_valid_id(id: Option<&str>) -> bool {
        id.is_some_and(super::store::valid_id)
    }
    pub async fn start() -> Result<(), HttpError> {
        RequestFacade::get_facade_root()?
            .with_async(move |request| {
                Box::pin(async move {
                    let store = request.session_mut()?;
                    store.start().await.map_err(Into::into)
                })
            })
            .await
    }

    pub async fn save() -> Result<(), HttpError> {
        RequestFacade::get_facade_root()?
            .with_async(move |request| {
                Box::pin(async move {
                    let store = request.session_mut()?;
                    store.save().await.map_err(Into::into)
                })
            })
            .await
    }

    pub async fn migrate(destroy: bool) -> Result<(), HttpError> {
        RequestFacade::get_facade_root()?
            .with_async(move |request| {
                Box::pin(async move {
                    let store = request.session_mut()?;
                    store.migrate(destroy).await.map_err(Into::into)
                })
            })
            .await
    }

    pub async fn regenerate(destroy: bool) -> Result<(), HttpError> {
        RequestFacade::get_facade_root()?
            .with_async(move |request| {
                Box::pin(async move {
                    let store = request.session_mut()?;
                    store.regenerate(destroy).await.map_err(Into::into)
                })
            })
            .await
    }

    pub async fn invalidate() -> Result<(), HttpError> {
        RequestFacade::get_facade_root()?
            .with_async(move |request| {
                Box::pin(async move {
                    let store = request.session_mut()?;
                    store.invalidate().await.map_err(Into::into)
                })
            })
            .await
    }

    pub fn should_block() -> Result<bool, HttpError> {
        Ok(Self::get_facade_root()?.should_block())
    }

    pub fn block_driver() -> Result<Option<String>, HttpError> {
        Ok(Self::get_facade_root()?.block_driver().map(str::to_owned))
    }

    pub fn default_route_block_lock_seconds() -> Result<u64, HttpError> {
        Ok(Self::get_facade_root()?.default_route_block_lock_seconds())
    }

    pub fn default_route_block_wait_seconds() -> Result<u64, HttpError> {
        Ok(Self::get_facade_root()?.default_route_block_wait_seconds())
    }

    pub fn get_session_config() -> Result<Value, HttpError> {
        Ok(Self::get_facade_root()?.get_session_config().clone())
    }
}
