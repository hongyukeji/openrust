use std::sync::Arc;

use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};

use super::MiddlewareEntry;

/// 控制器提供的静态路由中间件声明。
pub trait HasMiddleware {
    fn middleware() -> Vec<ControllerMiddleware>;
}

/// 一组共享动作过滤条件的控制器中间件。
#[derive(Clone)]
pub struct ControllerMiddleware {
    entries: Vec<MiddlewareEntry>,
    only: Option<Vec<String>>,
    except: Vec<String>,
}

impl ControllerMiddleware {
    pub fn new(entries: impl IntoIterator<Item = MiddlewareEntry>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
            only: None,
            except: Vec::new(),
        }
    }

    pub fn named(name: impl Into<String>) -> Self {
        Self::new([MiddlewareEntry::named(name)])
    }

    pub fn from_fn<F>(callback: F) -> Self
    where
        F: for<'a> Fn(&'a mut Request, Next<'a>) -> HttpFuture<'a, Result<Response, HttpError>>
            + Send
            + Sync
            + 'static,
    {
        Self::new([MiddlewareEntry::Instance(Arc::new(InlineMiddleware(
            callback,
        )))])
    }

    /// 仅为列出的动作启用，空切片表示不应用于任何动作。
    pub fn only(mut self, actions: &[&str]) -> Self {
        self.only = Some(actions.iter().map(|action| (*action).to_owned()).collect());
        self
    }

    pub fn except(mut self, actions: &[&str]) -> Self {
        self.except = actions.iter().map(|action| (*action).to_owned()).collect();
        self
    }

    fn applies(&self, action: &str) -> bool {
        self.only
            .as_ref()
            .is_none_or(|items| items.iter().any(|item| item == action))
            && !self.except.iter().any(|item| item == action)
    }
}

pub(super) fn for_action(
    definitions: Vec<ControllerMiddleware>,
    action: &str,
) -> Vec<MiddlewareEntry> {
    definitions
        .into_iter()
        .filter(|definition| definition.applies(action))
        .flat_map(|definition| definition.entries)
        .collect()
}

struct InlineMiddleware<F>(F);

impl<F> Middleware for InlineMiddleware<F>
where
    F: for<'a> Fn(&'a mut Request, Next<'a>) -> HttpFuture<'a, Result<Response, HttpError>>
        + Send
        + Sync
        + 'static,
{
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        (self.0)(request, next)
    }
}
