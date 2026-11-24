use super::TestApp;
use crate::{foundation::Application, support::facades::Facade};
use futures_util::FutureExt as _;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    future::Future,
    panic::{AssertUnwindSafe, resume_unwind},
    pin::Pin,
    sync::{Arc, RwLock},
};

pub type ParallelTestError = Box<dyn std::error::Error + Send + Sync>;
pub type ParallelTestFuture<'a, T = ()> =
    Pin<Box<dyn Future<Output = Result<T, ParallelTestError>> + Send + 'a>>;
pub type ParallelOptionsResolver =
    Arc<dyn Fn(&str) -> Result<Value, ParallelTestError> + Send + Sync>;
pub type ParallelTokenResolver = Arc<dyn Fn() -> Result<Value, ParallelTestError> + Send + Sync>;
type ProcessCallback =
    Arc<dyn for<'a> Fn(&'a Application, Value) -> ParallelTestFuture<'a> + Send + Sync>;
type CaseCallback =
    Arc<dyn for<'a> Fn(&'a mut TestApp, Value) -> ParallelTestFuture<'a> + Send + Sync>;
type DatabaseCallback =
    Arc<dyn for<'a> Fn(&'a Application, String, Value) -> ParallelTestFuture<'a> + Send + Sync>;
#[derive(Default)]
struct Callbacks {
    process_up: Vec<ProcessCallback>,
    process_down: Vec<ProcessCallback>,
    case_up: Vec<CaseCallback>,
    case_down: Vec<CaseCallback>,
    database_before: Vec<DatabaseCallback>,
    database_up: Vec<DatabaseCallback>,
}
/// 原生并行测试生命周期；回调取得真实应用或测试夹具，不以元数据副本替代。
pub struct ParallelTesting {
    server: BTreeMap<String, String>,
    options: RwLock<Option<ParallelOptionsResolver>>,
    token: RwLock<Option<ParallelTokenResolver>>,
    callbacks: RwLock<Callbacks>,
    isolation: isolation::Isolation,
    default_isolation: bool,
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::String(v) => !v.is_empty() && v != "0",
        Value::Number(v) => v.as_f64() != Some(0.0),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    }
}
macro_rules! process_callbacks {
    ($register:ident,$call:ident,$field:ident,$prepare:ident) => {
        pub fn $register(
            &self,
            callback: impl for<'a> Fn(&'a Application, Value) -> ParallelTestFuture<'a>
            + Send
            + Sync
            + 'static,
        ) {
            self.callbacks
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .$field
                .push(Arc::new(callback));
        }
        pub async fn $call(&self, app: &Application) -> Result<(), ParallelTestError> {
            crate::support::facades::App::set_facade_application(Some(app), async {
                if self.in_parallel()? {
                    if self.default_isolation {
                        self.$prepare(app).await?;
                    }
                    let callbacks = self
                        .callbacks
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .$field
                        .clone();
                    for callback in callbacks {
                        callback(app, self.token()?).await?;
                    }
                }
                Ok(())
            })
            .await
        }
    };
}
macro_rules! case_callbacks {
    ($register:ident,$call:ident,$field:ident) => {
        pub fn $register(
            &self,
            callback: impl for<'a> Fn(&'a mut TestApp, Value) -> ParallelTestFuture<'a>
            + Send
            + Sync
            + 'static,
        ) {
            self.callbacks
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .$field
                .push(Arc::new(callback));
        }
        pub async fn $call(&self, test: &mut TestApp) -> Result<(), ParallelTestError> {
            let app = test.app().share();
            crate::support::facades::App::set_facade_application(Some(&app), async {
                if self.in_parallel()? {
                    if self.default_isolation {
                        self.prepare_case(test).await?;
                    }
                    let callbacks = self
                        .callbacks
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .$field
                        .clone();
                    for callback in callbacks {
                        callback(test, self.token()?).await?;
                    }
                }
                Ok(())
            })
            .await
        }
    };
}
macro_rules! database_callbacks {
    ($register:ident,$call:ident,$field:ident) => {
        pub fn $register(
            &self,
            callback: impl for<'a> Fn(&'a Application, String, Value) -> ParallelTestFuture<'a>
            + Send
            + Sync
            + 'static,
        ) {
            self.callbacks
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .$field
                .push(Arc::new(callback));
        }
        pub async fn $call(
            &self,
            app: &Application,
            database: &str,
        ) -> Result<(), ParallelTestError> {
            crate::support::facades::App::set_facade_application(Some(app), async {
                if self.in_parallel()? {
                    let callbacks = self
                        .callbacks
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .$field
                        .clone();
                    for callback in callbacks {
                        callback(app, database.into(), self.token()?).await?;
                    }
                }
                Ok(())
            })
            .await
        }
    };
}
impl Default for ParallelTesting {
    fn default() -> Self {
        Self::new(std::env::vars().collect())
    }
}
impl ParallelTesting {
    pub fn new(server: BTreeMap<String, String>) -> Self {
        Self {
            server,
            options: RwLock::default(),
            token: RwLock::default(),
            callbacks: RwLock::default(),
            isolation: Default::default(),
            default_isolation: false,
        }
    }
    /// 应用装配时启用标准测试隔离；独立构造的生命周期注册表保持无附加副作用。
    pub fn with_default_isolation(mut self) -> Self {
        self.default_isolation = true;
        self
    }
    pub fn resolve_options_using(&self, resolver: Option<ParallelOptionsResolver>) {
        let old = std::mem::replace(
            &mut *self
                .options
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            resolver,
        );
        drop(old);
    }
    pub fn resolve_token_using(&self, resolver: Option<ParallelTokenResolver>) {
        let old = std::mem::replace(
            &mut *self
                .token
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            resolver,
        );
        drop(old);
    }
    pub fn option(&self, option: &str) -> Result<Value, ParallelTestError> {
        let resolver = self
            .options
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        match resolver {
            Some(resolver) => resolver(option),
            None => Ok(self
                .server
                .get(&format!(
                    "LARAVEL_PARALLEL_TESTING_{}",
                    option.to_uppercase()
                ))
                .cloned()
                .map_or(Value::Bool(false), Value::String)),
        }
    }
    pub fn token(&self) -> Result<Value, ParallelTestError> {
        let resolver = self
            .token
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        match resolver {
            Some(resolver) => resolver(),
            None => Ok(self
                .server
                .get("TEST_TOKEN")
                .cloned()
                .map_or(Value::Bool(false), Value::String)),
        }
    }
    fn in_parallel(&self) -> Result<bool, ParallelTestError> {
        Ok(self
            .server
            .get("LARAVEL_PARALLEL_TESTING")
            .is_some_and(|v| !v.is_empty() && v != "0")
            && truthy(&self.token()?))
    }
    process_callbacks!(
        set_up_process,
        call_set_up_process_callbacks,
        process_up,
        prepare_process
    );
    process_callbacks!(
        tear_down_process,
        call_tear_down_process_callbacks,
        process_down,
        cleanup_process
    );
    case_callbacks!(set_up_test_case, call_set_up_test_case_callbacks, case_up);
    case_callbacks!(
        tear_down_test_case,
        call_tear_down_test_case_callbacks,
        case_down
    );
    database_callbacks!(
        set_up_test_database_before_migrating,
        call_set_up_test_database_before_migrating_callbacks,
        database_before
    );
    database_callbacks!(
        set_up_test_database,
        call_set_up_test_database_callbacks,
        database_up
    );
    pub async fn run_process<T>(
        &self,
        app: &Application,
        test: impl Future<Output = Result<T, ParallelTestError>>,
    ) -> Result<T, ParallelTestError> {
        self.call_set_up_process_callbacks(app).await?;
        let result = AssertUnwindSafe(test).catch_unwind().await;
        let cleanup = self.call_tear_down_process_callbacks(app).await;
        match result {
            Ok(result) => finish(result, cleanup),
            Err(panic) => resume_unwind(panic),
        }
    }
}
fn finish<T>(
    result: Result<T, ParallelTestError>,
    cleanup: Result<(), ParallelTestError>,
) -> Result<T, ParallelTestError> {
    match result {
        Err(error) => Err(error),
        Ok(value) => cleanup.map(|()| value),
    }
}
impl TestApp {
    /// 原生测试闭包对应测试用例；错误和panic后仍执行已进入用例的清理回调。
    pub async fn run_parallel_test<T>(
        &mut self,
        test: impl for<'a> FnOnce(&'a mut Self) -> ParallelTestFuture<'a, T>,
    ) -> Result<T, ParallelTestError> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        self.boot().await?;
        let app = self.app().share();
        let parallel = app.container().make::<ParallelTesting>()?;
        crate::support::facades::App::set_facade_application(Some(&app), async {
            let operation = async {
                parallel.call_set_up_test_case_callbacks(self).await?;
                let outcome = AssertUnwindSafe(test(self)).catch_unwind().await;
                let cleanup = parallel.call_tear_down_test_case_callbacks(self).await;
                match outcome {
                    Ok(result) => finish(result, cleanup),
                    Err(panic) => resume_unwind(panic),
                }
            };
            if parallel
                .server
                .get("OPENRUST_PARALLEL_TEST_CASE_PROCESS")
                .is_some_and(|v| v == "1")
            {
                parallel.run_process(&app, operation).await
            } else {
                operation.await
            }
        })
        .await
    }
}

mod facade;
pub use facade::ParallelTesting as ParallelTestingFacade;

impl TestApp {
    pub(super) async fn parallel_database_before(
        &mut self,
        connection: &crate::database::Connection,
    ) -> Result<bool, ParallelTestError> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        let name = connection.get_database_name();
        let parallel = self.app().container().make::<ParallelTesting>()?;
        if self.parallel_databases.contains(name) || truthy(&parallel.option("without_databases")?)
        {
            return Ok(false);
        }
        // 数据库已由夹具创建；回调或迁移失败也不能把同一数据库再次当作新建。
        self.parallel_databases.insert(name.into());
        parallel
            .call_set_up_test_database_before_migrating_callbacks(self.app(), name)
            .await?;
        Ok(true)
    }
    pub(super) async fn parallel_database_after(
        &mut self,
        name: &str,
        created: bool,
    ) -> Result<(), ParallelTestError> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        if created {
            let parallel = self.app().container().make::<ParallelTesting>()?;
            parallel
                .call_set_up_test_database_callbacks(self.app(), name)
                .await?;
        }
        Ok(())
    }
}

pub(crate) mod runner;

mod isolation;
