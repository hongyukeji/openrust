//! 请求内 Cookie 队列，按名称和路径保留插入顺序。

use std::sync::{Arc, Mutex};

use http::StatusCode;

use super::value::{Cookie, CookieFactory, CookieOptions};
use crate::{Error, Result};

const MAX_QUEUED_COOKIES: usize = 128;
const MAX_QUEUED_HEADER_BYTES: usize = 64 * 1024;

#[derive(Clone, Default)]
struct QueueState {
    groups: Vec<QueueGroup>,
}

#[derive(Clone)]
struct QueueGroup {
    name: String,
    paths: Vec<QueuedCookie>,
}

#[derive(Clone)]
struct QueuedCookie {
    path: String,
    cookie: Cookie,
}

impl QueueState {
    fn insert(&mut self, cookie: Cookie) {
        let name = cookie.name().to_owned();
        let path = cookie.path().to_owned();
        if let Some(group) = self.groups.iter_mut().find(|group| group.name == name) {
            if let Some(entry) = group.paths.iter_mut().find(|entry| entry.path == path) {
                entry.cookie = cookie;
            } else {
                group.paths.push(QueuedCookie { path, cookie });
            }
        } else {
            self.groups.push(QueueGroup {
                name,
                paths: vec![QueuedCookie { path, cookie }],
            });
        }
    }

    fn find(&self, name: &str, path: Option<&str>) -> Option<&Cookie> {
        let group = self.groups.iter().find(|group| group.name == name)?;
        match path {
            Some(path) => group
                .paths
                .iter()
                .find(|entry| entry.path == path)
                .map(|entry| &entry.cookie),
            None => group.paths.last().map(|entry| &entry.cookie),
        }
    }

    fn remove(&mut self, name: &str, path: Option<&str>) {
        if let Some(group_index) = self.groups.iter().position(|group| group.name == name) {
            match path {
                None => {
                    self.groups.remove(group_index);
                }
                Some(path) => {
                    let group = &mut self.groups[group_index];
                    group.paths.retain(|entry| entry.path != path);
                    if group.paths.is_empty() {
                        self.groups.remove(group_index);
                    }
                }
            }
        }
    }

    fn cookies(&self) -> Vec<Cookie> {
        self.groups
            .iter()
            .flat_map(|group| group.paths.iter().map(|entry| entry.cookie.clone()))
            .collect()
    }

    fn validate(&self) -> Result<()> {
        let mut count = 0usize;
        let mut bytes = 0usize;
        for group in &self.groups {
            for entry in &group.paths {
                count = count.checked_add(1).ok_or_else(limit_error)?;
                if count > MAX_QUEUED_COOKIES {
                    return Err(limit_error());
                }
                bytes = bytes
                    .checked_add(entry.cookie.to_header(false)?.as_bytes().len())
                    .ok_or_else(limit_error)?;
                if bytes > MAX_QUEUED_HEADER_BYTES {
                    return Err(limit_error());
                }
            }
        }
        Ok(())
    }
}

/// 当前请求的 Cookie 队列；克隆后共享同一个请求的队列状态。
#[derive(Clone)]
pub struct CookieJar {
    factory: Arc<CookieFactory>,
    state: Arc<Mutex<QueueState>>,
}

impl std::fmt::Debug for CookieJar {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CookieJar").finish_non_exhaustive()
    }
}

impl CookieJar {
    /// 创建独立队列，并保留不可变 Cookie 创建默认值。
    pub fn new(factory: CookieFactory) -> Self {
        Self {
            factory: Arc::new(factory),
            state: Arc::new(Mutex::new(QueueState::default())),
        }
    }

    /// 返回此队列使用的 Cookie 创建器。
    pub fn factory(&self) -> &CookieFactory {
        &self.factory
    }

    /// 将 Cookie 加入队列；同名称、同路径覆盖但不改变原插入位置。
    pub fn queue(&self, cookie: Cookie) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable_error())?;
        let mut candidate = state.clone();
        candidate.insert(cookie);
        candidate.validate()?;
        *state = candidate;
        Ok(())
    }

    /// 读取名称与可选路径对应的 Cookie；不指定路径时取最后插入的路径。
    pub fn queued(&self, name: &str, path: Option<&str>) -> Result<Option<Cookie>> {
        let state = self.state.lock().map_err(|_| unavailable_error())?;
        Ok(state.find(name, path).cloned())
    }

    /// 判断名称与可选路径对应的 Cookie 是否在队列内。
    pub fn has_queued(&self, name: &str, path: Option<&str>) -> Result<bool> {
        Ok(self.queued(name, path)?.is_some())
    }

    /// 删除名称下所有路径，或仅删除指定路径。
    pub fn unqueue(&self, name: &str, path: Option<&str>) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable_error())?;
        state.remove(name, path);
        Ok(())
    }

    /// 将过期 Cookie 加入队列，以便下一个响应删除浏览器中的值。
    pub fn expire(&self, name: &str, options: CookieOptions) -> Result<()> {
        self.queue(self.factory.forget(name, options)?)
    }

    /// 依名称、路径的首次插入顺序读取全部待发送 Cookie。
    pub fn get_queued_cookies(&self) -> Result<Vec<Cookie>> {
        let state = self.state.lock().map_err(|_| unavailable_error())?;
        Ok(state.cookies())
    }

    /// 清空当前请求队列。
    pub fn flush_queued_cookies(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable_error())?;
        *state = QueueState::default();
        Ok(())
    }
}

fn limit_error() -> Error {
    Error::custom_with_status(
        StatusCode::PAYLOAD_TOO_LARGE,
        "input_cookie_limits_exceeded",
        "Cookie 队列超过资源限制",
    )
}

fn unavailable_error() -> Error {
    Error::custom_with_status(
        StatusCode::INTERNAL_SERVER_ERROR,
        "cookie_queue_unavailable",
        "Cookie 队列不可用",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(
        factory: &CookieFactory,
        name: &str,
        value: &str,
        path: &str,
        domain: Option<&str>,
    ) -> Result<Cookie> {
        factory.make(
            name,
            value,
            0,
            CookieOptions {
                path: Some(path.into()),
                domain: domain.map(str::to_owned),
                ..CookieOptions::default()
            },
        )
    }

    #[test]
    fn path_and_name_order_survive_replacement_and_reinsertion() -> Result<()> {
        let factory = CookieFactory::default();
        let jar = CookieJar::new(factory);
        jar.queue(make(jar.factory(), "a", "first", "/one", Some("one.test"))?)?;
        jar.queue(make(jar.factory(), "b", "second", "/", None)?)?;
        jar.queue(make(jar.factory(), "a", "third", "/two", None)?)?;
        jar.queue(make(
            jar.factory(),
            "a",
            "replacement",
            "/one",
            Some("two.test"),
        )?)?;

        let cookies = jar.get_queued_cookies()?;
        assert_eq!(cookies.len(), 3);
        assert_eq!(cookies[0].name(), "a");
        assert_eq!(cookies[0].path(), "/one");
        assert_eq!(cookies[0].domain(), Some("two.test"));
        assert_eq!(cookies[1].path(), "/two");
        assert_eq!(cookies[2].name(), "b");
        assert_eq!(
            jar.queued("a", None)?.as_ref().map(Cookie::path),
            Some("/two")
        );
        assert!(jar.has_queued("a", Some("/one"))?);

        jar.unqueue("a", Some("/two"))?;
        assert_eq!(
            jar.queued("a", None)?.as_ref().map(Cookie::path),
            Some("/one")
        );
        jar.unqueue("a", None)?;
        jar.queue(make(jar.factory(), "a", "again", "/new", None)?)?;
        let cookies = jar.get_queued_cookies()?;
        assert_eq!(
            cookies.iter().map(Cookie::name).collect::<Vec<_>>(),
            ["b", "a"]
        );
        Ok(())
    }

    #[test]
    fn expire_flush_and_budget_failure_leave_expected_state() -> Result<()> {
        let jar = CookieJar::new(CookieFactory::default());
        jar.expire("gone", CookieOptions::default())?;
        assert!(jar.has_queued("gone", Some("/"))?);
        jar.flush_queued_cookies()?;
        assert!(jar.get_queued_cookies()?.is_empty());

        for index in 0..MAX_QUEUED_COOKIES {
            jar.queue(make(
                jar.factory(),
                &format!("cookie{index}"),
                "v",
                "/",
                None,
            )?)?;
        }
        let before = jar.get_queued_cookies()?;
        let overflow = jar.queue(make(jar.factory(), "extra", "v", "/", None)?);
        assert!(overflow.is_err_and(|error| {
            error.status() == StatusCode::PAYLOAD_TOO_LARGE
                && error.code() == "input_cookie_limits_exceeded"
        }));
        assert_eq!(jar.get_queued_cookies()?.len(), before.len());
        assert!(!jar.has_queued("extra", None)?);

        jar.flush_queued_cookies()?;
        let large = "x".repeat(800);
        let mut reached_bytes = false;
        for index in 0..MAX_QUEUED_COOKIES {
            let name = format!("large{index}");
            let cookie = make(jar.factory(), &name, &large, "/", None)?;
            let previous = jar.get_queued_cookies()?.len();
            match jar.queue(cookie) {
                Ok(()) => {}
                Err(error) => {
                    assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
                    assert_eq!(error.code(), "input_cookie_limits_exceeded");
                    assert_eq!(jar.get_queued_cookies()?.len(), previous);
                    assert!(!jar.has_queued(&name, None)?);
                    reached_bytes = true;
                    break;
                }
            }
        }
        assert!(reached_bytes);
        Ok(())
    }

    #[test]
    fn clones_share_one_short_locked_queue_without_cross_request_state() -> Result<()> {
        let jar = CookieJar::new(CookieFactory::default());
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for index in 0..16 {
                let clone = jar.clone();
                handles.push(scope.spawn(move || {
                    let name = format!("thread{index}");
                    let cookie = make(clone.factory(), &name, "v", "/", None);
                    cookie.and_then(|cookie| clone.queue(cookie))
                }));
            }
            for handle in handles {
                assert!(handle.join().is_ok_and(|result| result.is_ok()));
            }
        });
        assert_eq!(jar.get_queued_cookies()?.len(), 16);
        let other = CookieJar::new(CookieFactory::default());
        assert!(other.get_queued_cookies()?.is_empty());
        Ok(())
    }
}
