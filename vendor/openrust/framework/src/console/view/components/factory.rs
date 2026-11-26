// 第三方许可见 LICENSE。
use super::{
    Alert, Ask, AskWithCompletion, BulletList, Choice, Confirm, Error, Info, Line, Secret, Success,
    Task, TwoColumnDetail, Warn,
};
use super::{ComponentContext, ComponentDispatch, ComponentRegistry};
use crate::console::{
    AsOutput, ChoiceKey, CompletionCallback, ConsoleError, OutputOptions, OutputReference,
    QuestionStyle,
};
use crate::contracts::container::Container as _;
use serde_json::Value;
use std::sync::Arc;

/// 组件工厂所需的输出能力；普通流可显示组件，问答还需保留原样式输入。
pub trait ComponentOutput: AsOutput {
    fn component_context(&mut self) -> ComponentContext<'_> {
        ComponentContext::Output(self.as_output())
    }
    fn question_style(&mut self) -> Option<&mut (dyn QuestionStyle + '_)> {
        match self.component_context() {
            ComponentContext::Style(style) => Some(style),
            ComponentContext::Output(_) => None,
        }
    }
}
impl ComponentOutput for crate::console::Output {}
impl ComponentOutput for OutputReference<'_> {}
impl ComponentOutput for crate::console::ConsoleSectionOutput<'_> {}
impl ComponentOutput for crate::console::OutputStyle<'_> {
    fn component_context(&mut self) -> ComponentContext<'_> {
        ComponentContext::Style(self)
    }
    fn question_style(&mut self) -> Option<&mut (dyn QuestionStyle + '_)> {
        Some(self)
    }
}
impl ComponentOutput for crate::console::ConsoleStyle<'_> {
    fn component_context(&mut self) -> ComponentContext<'_> {
        ComponentContext::Style(self)
    }
    fn question_style(&mut self) -> Option<&mut (dyn QuestionStyle + '_)> {
        Some(self)
    }
}
impl<T: ComponentOutput + ?Sized> ComponentOutput for Box<T> {
    fn component_context(&mut self) -> ComponentContext<'_> {
        (**self).component_context()
    }
    fn question_style(&mut self) -> Option<&mut (dyn QuestionStyle + '_)> {
        (**self).question_style()
    }
}
/// 每次借用同一输出的组件门面；不重新构造样式、复制输入或保存全局渲染器。
pub struct Factory<'a, T: ComponentOutput + ?Sized> {
    output: &'a mut T,
    registry_source: RegistrySource,
    pub(super) dispatch: Option<Arc<dyn ComponentDispatch>>,
}
enum RegistrySource {
    Application(Arc<crate::container::Container>),
    Explicit(Arc<ComponentRegistry>),
    None,
}
impl<'a, T: ComponentOutput + ?Sized> Factory<'a, T> {
    pub fn new(output: &'a mut T) -> Self {
        Self {
            output,
            registry_source: crate::support::facades::current_container()
                .map_or(RegistrySource::None, RegistrySource::Application),
            dispatch: None,
        }
    }
    pub fn with_registry(output: &'a mut T, registry: Arc<ComponentRegistry>) -> Self {
        Self {
            output,
            registry_source: RegistrySource::Explicit(registry),
            dispatch: None,
        }
    }
    pub(crate) fn with_dispatch(
        output: &'a mut T,
        dispatch: Option<Arc<dyn ComponentDispatch>>,
    ) -> Self {
        Self {
            dispatch,
            ..Self::new(output)
        }
    }
    fn display_line(
        &mut self,
        name: &str,
        text: &str,
        options: OutputOptions,
    ) -> Option<Result<(), ConsoleError>> {
        self.dispatch
            .clone()?
            .display_line(name, text, options, self.component_context())
    }
    pub(super) fn registry(&self) -> Result<Option<Arc<ComponentRegistry>>, ConsoleError> {
        match &self.registry_source {
            RegistrySource::None => Ok(None),
            RegistrySource::Explicit(registry) => Ok(Some(registry.clone())),
            RegistrySource::Application(container) => {
                if container.bound::<ComponentRegistry>() {
                    container
                        .make::<ComponentRegistry>()
                        .map(Some)
                        .map_err(ConsoleError::from_source)
                } else {
                    Ok(None)
                }
            }
        }
    }
    pub(super) fn component_context(&mut self) -> ComponentContext<'_> {
        self.output.component_context()
    }
    pub fn get_output(&mut self) -> OutputReference<'_> {
        self.output.as_output()
    }
    fn question_style(&mut self) -> Result<&mut (dyn QuestionStyle + '_), ConsoleError> {
        self.output.question_style().ok_or_else(|| {
            ConsoleError::from_runtime_source(std::io::Error::other("当前输出未提供问答样式及输入"))
        })
    }
    pub fn line(&mut self, style: &str, text: &str) -> Result<(), ConsoleError> {
        Line::new(self.output).render(style, text)
    }
    pub fn line_with(
        &mut self,
        style: &str,
        text: &str,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        Line::new(self.output).render_with(style, text, options)
    }
    pub fn alert(&mut self, text: &str) -> Result<(), ConsoleError> {
        Alert::new(self.output).render(text)
    }
    pub fn alert_with(
        &mut self,
        text: &str,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        Alert::new(self.output).render_with(text, options)
    }
    pub fn bullet_list<S: AsRef<str>>(
        &mut self,
        items: impl IntoIterator<Item = S>,
    ) -> Result<(), ConsoleError> {
        BulletList::new(self.output).render(items)
    }
    pub fn bullet_list_with<S: AsRef<str>>(
        &mut self,
        items: impl IntoIterator<Item = S>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        BulletList::new(self.output).render_with(items, options)
    }
    pub fn two_column_detail(
        &mut self,
        first: &str,
        second: Option<&str>,
    ) -> Result<(), ConsoleError> {
        TwoColumnDetail::new(self.output).render(first, second)
    }
    pub fn two_column_detail_with(
        &mut self,
        first: &str,
        second: Option<&str>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        TwoColumnDetail::new(self.output).render_with(first, second, options)
    }
    pub fn task(&mut self, description: &str) -> Result<(), ConsoleError> {
        Task::new(self.output).render(description)
    }
    pub fn task_with(
        &mut self,
        description: &str,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        Task::new(self.output).render_with(description, options)
    }
    pub fn task_callback<V: Into<Value>>(
        &mut self,
        description: &str,
        callback: impl FnOnce(&mut OutputReference<'_>) -> Result<V, ConsoleError>,
    ) -> Result<(), ConsoleError> {
        Task::new(self.output).render_task(description, callback)
    }
    pub fn task_callback_with<V: Into<Value>>(
        &mut self,
        description: &str,
        callback: impl FnOnce(&mut OutputReference<'_>) -> Result<V, ConsoleError>,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        Task::new(self.output).render_task_with(description, callback, options)
    }
    pub async fn ask(&mut self, question: &str) -> Result<Value, ConsoleError> {
        Ask::new(self.question_style()?).render(question).await
    }
    pub async fn ask_with(
        &mut self,
        question: &str,
        default: Option<Value>,
        multiline: bool,
    ) -> Result<Value, ConsoleError> {
        Ask::new(self.question_style()?)
            .render_with(question, default, multiline)
            .await
    }
    pub async fn ask_with_completion<S: Into<String>>(
        &mut self,
        question: &str,
        choices: impl IntoIterator<Item = S>,
    ) -> Result<Value, ConsoleError> {
        AskWithCompletion::new(self.question_style()?)
            .render(question, choices)
            .await
    }
    pub async fn ask_with_completion_with<S: Into<String>>(
        &mut self,
        question: &str,
        choices: impl IntoIterator<Item = S>,
        default: Option<Value>,
    ) -> Result<Value, ConsoleError> {
        AskWithCompletion::new(self.question_style()?)
            .render_with(question, choices, default)
            .await
    }
    pub async fn ask_with_completion_callback(
        &mut self,
        question: &str,
        choices: CompletionCallback,
        default: Option<Value>,
    ) -> Result<Value, ConsoleError> {
        AskWithCompletion::new(self.question_style()?)
            .render_with_callback(question, choices, default)
            .await
    }
    pub async fn confirm(&mut self, question: &str) -> Result<bool, ConsoleError> {
        Confirm::new(self.question_style()?).render(question).await
    }
    pub async fn confirm_with(
        &mut self,
        question: &str,
        default: bool,
    ) -> Result<bool, ConsoleError> {
        Confirm::new(self.question_style()?)
            .render_with(question, default)
            .await
    }
    pub async fn secret(&mut self, question: &str) -> Result<Value, ConsoleError> {
        Secret::new(self.question_style()?).render(question).await
    }
    pub async fn secret_with(
        &mut self,
        question: &str,
        fallback: bool,
    ) -> Result<Value, ConsoleError> {
        Secret::new(self.question_style()?)
            .render_with(question, fallback)
            .await
    }
    pub async fn choice<V: Into<Value>>(
        &mut self,
        question: &str,
        choices: impl IntoIterator<Item = V>,
    ) -> Result<Value, ConsoleError> {
        Choice::new(self.question_style()?)
            .render(question, choices)
            .await
    }
    pub async fn choice_with<V: Into<Value>>(
        &mut self,
        question: &str,
        choices: impl IntoIterator<Item = V>,
        default: Option<Value>,
        attempts: Option<usize>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        Choice::new(self.question_style()?)
            .render_with(question, choices, default, attempts, multiple)
            .await
    }
    pub async fn choice_pairs_with<K: Into<ChoiceKey>, V: Into<Value>>(
        &mut self,
        question: &str,
        choices: impl IntoIterator<Item = (K, V)>,
        default: Option<Value>,
        attempts: Option<usize>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        Choice::new(self.question_style()?)
            .render_pairs_with(question, choices, default, attempts, multiple)
            .await
    }
}
macro_rules! line_method {
    ($method:ident,$with:ident,$component:ident) => {
        impl<T: ComponentOutput + ?Sized> Factory<'_, T> {
            pub fn $method(&mut self, text: &str) -> Result<(), ConsoleError> {
                self.$with(text, crate::console::OutputVerbosity::Normal)
            }
            pub fn $with(
                &mut self,
                text: &str,
                options: impl Into<OutputOptions>,
            ) -> Result<(), ConsoleError> {
                let options = options.into();
                if let Some(result) = self.display_line(stringify!($method), text, options) {
                    return result;
                }
                $component::new(self.output).render_with(text, options)
            }
        }
    };
}
line_method!(info, info_with, Info);
line_method!(success, success_with, Success);
line_method!(error, error_with, Error);
line_method!(warn, warn_with, Warn);
impl<T: ComponentOutput + ?Sized> AsOutput for Factory<'_, T> {
    fn as_output(&mut self) -> OutputReference<'_> {
        self.output.as_output()
    }
}
