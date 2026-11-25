use super::{ConsoleError, OutputStyle};

impl OutputStyle<'_> {
    /// 创建借用当前底层输出的样式指南表格；真实控制台使用独立分区。
    pub fn create_table(&mut self) -> Result<super::super::Table<'_>, ConsoleError> {
        let mut table = super::super::Table::from_reference(self.output.for_style_table())?;
        let mut style = table.style_definition_snapshot("symfony-style-guide")?;
        style.set_cell_header_format("<info>%s</info>");
        table.set_style(style);
        Ok(table)
    }
}
