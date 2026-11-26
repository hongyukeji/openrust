// 第三方许可见 LICENSE.renderer。
mod code;
mod dimensions;
pub use dimensions::TerminalDimensions;
mod dom;
mod element;
mod inheritance;
pub use element::{TerminalChild, TerminalContent, TerminalElement, TerminalElementKind};
mod layout;
mod lists;
mod modifiers;
pub use modifiers::TerminalStyleModifiers;
mod palette;
mod pre;
mod properties;
mod repository;
pub use repository::{StyleRepository, TerminalStyle, TerminalStyles};
pub use style_methods::StyleArgument;
mod style;
mod style_methods;
mod style_tokens;
mod table;
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
pub struct BuiltinComponentViewRenderer {
    styles: Option<StyleRepository>,
}
impl BuiltinComponentViewRenderer {
    pub const fn new() -> Self {
        Self { styles: None }
    }

    pub fn with_styles(styles: StyleRepository) -> Self {
        Self {
            styles: Some(styles),
        }
    }
}
impl ComponentViewRenderer for BuiltinComponentViewRenderer {
    fn render(
        &mut self,
        html: &str,
        options: OutputOptions,
        output: &mut ComponentContext<'_>,
    ) -> Result<(), ConsoleError> {
        let markup = layout::render(html, self.styles.as_ref())?;
        output.as_output().writeln(markup.as_str(), options)
    }
}
