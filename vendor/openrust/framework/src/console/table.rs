use super::{ConsoleError, OutputStyle, OutputVerbosity};
mod error;
pub use error::ConsoleTableError;
mod cell;
pub use cell::TableCell;

mod style;
use std::collections::BTreeMap;
pub use style::{TablePadding, TableStyle};

/// 标准矩形表格；输入转成文本后按可见宽度布局，标签交给原格式器处理。
pub struct Table<'a, 'o> {
    output: &'a mut OutputStyle<'o>,
    headers: Vec<Vec<String>>,
    rows: Vec<Vec<String>>,
    style: TableStyle,
    column_styles: BTreeMap<usize, TableStyle>,
}
impl<'a, 'o> Table<'a, 'o> {
    pub fn new(output: &'a mut OutputStyle<'o>) -> Self {
        Self {
            output,
            headers: Vec::new(),
            rows: Vec::new(),
            style: TableStyle::default(),
            column_styles: BTreeMap::new(),
        }
    }
    pub fn set_headers<I, T>(&mut self, headers: I) -> &mut Self
    where
        I: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        self.headers = vec![
            headers
                .into_iter()
                .map(|cell| cell.into().to_string())
                .collect(),
        ];
        self
    }
    pub fn set_rows<I, R, T>(&mut self, rows: I) -> &mut Self
    where
        I: IntoIterator<Item = R>,
        R: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        self.rows = rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| cell.into().to_string())
                    .collect()
            })
            .collect();
        self
    }
    pub fn add_row<I, T>(&mut self, row: I) -> &mut Self
    where
        I: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        self.rows.push(
            row.into_iter()
                .map(|cell| cell.into().to_string())
                .collect(),
        );
        self
    }
    pub fn set_style(&mut self, style: TableStyle) -> &mut Self {
        self.style = style;
        self
    }
    pub fn get_style(&self) -> &TableStyle {
        &self.style
    }
    pub fn set_column_style(&mut self, column: usize, style: TableStyle) -> &mut Self {
        self.column_styles.insert(column, style);
        self
    }
    pub fn get_column_style(&self, column: usize) -> &TableStyle {
        self.column_styles.get(&column).unwrap_or(&self.style)
    }
    pub fn render(&mut self) -> Result<(), ConsoleError> {
        let count = self
            .headers
            .iter()
            .chain(&self.rows)
            .map(Vec::len)
            .max()
            .unwrap_or_default();
        if count == 0 {
            return Ok(());
        }
        let mut widths = vec![0; count];
        for row in self.headers.iter().chain(&self.rows) {
            for (column, cell) in row.iter().enumerate() {
                for line in lines(cell) {
                    let visible = self
                        .output
                        .get_output()
                        .formatter
                        .format(line, false)
                        .map_err(ConsoleError::from_source)?;
                    widths[column] = widths[column].max(width(&visible));
                }
            }
        }
        let padding = width(&self.style.cell_row_content_format).saturating_sub(2);
        for value in &mut widths {
            *value += padding;
        }
        let headers = self.headers.clone();
        let rows = self.rows.clone();
        let has_headers = headers.iter().any(|row| !row.is_empty());
        let has_rows = rows.iter().any(|row| !row.is_empty());
        if has_headers && self.style.display_outside_border() {
            self.separator(&widths, 0)?;
        }
        for row in &headers {
            self.row(row, &widths, true)?;
        }
        if has_rows {
            self.separator(&widths, 1)?;
        }
        for row in &rows {
            self.row(row, &widths, false)?;
        }
        if self.style.display_outside_border() {
            self.separator(&widths, 2)?;
        }
        Ok(())
    }
    fn separator(&mut self, widths: &[usize], position: usize) -> Result<(), ConsoleError> {
        let b = &self.style.borders;
        let c = &self.style.crossings;
        if b[0].is_empty() && b[2].is_empty() && c[0].is_empty() {
            return Ok(());
        }
        let (horizontal, left, mid, right) = match position {
            0 => (&b[0], &c[1], &c[2], &c[3]),
            2 => (&b[0], &c[7], &c[6], &c[5]),
            _ => (&b[0], &c[9], &c[10], &c[11]),
        };
        let mut line = left.clone();
        for (column, width) in widths.iter().enumerate() {
            line.push_str(&horizontal.repeat(*width));
            line.push_str(if column + 1 == widths.len() {
                right
            } else {
                mid
            });
        }
        let line = template(&self.style.border_format, &line)?;
        self.output.writeln(line.as_str(), OutputVerbosity::Normal)
    }
    fn row(&mut self, row: &[String], widths: &[usize], header: bool) -> Result<(), ConsoleError> {
        if row.is_empty() {
            return Ok(());
        }
        let parts: Vec<Vec<String>> = row
            .iter()
            .map(|cell| {
                let mut parts: Vec<_> = lines(cell).map(str::to_owned).collect();
                let count = parts.len();
                if count > 1 {
                    for (index, part) in parts.iter_mut().enumerate() {
                        *part = super::formatting::Formatter::escape_trailing_backslash(part);
                        if index + 1 < count {
                            part.push_str("<fg=default;bg=default></>");
                        }
                    }
                }
                parts
            })
            .collect();
        let height = parts.iter().map(Vec::len).max().unwrap_or_default();
        let border = template(&self.style.border_format, &self.style.borders[1])?;
        let inside = template(&self.style.border_format, &self.style.borders[3])?;
        for index in 0..height {
            let mut line = border.clone();
            for (column, target) in widths.iter().enumerate() {
                let cell = parts
                    .get(column)
                    .and_then(|part| part.get(index))
                    .map(String::as_str)
                    .unwrap_or("");
                let visible = self
                    .output
                    .get_output()
                    .formatter
                    .format(cell, false)
                    .map_err(ConsoleError::from_source)?;
                let style = self.get_column_style(column);
                let content = template(&style.cell_row_content_format, cell)?;
                let target = target + cell.len()
                    - width(&visible)
                    - cell.bytes().filter(|byte| *byte == 0).count();
                let missing = target.saturating_sub(content.len());
                let (left, right) = match style.pad_type {
                    TablePadding::Left => (missing, 0),
                    TablePadding::Right => (0, missing),
                    TablePadding::Both => (missing / 2, missing - missing / 2),
                };
                let content = format!(
                    "{}{}{}",
                    padding(&style.padding_char, left)?,
                    content,
                    padding(&style.padding_char, right)?
                );
                line.push_str(&template(
                    if header {
                        &self.style.cell_header_format
                    } else {
                        &self.style.cell_row_format
                    },
                    &content,
                )?);
                line.push_str(if column + 1 < widths.len() {
                    &inside
                } else {
                    &border
                });
            }
            self.output
                .writeln(line.as_str(), OutputVerbosity::Normal)?;
        }
        Ok(())
    }
}
fn lines(value: &str) -> impl Iterator<Item = &str> {
    let separator = if value.contains("\r\n") { "\r\n" } else { "\n" };
    value.split(separator)
}
fn width(value: &str) -> usize {
    ::console::strip_ansi_codes(value)
        .chars()
        .map(|ch| {
            if ch.is_control() {
                1
            } else {
                let mut bytes = [0; 4];
                ::console::measure_text_width(ch.encode_utf8(&mut bytes))
            }
        })
        .sum()
}

impl super::CommandIo<'_> {
    pub fn table<H, HT, R, RT, C>(&mut self, headers: H, rows: R) -> Result<(), ConsoleError>
    where
        H: IntoIterator<Item = HT>,
        HT: Into<TableCell>,
        R: IntoIterator<Item = RT>,
        RT: IntoIterator<Item = C>,
        C: Into<TableCell>,
    {
        self.table_with(headers, rows, TableStyle::default())
    }
    pub fn table_with<H, HT, R, RT, C>(
        &mut self,
        headers: H,
        rows: R,
        style: TableStyle,
    ) -> Result<(), ConsoleError>
    where
        H: IntoIterator<Item = HT>,
        HT: Into<TableCell>,
        R: IntoIterator<Item = RT>,
        RT: IntoIterator<Item = C>,
        C: Into<TableCell>,
    {
        self.table_with_columns(headers, rows, style, std::iter::empty())
    }
    pub fn table_with_columns<H, HT, R, RT, C, S>(
        &mut self,
        headers: H,
        rows: R,
        style: TableStyle,
        column_styles: S,
    ) -> Result<(), ConsoleError>
    where
        H: IntoIterator<Item = HT>,
        HT: Into<TableCell>,
        R: IntoIterator<Item = RT>,
        RT: IntoIterator<Item = C>,
        C: Into<TableCell>,
        S: IntoIterator<Item = (usize, TableStyle)>,
    {
        let mut table = Table::new(self.get_output());
        table.set_headers(headers).set_rows(rows).set_style(style);
        for (column, style) in column_styles {
            table.set_column_style(column, style);
        }
        table.render()
    }
}

fn template(format: &str, value: &str) -> Result<String, ConsoleError> {
    super::question::format_template(format, &serde_json::Value::String(value.to_owned()))
        .map_err(|error| ConsoleError::from_source(ConsoleTableError::InvalidFormat(error)))
}
fn padding(value: &str, count: usize) -> Result<String, ConsoleError> {
    let mut result = Vec::with_capacity(count);
    result.extend(value.as_bytes().iter().copied().cycle().take(count));
    String::from_utf8(result).map_err(|error| {
        ConsoleError::from_source(ConsoleTableError::InvalidPaddingEncoding(error))
    })
}
