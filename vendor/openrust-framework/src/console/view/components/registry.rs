use super::{ComponentArgument, ComponentOutput, Factory};
use crate::console::{AsOutput, ConsoleError, ConsoleFuture, OutputReference, QuestionStyle};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

/// 自定义组件所借用的原始输出；样式分支同时保留原问答输入。
#[non_exhaustive]
pub enum ComponentContext<'a> {
    Output(OutputReference<'a>),
    Style(&'a mut dyn QuestionStyle),
}
impl AsOutput for ComponentContext<'_> {
    fn as_output(&mut self) -> OutputReference<'_> {
        match self {
            Self::Output(output) => output.as_output(),
            Self::Style(style) => style.as_output(),
        }
    }
}
impl ComponentContext<'_> {
    pub fn question_style(&mut self) -> Result<&mut (dyn QuestionStyle + '_), ConsoleError> {
        match self {
            Self::Style(style) => Ok(*style),
            Self::Output(_) => Err(ConsoleError::from_runtime_source(std::io::Error::other(
                "当前组件输出未提供问答输入",
            ))),
        }
    }
}
/// 每次调用创建的组件；不与其他调用共享可变实例。
pub trait RenderableComponent: Send {
    fn render<'a>(
        &'a mut self,
        parameters: Vec<ComponentArgument<'a>>,
    ) -> ConsoleFuture<'a, Result<Value, ConsoleError>>;
}
/// 将编译后的组件构造器注册到所属应用；输出由工厂按调用借出。
pub trait ComponentConstructor: Send + Sync {
    fn make<'a>(
        &'a self,
        output: ComponentContext<'a>,
    ) -> Result<Box<dyn RenderableComponent + 'a>, ConsoleError>;
}
#[derive(Default)]
pub struct ComponentRegistry(RwLock<BTreeMap<String, Arc<dyn ComponentConstructor>>>);
impl ComponentRegistry {
    pub fn register(
        &self,
        name: &str,
        constructor: Arc<dyn ComponentConstructor>,
    ) -> Result<(), ConsoleError> {
        if !valid_name(name)
            || builtin(name)
            || matches!(name.to_ascii_lowercase().as_str(), "component" | "factory")
            || name.to_ascii_lowercase().starts_with("mutators\\")
        {
            return Err(ConsoleError::new(format!("不可注册控制台组件名称：{name}")));
        }
        let key = name.to_ascii_lowercase();
        let mut entries = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.contains_key(&key) {
            drop(entries);
            return Err(ConsoleError::new(format!("控制台组件已注册：{name}")));
        }
        entries.insert(key, constructor);
        Ok(())
    }
    fn resolve(&self, name: &str) -> Option<Arc<dyn ComponentConstructor>> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&name.to_ascii_lowercase())
            .cloned()
    }
}
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('\\').all(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return false;
            };
            let start = |c: char| c == '_' || c.is_ascii_alphabetic() || !c.is_ascii();
            start(first) && chars.all(|c| start(c) || c.is_ascii_digit())
        })
}
pub(super) fn builtin(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "info"
            | "success"
            | "warn"
            | "error"
            | "alert"
            | "line"
            | "bulletlist"
            | "twocolumndetail"
            | "task"
            | "ask"
            | "askwithcompletion"
            | "choice"
            | "confirm"
            | "secret"
    )
}
pub(super) async fn render_custom<'a, T: ComponentOutput + ?Sized>(
    factory: &mut Factory<'_, T>,
    name: &str,
    parameters: Vec<ComponentArgument<'a>>,
) -> Result<Value, ConsoleError> {
    let constructor = factory
        .registry()?
        .and_then(|registry| registry.resolve(name))
        .ok_or_else(|| ConsoleError::new(format!("未找到控制台组件：{name}")))?;
    let mut component = constructor.make(factory.component_context())?;
    component.render(parameters).await
}
