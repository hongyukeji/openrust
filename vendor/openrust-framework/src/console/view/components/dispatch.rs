use super::{ComponentOutput, Factory};
use crate::console::{
    ChoiceKey, CompletionCallback, ConsoleError, OutputFormat, OutputOptions, OutputReference,
    OutputVerbosity,
};
use serde_json::Value;

pub type ComponentTaskCallback<'a> =
    Box<dyn FnOnce(&mut OutputReference<'_>) -> Result<Value, ConsoleError> + Send + 'a>;

/// 动态组件的参数；回调通过编译后的闭包传递，不解析可执行文本。
#[non_exhaustive]
pub enum ComponentArgument<'a> {
    Value(Value),
    Completion(CompletionCallback),
    Task(ComponentTaskCallback<'a>),
    Options(OutputOptions),
}
impl<'a> ComponentArgument<'a> {
    pub fn task<V: Into<Value>>(
        callback: impl FnOnce(&mut OutputReference<'_>) -> Result<V, ConsoleError> + Send + 'a,
    ) -> Self {
        Self::Task(Box::new(move |output| callback(output).map(Into::into)))
    }
}
impl From<Value> for ComponentArgument<'_> {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}
impl From<&str> for ComponentArgument<'_> {
    fn from(value: &str) -> Self {
        Self::Value(value.into())
    }
}
impl From<String> for ComponentArgument<'_> {
    fn from(value: String) -> Self {
        Self::Value(value.into())
    }
}
impl From<CompletionCallback> for ComponentArgument<'_> {
    fn from(value: CompletionCallback) -> Self {
        Self::Completion(value)
    }
}
impl From<OutputOptions> for ComponentArgument<'_> {
    fn from(value: OutputOptions) -> Self {
        Self::Options(value)
    }
}
struct Arguments<'a>(std::vec::IntoIter<ComponentArgument<'a>>);
impl Arguments<'_> {
    fn value(&mut self, name: &str) -> Result<Value, ConsoleError> {
        match self.0.next() {
            Some(ComponentArgument::Value(value)) => Ok(value),
            Some(_) => Err(invalid(name, "需要数据值")),
            None => Err(invalid(name, "缺少必需参数")),
        }
    }
    fn optional(&mut self, name: &str, default: Value) -> Result<Value, ConsoleError> {
        match self.0.next() {
            Some(ComponentArgument::Value(value)) => Ok(value),
            None => Ok(default),
            _ => Err(invalid(name, "需要数据值")),
        }
    }
    fn text(&mut self, name: &str) -> Result<String, ConsoleError> {
        scalar(self.value(name)?, name)
    }
    fn boolean(&mut self, name: &str, default: bool) -> Result<bool, ConsoleError> {
        let value = self.optional(name, Value::Bool(default))?;
        if value.is_array() || value.is_object() {
            return Err(invalid(name, "需要标量布尔值"));
        }
        Ok(crate::support::truthy(&value))
    }
    fn options(&mut self) -> Result<OutputOptions, ConsoleError> {
        match self.0.next() {
            None => Ok(OutputVerbosity::Normal.into()),
            Some(ComponentArgument::Options(options)) => Ok(options),
            Some(ComponentArgument::Value(value)) => parse_options(value),
            _ => Err(invalid("verbosity", "需要输出选项")),
        }
    }
}
fn invalid(name: &str, reason: &str) -> ConsoleError {
    ConsoleError::new(format!("组件参数 {name} 无效：{reason}"))
}
fn scalar(value: Value, name: &str) -> Result<String, ConsoleError> {
    if value.is_array() || value.is_object() {
        return Err(invalid(name, "需要标量文本"));
    }
    Ok(crate::support::input_scalars::string(value))
}
fn parse_options(value: Value) -> Result<OutputOptions, ConsoleError> {
    let mask = match value {
        Value::Null => 0,
        Value::Bool(v) => u16::from(v),
        Value::Number(v) => v
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .ok_or_else(|| invalid("verbosity", "需要非负整数输出选项"))?,
        Value::String(v) => v
            .parse::<u16>()
            .map_err(|_| invalid("verbosity", "需要整数输出选项"))?,
        _ => return Err(invalid("verbosity", "需要整数输出选项")),
    };
    let format = match mask & 7 {
        0 => OutputFormat::Normal,
        1 => OutputFormat::ExplicitNormal,
        2 => OutputFormat::Raw,
        4 => OutputFormat::Plain,
        _ => return Err(invalid("verbosity", "格式标志不可组合")),
    };
    let verbosity = match mask & !7 {
        0 => None,
        8 => Some(OutputVerbosity::Silent),
        16 => Some(OutputVerbosity::Quiet),
        32 => Some(OutputVerbosity::Normal),
        64 => Some(OutputVerbosity::Verbose),
        128 => Some(OutputVerbosity::VeryVerbose),
        256 => Some(OutputVerbosity::Debug),
        _ => return Err(invalid("verbosity", "需要单个详细级别")),
    };
    Ok(OutputOptions { format, verbosity })
}
impl<T: ComponentOutput + ?Sized> Factory<'_, T> {
    /// 组件名称是动态文本契约；保留原大小写不敏感的名称，不把下划线静默变成别名。
    pub async fn call<'a>(
        &mut self,
        name: &str,
        parameters: Vec<ComponentArgument<'a>>,
    ) -> Result<Value, ConsoleError> {
        if let Some(dispatch) = self
            .dispatch
            .clone()
            .filter(|dispatch| dispatch.handles_call(name))
        {
            return dispatch
                .call(name, parameters, self.component_context())
                .await;
        }
        if !super::registry::builtin(name) {
            return super::registry::render_custom(self, name, parameters).await;
        }
        let mut args = Arguments(parameters.into_iter());
        match name.to_ascii_lowercase().as_str() {
            "info" | "success" | "warn" | "error" | "alert" => {
                let text = args.text("text")?;
                let options = args.options()?;
                match name.to_ascii_lowercase().as_str() {
                    "info" => self.info_with(&text, options),
                    "success" => self.success_with(&text, options),
                    "warn" => self.warn_with(&text, options),
                    "error" => self.error_with(&text, options),
                    _ => self.alert_with(&text, options),
                }?;
            }
            "line" => {
                let style = args.text("style")?;
                let text = args.text("text")?;
                self.line_with(&style, &text, args.options()?)?;
            }
            "bulletlist" => {
                let values = args.value("elements")?;
                let values = match values {
                    Value::Array(v) => v,
                    Value::Object(v) => v.into_values().collect(),
                    _ => return Err(invalid("elements", "需要列表或关联条目")),
                };
                let items = values
                    .into_iter()
                    .map(|v| scalar(v, "elements"))
                    .collect::<Result<Vec<_>, _>>()?;
                self.bullet_list_with(items, args.options()?)?;
            }
            "twocolumndetail" => {
                let first = args.text("first")?;
                let second = args.optional("second", Value::Null)?;
                let second = if second.is_null() {
                    None
                } else {
                    Some(scalar(second, "second")?)
                };
                self.two_column_detail_with(&first, second.as_deref(), args.options()?)?;
            }
            "task" => {
                let description = args.text("description")?;
                let callback = args.0.next();
                let options = args.options()?;
                match callback {
                    None => self.task_with(&description, options)?,
                    Some(ComponentArgument::Value(ref v)) if !crate::support::truthy(v) => {
                        self.task_with(&description, options)?
                    }
                    Some(ComponentArgument::Task(callback)) => {
                        self.task_callback_with(&description, callback, options)?
                    }
                    _ => self.task_callback_with::<Value>(
                        &description,
                        |_| Err(invalid("task", "需要已注册的可执行回调")),
                        options,
                    )?,
                }
            }
            "ask" => {
                let label = args.text("question")?;
                let default = args.optional("default", Value::Null)?;
                let multiline = args.boolean("multiline", false)?;
                return self
                    .ask_with(&label, (!default.is_null()).then_some(default), multiline)
                    .await;
            }
            "askwithcompletion" => {
                let label = args.text("question")?;
                let choices = args
                    .0
                    .next()
                    .ok_or_else(|| invalid("choices", "缺少必需参数"))?;
                let default = args.optional("default", Value::Null)?;
                let default = (!default.is_null()).then_some(default);
                return match choices {
                    ComponentArgument::Completion(callback) => {
                        self.ask_with_completion_callback(&label, callback, default)
                            .await
                    }
                    ComponentArgument::Value(Value::Array(values)) => {
                        let values = values
                            .into_iter()
                            .map(|v| scalar(v, "choices"))
                            .collect::<Result<Vec<_>, _>>()?;
                        self.ask_with_completion_with(&label, values, default).await
                    }
                    _ => Err(invalid("choices", "需要候选列表或编译后的补全回调")),
                };
            }
            "confirm" => {
                let label = args.text("question")?;
                let default = args.boolean("default", false)?;
                return self.confirm_with(&label, default).await.map(Value::Bool);
            }
            "secret" => {
                let label = args.text("question")?;
                let fallback = args.boolean("fallback", true)?;
                return self.secret_with(&label, fallback).await;
            }
            "choice" => {
                let label = args.text("question")?;
                let choices = args.value("choices")?;
                let default = args.optional("default", Value::Null)?;
                let default = (!default.is_null()).then_some(default);
                let attempts = args.optional("attempts", Value::Null)?;
                let attempts = if attempts.is_null() {
                    None
                } else {
                    Some(
                        attempts
                            .as_u64()
                            .and_then(|v| usize::try_from(v).ok())
                            .ok_or_else(|| invalid("attempts", "需要非负整数"))?,
                    )
                };
                let multiple = args.boolean("multiple", false)?;
                return match choices {
                    Value::Array(values) => {
                        self.choice_with(&label, values, default, attempts, multiple)
                            .await
                    }
                    Value::Object(values) => {
                        self.choice_pairs_with(
                            &label,
                            values.into_iter().map(|(k, v)| (ChoiceKey::from(k), v)),
                            default,
                            attempts,
                            multiple,
                        )
                        .await
                    }
                    _ => Err(invalid("choices", "需要列表或关联条目")),
                };
            }
            _ => return Err(ConsoleError::new(format!("未找到控制台组件：{name}"))),
        }
        Ok(Value::Null)
    }
}
