//! 示例业务依赖；由模块 Provider 登记，通过框架容器注入控制器。

mod message;

pub(crate) use message::{ExampleConfig, ExampleMessage};
