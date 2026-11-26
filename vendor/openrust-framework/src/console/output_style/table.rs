use super::{ConsoleError, OutputStyle};

impl OutputStyle<'_> {
    /// 创建借用当前底层输出的样式指南表格；真实控制台使用独立分区。
    pub fn create_table(&mut self) -> Result<super::super::Table<'_>, ConsoleError> {
        let mut table = super::super::Table::from_reference(self.output.for_style_component())?;
        let mut style = table.style_definition_snapshot("symfony-style-guide")?;
        style.set_cell_header_format("<info>%s</info>");
        table.set_style(style);
        Ok(table)
    }
}

impl OutputStyle<'_> {
    pub fn table<I, R>(
        &mut self,
        headers: impl Into<super::super::TableHeaders>,
        rows: I,
    ) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = R>,
        R: Into<super::super::TableRow>,
    {
        self.render_style_table(headers.into(), rows, false)
    }
    pub fn horizontal_table<I, R>(
        &mut self,
        headers: impl Into<super::super::TableHeaders>,
        rows: I,
    ) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = R>,
        R: Into<super::super::TableRow>,
    {
        self.render_style_table(headers.into(), rows, true)
    }
    fn render_style_table<I, R>(
        &mut self,
        headers: super::super::TableHeaders,
        rows: I,
        horizontal: bool,
    ) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = R>,
        R: Into<super::super::TableRow>,
    {
        {
            let mut table = self.create_table()?;
            table.set_horizontal(horizontal);
            match headers {
                super::super::TableHeaders::Cells(cells) => {
                    table.set_headers(cells);
                }
                super::super::TableHeaders::Rows(rows) => {
                    table.set_headers_rows(rows);
                }
            }
            table.set_rows(rows).render()?;
        }
        self.new_line(1)
    }
    pub fn definition_list<I, T>(&mut self, list: I) -> Result<(), ConsoleError>
    where
        I: IntoIterator<Item = T>,
        T: Into<super::DefinitionListItem>,
    {
        use super::super::TableCell;
        use super::DefinitionListItem;
        let mut headers = Vec::<TableCell>::new();
        let mut row = Vec::<TableCell>::new();
        for item in list {
            match item.into() {
                DefinitionListItem::Title(title) => {
                    headers.push(TableCell::from(title).with_colspan(2));
                    row.push(serde_json::Value::Null.into());
                }
                DefinitionListItem::Separator(separator) => {
                    headers.push(separator.clone().into());
                    row.push(separator.into());
                }
                DefinitionListItem::Entries(entries) => {
                    if let Some((key, value)) = entries.into_iter().next() {
                        headers.push(TableCell::from(key.to_string()));
                        row.push(value);
                    } else {
                        headers.push(serde_json::Value::Null.into());
                        row.push(TableCell::default());
                    }
                }
            }
        }
        self.horizontal_table(headers, [row])
    }
}
