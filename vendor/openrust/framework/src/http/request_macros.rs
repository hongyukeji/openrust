use crate::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::{HttpError, HttpFuture},
    support::{ProviderResult, ServiceProvider},
};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, RwLock},
};

type Callback = dyn Fn(Vec<Value>) -> HttpFuture<'static, Result<Value, HttpError>> + Send + Sync;
/// 请求宏显式调用；回调可使用 Request 门面，注册表按应用隔离。
#[derive(Clone)]
pub struct RequestMacro(Arc<Callback>);
impl RequestMacro {
    pub fn new<F, Fut>(callback: F) -> Self
    where
        F: Fn(Vec<Value>) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Value, HttpError>> + Send + 'static,
    {
        Self(Arc::new(move |arguments| Box::pin(callback(arguments))))
    }
}
#[derive(Default)]
pub(crate) struct RequestMacros(RwLock<HashMap<String, RequestMacro>>);
impl RequestMacros {
    pub(crate) fn mixin<K: Into<String>>(
        &self,
        macros: impl IntoIterator<Item = (K, RequestMacro)>,
        replace: bool,
    ) {
        for (name, callback) in macros {
            let mut entries = self
                .0
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let key = name.into();
            let previous = if replace || !entries.contains_key(&key) {
                entries.insert(key, callback)
            } else {
                Some(callback)
            };
            drop(entries);
            drop(previous);
        }
    }
    pub(crate) fn has(&self, name: &str) -> bool {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(name)
    }
    pub(crate) fn clear(&self) {
        let retired = std::mem::take(
            &mut *self
                .0
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        drop(retired);
    }
    pub(crate) async fn call(&self, name: &str, arguments: Vec<Value>) -> Result<Value, HttpError> {
        let callback = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(name)
            .cloned()
            .ok_or_else(|| HttpError::Internal {
                source: std::io::Error::other(format!("请求宏不存在：{name}")).into(),
            })?;
        (callback.0)(arguments).await
    }
}
pub(crate) struct RequestServiceProvider;
impl ServiceProvider for RequestServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().bind_named("request", |_| {
                crate::support::facades::RequestHandle::for_current_request()
            })?;
            app.container().alias(
                "request",
                crate::container::ServiceKey::typed::<crate::support::facades::RequestHandle>(),
            )?;
            app.container()
                .singleton::<RequestMacros, _>(|_| Ok(Arc::new(RequestMacros::default())))?;
            Ok(())
        })
    }
}
