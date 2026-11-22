use std::sync::{Arc, RwLock};

use crate::{
    container::ServiceKey,
    http::request_context,
    support::facades::{Facade, View as ViewFacade},
    view::{Factory, ViewError, ViewResult},
};

use super::{Cursor, CursorPaginator, LengthAwarePaginator, Paginator};

type PathResolver = Arc<dyn Fn() -> String + Send + Sync>;
type PageResolver = Arc<dyn Fn(&str) -> u64 + Send + Sync>;
type QueryResolver = Arc<dyn Fn() -> Option<Vec<(String, String)>> + Send + Sync>;
type CursorResolver = Arc<dyn Fn(&str) -> Option<Cursor> + Send + Sync>;
type ViewResolver = Arc<dyn Fn() -> ViewResult<Arc<Factory>> + Send + Sync>;

/// 每个应用各自保存解析器，调用回调前释放锁，避免重入死锁。
#[derive(Default)]
pub(super) struct PaginationState {
    path: RwLock<Option<PathResolver>>,
    page: RwLock<Option<PageResolver>>,
    query: RwLock<Option<QueryResolver>>,
    cursor: RwLock<Option<CursorResolver>>,
    view: RwLock<Option<ViewResolver>>,
}

struct StateFacade;

impl Facade for StateFacade {
    type Service = PaginationState;

    #[allow(non_snake_case)]
    fn getFacadeAccessor() -> ServiceKey {
        ServiceKey::typed::<PaginationState>()
    }
}

fn state() -> ViewResult<Arc<PaginationState>> {
    StateFacade::getFacadeRoot().map_err(ViewError::Unavailable)
}

fn query() -> Option<Vec<(String, String)>> {
    request_context::with_uri(|uri| {
        url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
            .into_owned()
            .collect()
    })
}

fn input(name: &str) -> Option<String> {
    query()?
        .into_iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

// 静态入口只保留在无项目类型参数的实例上，避免调用时要求推断分页项类型。
macro_rules! offset_resolvers {
    ($type:ident) => {
        #[allow(non_snake_case)]
        impl $type<()> {
            pub fn currentPathResolver(
                resolver: impl Fn() -> String + Send + Sync + 'static,
            ) -> ViewResult<()> {
                *state()?.path.write().expect("分页路径解析器锁") = Some(Arc::new(resolver));
                Ok(())
            }

            pub fn resolveCurrentPath(default: &str) -> String {
                let callback = state()
                    .ok()
                    .and_then(|state| state.path.read().expect("分页路径解析器锁").clone());
                if let Some(callback) = callback {
                    return callback();
                }
                request_context::with_uri(|uri| format!("/{}", uri.path().trim_matches('/')))
                    .unwrap_or_else(|| default.to_owned())
            }

            pub fn currentPageResolver(
                resolver: impl Fn(&str) -> u64 + Send + Sync + 'static,
            ) -> ViewResult<()> {
                *state()?.page.write().expect("分页页码解析器锁") = Some(Arc::new(resolver));
                Ok(())
            }

            pub fn resolveCurrentPage(pageName: &str, default: u64) -> u64 {
                let callback = state()
                    .ok()
                    .and_then(|state| state.page.read().expect("分页页码解析器锁").clone());
                if let Some(callback) = callback {
                    return callback(pageName);
                }
                if query().is_some() {
                    return input(pageName)
                        .and_then(|value| {
                            let value = serde_json::Value::String(value);
                            crate::support::input_scalars::valid_integer(&value)
                                .then(|| crate::support::input_scalars::integer(&value))
                                .and_then(|value| u64::try_from(value).ok())
                        })
                        .filter(|value| *value >= 1)
                        .unwrap_or(1);
                }
                default
            }

            pub fn queryStringResolver<R>(
                resolver: impl Fn() -> R + Send + Sync + 'static,
            ) -> ViewResult<()>
            where
                R: Into<Option<Vec<(String, String)>>>,
            {
                *state()?.query.write().expect("分页查询解析器锁") =
                    Some(Arc::new(move || resolver().into()));
                Ok(())
            }

            pub fn resolveQueryString(
                default: Option<Vec<(String, String)>>,
            ) -> Option<Vec<(String, String)>> {
                let callback = state()
                    .ok()
                    .and_then(|state| state.query.read().expect("分页查询解析器锁").clone());
                match callback {
                    Some(callback) => callback(),
                    None => query().or(default),
                }
            }

            pub fn viewFactoryResolver(
                resolver: impl Fn() -> ViewResult<Arc<Factory>> + Send + Sync + 'static,
            ) -> ViewResult<()> {
                *state()?.view.write().expect("分页视图解析器锁") = Some(Arc::new(resolver));
                Ok(())
            }

            pub fn viewFactory() -> ViewResult<Arc<Factory>> {
                let callback = state()
                    .ok()
                    .and_then(|state| state.view.read().expect("分页视图解析器锁").clone());
                match callback {
                    Some(callback) => callback(),
                    None => ViewFacade::getFacadeRoot().map_err(ViewError::Unavailable),
                }
            }
        }
    };
}

offset_resolvers!(Paginator);
offset_resolvers!(LengthAwarePaginator);

#[allow(non_snake_case)]
impl CursorPaginator<()> {
    pub fn currentCursorResolver(
        resolver: impl Fn(&str) -> Option<Cursor> + Send + Sync + 'static,
    ) -> ViewResult<()> {
        *state()?.cursor.write().expect("分页游标解析器锁") = Some(Arc::new(resolver));
        Ok(())
    }

    pub fn resolveCurrentCursor(cursorName: &str, default: Option<Cursor>) -> Option<Cursor> {
        let callback = state()
            .ok()
            .and_then(|state| state.cursor.read().expect("分页游标解析器锁").clone());
        if let Some(callback) = callback {
            return callback(cursorName);
        }
        if query().is_some() {
            return input(cursorName).and_then(|value| Cursor::fromEncoded(&value).ok());
        }
        default
    }

    pub fn viewFactory() -> ViewResult<Arc<Factory>> {
        Paginator::viewFactory()
    }
}
