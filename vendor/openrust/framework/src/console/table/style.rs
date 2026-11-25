use super::{ConsoleError, ConsoleTableError};
/// 单元格填充位置，编译期排除无效的整数标志。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TablePadding {
    Left,
    #[default]
    Right,
    Both,
}
/// 表格及列的样式配置，不修改进程级默认值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableStyle {
    pub(super) padding_char: String,
    pub(super) borders: [String; 4],
    pub(super) crossings: [String; 12],
    pub(super) pad_type: TablePadding,
    pub(super) outside: bool,
    pub(super) cell_header_format: String,
    pub(super) cell_row_format: String,
    pub(super) cell_row_content_format: String,
    pub(super) border_format: String,
    pub(super) header_title_format: String,
    pub(super) footer_title_format: String,
}
impl Default for TableStyle {
    fn default() -> Self {
        Self {
            padding_char: " ".into(),
            borders: ["-", "|", "-", "|"].map(str::to_owned),
            crossings: std::array::from_fn(|_| "+".into()),
            pad_type: TablePadding::Right,
            outside: true,
            cell_header_format: "<info>%s</info>".into(),
            cell_row_format: "%s".into(),
            cell_row_content_format: " %s ".into(),
            border_format: "%s".into(),
            header_title_format: "<fg=black;bg=white;options=bold> %s </>".into(),
            footer_title_format: "<fg=black;bg=white;options=bold> %s </>".into(),
        }
    }
}
impl TableStyle {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn from_name(name: &str) -> Result<Self, ConsoleError> {
        let mut style = Self::default();
        match name {
            "default" => {}
            "markdown" => {
                style
                    .set_default_crossing_char("|")
                    .set_display_outside_border(false);
            }
            "borderless" => {
                style
                    .set_horizontal_border_chars("=", None)
                    .set_vertical_border_chars(" ", None)
                    .set_default_crossing_char(" ");
            }
            "compact" => {
                style
                    .set_horizontal_border_chars("", None)
                    .set_vertical_border_chars("", None)
                    .set_default_crossing_char("")
                    .set_cell_row_content_format("%s ");
            }
            "symfony-style-guide" => {
                style
                    .set_vertical_border_chars(" ", None)
                    .set_default_crossing_char(" ")
                    .set_cell_header_format("%s");
            }
            "box" => {
                style
                    .set_horizontal_border_chars("─", None)
                    .set_vertical_border_chars("│", None)
                    .set_crossing_chars([
                        "┼", "┌", "┬", "┐", "┤", "┘", "┴", "└", "├", "├", "┼", "┤",
                    ]);
            }
            "box-double" => {
                style
                    .set_horizontal_border_chars("═", Some("─"))
                    .set_vertical_border_chars("║", Some("│"))
                    .set_crossing_chars([
                        "┼", "╔", "╤", "╗", "╢", "╝", "╧", "╚", "╟", "╠", "╪", "╣",
                    ]);
            }
            _ => {
                return Err(ConsoleError::from_source(
                    ConsoleTableError::UndefinedStyle(name.to_owned()),
                ));
            }
        }
        Ok(style)
    }
    pub fn set_padding_char(
        &mut self,
        value: impl Into<String>,
    ) -> Result<&mut Self, ConsoleError> {
        let value = value.into();
        if value.is_empty() {
            return Err(ConsoleError::from_source(ConsoleTableError::EmptyPadding));
        }
        self.padding_char = value;
        Ok(self)
    }
    pub fn get_padding_char(&self) -> &str {
        &self.padding_char
    }
    pub fn set_horizontal_border_chars(
        &mut self,
        outside: &str,
        inside: Option<&str>,
    ) -> &mut Self {
        self.borders[0] = outside.into();
        self.borders[2] = inside.unwrap_or(outside).into();
        self
    }
    pub fn set_vertical_border_chars(&mut self, outside: &str, inside: Option<&str>) -> &mut Self {
        self.borders[1] = outside.into();
        self.borders[3] = inside.unwrap_or(outside).into();
        self
    }
    pub fn get_border_chars(&self) -> &[String; 4] {
        &self.borders
    }
    /// 顺序与交叉点定义一致；末三项为表头与正文之间的交叉点。
    pub fn set_crossing_chars(&mut self, chars: [&str; 12]) -> &mut Self {
        self.crossings = chars.map(str::to_owned);
        self
    }
    pub fn set_default_crossing_char(&mut self, value: &str) -> &mut Self {
        self.crossings = std::array::from_fn(|_| value.to_owned());
        self
    }
    pub fn get_crossing_char(&self) -> &str {
        &self.crossings[0]
    }
    pub fn get_crossing_chars(&self) -> &[String; 12] {
        &self.crossings
    }
    pub fn set_pad_type(&mut self, value: TablePadding) -> &mut Self {
        self.pad_type = value;
        self
    }
    pub fn get_pad_type(&self) -> TablePadding {
        self.pad_type
    }
    pub fn set_display_outside_border(&mut self, value: bool) -> &mut Self {
        self.outside = value;
        self
    }
    pub fn display_outside_border(&self) -> bool {
        self.outside
    }
    pub fn set_cell_header_format(&mut self, value: impl Into<String>) -> &mut Self {
        self.cell_header_format = value.into();
        self
    }
    pub fn get_cell_header_format(&self) -> &str {
        &self.cell_header_format
    }
    pub fn set_cell_row_format(&mut self, value: impl Into<String>) -> &mut Self {
        self.cell_row_format = value.into();
        self
    }
    pub fn get_cell_row_format(&self) -> &str {
        &self.cell_row_format
    }
    pub fn set_cell_row_content_format(&mut self, value: impl Into<String>) -> &mut Self {
        self.cell_row_content_format = value.into();
        self
    }
    pub fn get_cell_row_content_format(&self) -> &str {
        &self.cell_row_content_format
    }
    pub fn set_border_format(&mut self, value: impl Into<String>) -> &mut Self {
        self.border_format = value.into();
        self
    }
    pub fn get_border_format(&self) -> &str {
        &self.border_format
    }
    pub fn set_header_title_format(&mut self, value: impl Into<String>) -> &mut Self {
        self.header_title_format = value.into();
        self
    }
    pub fn get_header_title_format(&self) -> &str {
        &self.header_title_format
    }
    pub fn set_footer_title_format(&mut self, value: impl Into<String>) -> &mut Self {
        self.footer_title_format = value.into();
        self
    }
    pub fn get_footer_title_format(&self) -> &str {
        &self.footer_title_format
    }
}
