use super::{
    BuiltinComponentViewCompiler, ComponentContext, ComponentOutput, ComponentViewCompiler,
    mutators::Mutator,
};
use crate::console::{
    AsOutput, ConsoleError, OutputReference, QuestionHelper, QuestionHelperScope, QuestionStyle,
};
use serde_json::Value;
use std::{marker::PhantomData, sync::Arc};

/// 编译后的变换器构造入口；每次数据变换为每一项构造独立对象。
pub trait MutatorConstructor: Send + Sync {
    fn make<'a>(&'a self) -> Result<Box<dyn Mutator + 'a>, ConsoleError>;
}
/// 无参构造的变换器使用Default；零尺寸标记不保存跨调用对象。
pub struct DefaultMutatorConstructor<M>(PhantomData<fn() -> M>);
impl<M> DefaultMutatorConstructor<M> {
    pub const fn new() -> Self {
        Self(PhantomData)
    }
}
impl<M> Default for DefaultMutatorConstructor<M> {
    fn default() -> Self {
        Self::new()
    }
}
impl<M: Mutator + Default + 'static> MutatorConstructor for DefaultMutatorConstructor<M> {
    fn make<'a>(&'a self) -> Result<Box<dyn Mutator + 'a>, ConsoleError> {
        Ok(Box::new(M::default()))
    }
}
/// 问答回调的短期借用；将样式和对象生命周期绑定为同一个作用域。
pub struct QuestionStyleReference<'a> {
    style: &'a mut (dyn QuestionStyle + 'a),
}
impl QuestionStyleReference<'_> {
    pub fn ask_question<'a>(
        &'a mut self,
        question: &'a crate::console::Question,
    ) -> crate::console::ConsoleFuture<'a, Result<Value, ConsoleError>> {
        self.style.ask_question(question)
    }
    pub fn get_question_helper(&self) -> Option<Arc<dyn crate::console::QuestionHelperBehavior>> {
        self.style.get_question_helper()
    }
    pub fn set_question_helper(
        &mut self,
        helper: Option<Arc<dyn crate::console::QuestionHelperBehavior>>,
    ) {
        self.style.set_question_helper(helper);
    }
}
impl AsOutput for QuestionStyleReference<'_> {
    fn as_output(&mut self) -> OutputReference<'_> {
        self.style.as_output()
    }
}
impl QuestionStyle for QuestionStyleReference<'_> {
    fn ask_question<'a>(
        &'a mut self,
        question: &'a crate::console::Question,
    ) -> crate::console::ConsoleFuture<'a, Result<Value, ConsoleError>> {
        self.style.ask_question(question)
    }
    fn get_question_helper(&self) -> Option<Arc<dyn crate::console::QuestionHelperBehavior>> {
        self.style.get_question_helper()
    }
    fn set_question_helper(
        &mut self,
        helper: Option<Arc<dyn crate::console::QuestionHelperBehavior>>,
    ) {
        self.style.set_question_helper(helper);
    }
}
/// 通过组合复用组件数据变换和原始输出，不复制输入或帮助器。
pub struct Component<'a> {
    output: ComponentContext<'a>,
    view_compiler: Box<dyn ComponentViewCompiler>,
}
impl<'a> Component<'a> {
    pub fn new(output: &'a mut (impl ComponentOutput + ?Sized)) -> Self {
        Self::from_context(output.component_context())
    }
    pub fn from_context(output: ComponentContext<'a>) -> Self {
        Self {
            output,
            view_compiler: Box::new(BuiltinComponentViewCompiler::new()),
        }
    }
    pub fn compile(&mut self, view: &str, data: Value) -> Result<String, ConsoleError> {
        self.view_compiler.compile(view, data, &mut self.output)
    }
    pub fn set_view_compiler(&mut self, compiler: Option<Box<dyn ComponentViewCompiler>>) {
        self.view_compiler =
            compiler.unwrap_or_else(|| Box::new(BuiltinComponentViewCompiler::new()));
    }
    pub fn get_output(&mut self) -> OutputReference<'_> {
        self.output.as_output()
    }
    pub fn question_style(&mut self) -> Result<&mut (dyn QuestionStyle + '_), ConsoleError> {
        self.output.question_style()
    }
    pub fn mutate(
        &self,
        mut data: Value,
        constructors: &[&dyn MutatorConstructor],
    ) -> Result<Value, ConsoleError> {
        for constructor in constructors {
            let mutator = constructor.make()?;
            match &mut data {
                Value::Array(values) => {
                    for value in values {
                        *value = mutator.apply_value(std::mem::take(value))?;
                    }
                }
                Value::Object(values) => {
                    for value in values.values_mut() {
                        *value = mutator.apply_value(std::mem::take(value))?;
                    }
                }
                value => *value = mutator.apply_value(std::mem::take(value))?,
            }
        }
        Ok(data)
    }
    pub async fn using_question_helper<T>(
        &mut self,
        operation: impl for<'scope> AsyncFnOnce(
            QuestionStyleReference<'scope>,
        ) -> Result<T, ConsoleError>,
    ) -> Result<T, ConsoleError> {
        let mut scope = QuestionHelperScope::new(
            self.output.question_style()?,
            Arc::new(QuestionHelper::new()),
        );
        operation(QuestionStyleReference { style: &mut *scope }).await
    }
}
impl AsOutput for Component<'_> {
    fn as_output(&mut self) -> OutputReference<'_> {
        self.get_output()
    }
}
