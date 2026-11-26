use super::{
    ConsoleFormatError,
    formatting::{Style, color, style_option},
};
/// 已编译的ANSI颜色；对象创建后不再读取终端能力。
#[derive(Clone, Default)]
pub struct Color {
    style: Style,
}
impl Color {
    pub fn new(
        foreground: &str,
        background: &str,
        options: &[&str],
    ) -> Result<Self, ConsoleFormatError> {
        let mut style = Style {
            fg: if foreground.is_empty() {
                String::new()
            } else {
                color(foreground, false)?
            },
            bg: if background.is_empty() {
                String::new()
            } else {
                color(background, true)?
            },
            ..Style::default()
        };
        for option in options {
            let option = style_option(option)?;
            if !style.options.contains(&option) {
                style.options.push(option);
            }
        }
        Ok(Self { style })
    }
    pub fn apply(&self, text: &str) -> String {
        let mut output = String::new();
        self.style.append(text, true, &mut output);
        output
    }
    pub fn set(&self) -> String {
        self.style.set()
    }
    pub fn unset(&self) -> String {
        self.style.unset()
    }
}
