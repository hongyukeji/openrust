use super::{
    ParallelOptionsResolver, ParallelTestError, ParallelTestFuture, ParallelTesting as Service,
    ParallelTokenResolver,
};
use crate::{
    container::ServiceKey,
    foundation::Application,
    support::facades::{App, Facade},
    testing::TestApp,
};
use serde_json::Value;
use std::future::Future;
/// 当前应用的并行测试生命周期入口。
pub struct ParallelTesting;
impl Facade for ParallelTesting {
    type Service = Service;
    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Service>()
    }
}
macro_rules! process_forward {
    ($register:ident,$call:ident) => {
        pub fn $register(
            callback: impl for<'a> Fn(&'a Application, Value) -> ParallelTestFuture<'a>
            + Send
            + Sync
            + 'static,
        ) -> Result<(), ParallelTestError> {
            Self::get_facade_root()?.$register(callback);
            Ok(())
        }
        pub async fn $call() -> Result<(), ParallelTestError> {
            let root = Self::get_facade_root()?;
            let app = App::get_facade_root()?;
            root.$call(&app).await
        }
    };
}
macro_rules! case_forward {
    ($register:ident,$call:ident) => {
        pub fn $register(
            callback: impl for<'a> Fn(&'a mut TestApp, Value) -> ParallelTestFuture<'a>
            + Send
            + Sync
            + 'static,
        ) -> Result<(), ParallelTestError> {
            Self::get_facade_root()?.$register(callback);
            Ok(())
        }
        pub async fn $call(test: &mut TestApp) -> Result<(), ParallelTestError> {
            Self::get_facade_root()?.$call(test).await
        }
    };
}
macro_rules! database_forward {
    ($register:ident,$call:ident) => {
        pub fn $register(
            callback: impl for<'a> Fn(&'a Application, String, Value) -> ParallelTestFuture<'a>
            + Send
            + Sync
            + 'static,
        ) -> Result<(), ParallelTestError> {
            Self::get_facade_root()?.$register(callback);
            Ok(())
        }
        pub async fn $call(database: &str) -> Result<(), ParallelTestError> {
            let root = Self::get_facade_root()?;
            let app = App::get_facade_root()?;
            root.$call(&app, database).await
        }
    };
}
impl ParallelTesting {
    pub fn resolve_options_using(
        resolver: Option<ParallelOptionsResolver>,
    ) -> Result<(), ParallelTestError> {
        Self::get_facade_root()?.resolve_options_using(resolver);
        Ok(())
    }
    pub fn resolve_token_using(
        resolver: Option<ParallelTokenResolver>,
    ) -> Result<(), ParallelTestError> {
        Self::get_facade_root()?.resolve_token_using(resolver);
        Ok(())
    }
    pub fn option(name: &str) -> Result<Value, ParallelTestError> {
        Self::get_facade_root()?.option(name)
    }
    pub fn token() -> Result<Value, ParallelTestError> {
        Self::get_facade_root()?.token()
    }
    process_forward!(set_up_process, call_set_up_process_callbacks);
    process_forward!(tear_down_process, call_tear_down_process_callbacks);
    case_forward!(set_up_test_case, call_set_up_test_case_callbacks);
    case_forward!(tear_down_test_case, call_tear_down_test_case_callbacks);
    database_forward!(
        set_up_test_database_before_migrating,
        call_set_up_test_database_before_migrating_callbacks
    );
    database_forward!(set_up_test_database, call_set_up_test_database_callbacks);
    pub async fn run_process<T>(
        test: impl Future<Output = Result<T, ParallelTestError>>,
    ) -> Result<T, ParallelTestError> {
        let root = Self::get_facade_root()?;
        let app = App::get_facade_root()?;
        root.run_process(&app, test).await
    }
}
