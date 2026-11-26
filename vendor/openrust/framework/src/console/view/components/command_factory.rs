use std::sync::Arc;

use serde_json::Value;

use super::{ComponentArgument, ComponentContext};
use crate::console::{ConsoleError, ConsoleFuture, Input, OutputOptions, OutputReference};

/// 由当前应用容器提供，在每次命令执行时构造一次组件分发器。
pub trait CommandComponentFactory: Send + Sync {
    fn make(
        &self,
        input: &Input,
        output: OutputReference<'_>,
    ) -> Result<Arc<dyn ComponentDispatch>, ConsoleError>;
}

/// 特定命令执行中共享的组件覆盖对象。回调每次借用该工厂原有的输出。
pub trait ComponentDispatch: Send + Sync {
    /// 返回是否接管按名称发起的调用。默认保留框架的组件分发。
    fn handles_call(&self, _name: &str) -> bool {
        false
    }

    fn call<'a>(
        &'a self,
        name: &'a str,
        parameters: Vec<ComponentArgument<'a>>,
        output: ComponentContext<'a>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>>;

    /// 同步文字组件可以独立覆盖。返回 None 时继续使用内置布局。
    fn display_line(
        &self,
        _name: &str,
        _text: &str,
        _options: OutputOptions,
        _output: ComponentContext<'_>,
    ) -> Option<Result<(), ConsoleError>> {
        None
    }
}
