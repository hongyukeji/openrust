//! 业务模块装配入口；后继生命周期统一遵循框架 `ServiceProvider` contract。

mod example;
pub(crate) use example::ExampleServiceProvider;
