// 第三方许可见 LICENSE.renderer。
mod dimensions;
mod dom;
mod layout;
mod lists;
mod palette;
mod pre;
mod style;
mod style_methods;
mod style_tokens;
mod tree;
use super::ComponentContext;
use crate::console::{AsOutput, ConsoleError, OutputOptions};
/// 组件拥有的渲染策略；编译结果、选项及原输出上下文按次传递。
pub trait ComponentViewRenderer: Send {
    fn render(
        &mut self,
        html: &str,
        options: OutputOptions,
        output: &mut ComponentContext<'_>,
    ) -> Result<(), ConsoleError>;
}
#[derive(Default)]
#[non_exhaustive]
pub struct BuiltinComponentViewRenderer;
impl BuiltinComponentViewRenderer {
    pub const fn new() -> Self {
        Self
    }
}
impl ComponentViewRenderer for BuiltinComponentViewRenderer {
    fn render(
        &mut self,
        html: &str,
        options: OutputOptions,
        output: &mut ComponentContext<'_>,
    ) -> Result<(), ConsoleError> {
        let markup = layout::render(html)?;
        output.as_output().writeln(markup.as_str(), options)
    }
}
