// 第三方许可见 LICENSE 与 LICENSE.renderer。
use super::mutators::{
    EnsureDynamicContentIsHighlighted, EnsurePunctuation, EnsureRelativePaths, Mutator,
};
use crate::console::{AsOutput, ConsoleError, OutputOptions, OutputReference, OutputVerbosity};

/// 借用所选输出，以带标签的组件样式展示文本。
/// ```compile_fail,E0499
/// use openrust::console::{Output, view::components::Line};
/// let mut output = Output::default();
/// let mut line = Line::new(&mut output);
/// output.stdout.clear();
/// line.render("info", "value").unwrap();
/// ```
pub struct Line<'a> {
    output: OutputReference<'a>,
}
impl<'a> Line<'a> {
    pub fn new(output: &'a mut (impl AsOutput + ?Sized)) -> Self {
        Self {
            output: output.as_output(),
        }
    }
    pub fn render(&mut self, style: &str, text: &str) -> Result<(), ConsoleError> {
        self.render_with(style, text, OutputVerbosity::Normal)
    }
    pub fn render_with(
        &mut self,
        style: &str,
        text: &str,
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        let text = EnsureDynamicContentIsHighlighted.apply(text)?;
        let text = EnsurePunctuation.apply(&text)?;
        let text = EnsureRelativePaths.apply(&text)?;
        let (background, foreground, label) = match style {
            "info" => ("blue", "white", "INFO"),
            "success" => ("green", "white", "SUCCESS"),
            "warn" => ("yellow", "black", "WARN"),
            "error" => ("red", "white", "ERROR"),
            _ => {
                return Err(ConsoleError::new(format!(
                    "未定义的控制台组件样式：{style}"
                )));
            }
        };
        let margin = self
            .output
            .parent_count()
            .map_or(1, |lines| 2usize.saturating_sub(lines));
        let text = normalize_view_text(&text);
        let message = format!(
            "{}  <bg={background};fg={foreground}> {label} </> {text}  \n",
            "\n".repeat(margin)
        );
        self.output.writeln(message.as_str(), options)
    }
}

// 组件模板将内容编码为HTML文本；保留对应文本节点的控制字符与空白语义。
// 不对任意HTML公开解析接口，也不进行路径安全规范化。
pub(super) fn normalize_view_text(text: &str) -> String {
    let mut result = String::new();
    let mut space = false;
    for character in text.chars() {
        match character {
            '\0' | '\t' | '\n' | '\r' | ' ' => space = !result.is_empty(),
            '\u{1}'..='\u{1f}' => {}
            _ => {
                if space {
                    result.push(' ');
                }
                space = false;
                result.push(character);
            }
        }
    }
    result
}
macro_rules! line_component {
    ($name:ident, $style:literal) => {
        pub struct $name<'a> {
            line: Line<'a>,
        }
        impl<'a> $name<'a> {
            pub fn new(output: &'a mut (impl AsOutput + ?Sized)) -> Self {
                Self {
                    line: Line::new(output),
                }
            }
            pub fn render(&mut self, text: &str) -> Result<(), ConsoleError> {
                self.line.render($style, text)
            }
            pub fn render_with(
                &mut self,
                text: &str,
                options: impl Into<OutputOptions>,
            ) -> Result<(), ConsoleError> {
                self.line.render_with($style, text, options)
            }
        }
    };
}
line_component!(Info, "info");
line_component!(Success, "success");
line_component!(Warn, "warn");
line_component!(Error, "error");
