// 第三方许可见 LICENSE。
use super::ComponentContext;
use crate::{
    console::ConsoleError,
    view::{
        ViewData, ViewValue,
        compilers::{BladeCompiler, CompiledTemplate},
    },
};
use serde_json::Value;
use std::{path::PathBuf, sync::LazyLock};

/// 同步编译钩子；通过原输出上下文支持组合扩展，不执行运行期源文件。
pub trait ComponentViewCompiler: Send {
    fn compile(
        &mut self,
        view: &str,
        data: Value,
        output: &mut ComponentContext<'_>,
    ) -> Result<String, ConsoleError>;
}
#[derive(Default)]
#[non_exhaustive]
pub struct BuiltinComponentViewCompiler;
impl BuiltinComponentViewCompiler {
    pub const fn new() -> Self {
        Self
    }
}
fn program(source: &str) -> CompiledTemplate {
    BladeCompiler::new(PathBuf::new())
        .compile_string(source)
        .expect("内置控制台视图具有有效语法")
}
impl ComponentViewCompiler for BuiltinComponentViewCompiler {
    fn compile(
        &mut self,
        view: &str,
        data: Value,
        _output: &mut ComponentContext<'_>,
    ) -> Result<String, ConsoleError> {
        static ALERT: LazyLock<CompiledTemplate> = LazyLock::new(|| {
            program(include_str!(
                "../../resources/views/components/alert.blade.html"
            ))
        });
        static LINE: LazyLock<CompiledTemplate> = LazyLock::new(|| {
            program(include_str!(
                "../../resources/views/components/line.blade.html"
            ))
        });
        static BULLETS: LazyLock<CompiledTemplate> = LazyLock::new(|| {
            program(include_str!(
                "../../resources/views/components/bullet-list.blade.html"
            ))
        });
        static DETAIL: LazyLock<CompiledTemplate> = LazyLock::new(|| {
            program(include_str!(
                "../../resources/views/components/two-column-detail.blade.html"
            ))
        });
        let (template, keys): (&CompiledTemplate, &[&str]) = match view {
            "alert" => (&ALERT, &["content"]),
            "line" => (
                &LINE,
                &["marginTop", "bgColor", "fgColor", "title", "content"],
            ),
            "bullet-list" => (&BULLETS, &["elements"]),
            "two-column-detail" => (&DETAIL, &["first", "second"]),
            _ => return Err(ConsoleError::new(format!("未找到控制台组件视图：{view}"))),
        };
        let Value::Object(data) = data else {
            return Err(ConsoleError::new("组件视图数据必须为具名条目"));
        };
        for key in keys {
            let value = data
                .get(*key)
                .ok_or_else(|| ConsoleError::new(format!("组件视图缺少变量：{key}")))?;
            if *key == "elements" {
                let values: Box<dyn Iterator<Item = &Value>> = match value {
                    Value::Array(values) => Box::new(values.iter()),
                    Value::Object(values) => Box::new(values.values()),
                    _ => return Err(ConsoleError::new("组件列表视图需要可迭代条目")),
                };
                if values
                    .into_iter()
                    .any(|value| value.is_array() || value.is_object())
                {
                    return Err(ConsoleError::new("组件列表视图的每项必须为标量"));
                }
            } else if value.is_array() || value.is_object() {
                return Err(ConsoleError::new(format!("组件视图变量必须为标量：{key}")));
            }
        }
        let data: ViewData = data
            .into_iter()
            .map(|(key, value)| (key, ViewValue::from(value)))
            .collect();
        template.render(&data).map_err(ConsoleError::from_source)
    }
}
