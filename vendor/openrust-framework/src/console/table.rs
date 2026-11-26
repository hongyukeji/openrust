use super::formatting::display_width as width;
use super::{ConsoleError, OutputOptions, OutputReference};
mod error;
pub use error::ConsoleTableError;
mod cell_style;
pub use cell_style::TableCellStyle;
use cell_style::{AttributeRows, Attributes};
mod cell;
pub use cell::{TableCell, TableCellOptions};

mod definitions;
pub(crate) use definitions::Definitions as TableDefinitions;
mod headers;
mod layout;
mod row;
pub use headers::TableHeaders;
mod span_layout;
pub use row::{TableRow, TableSeparator};
mod rows;
use crate::support::ArrayKey;
use rows::Orientation;
mod style;
use std::collections::BTreeMap;
pub use style::{TablePadding, TableStyle};

/// 标准矩形表格；输入转成文本后按可见宽度布局，标签交给原格式器处理。
pub struct Table<'a> {
    definitions: std::sync::Arc<definitions::Definitions>,
    output: OutputReference<'a>,
    headers: Vec<Vec<TableCell>>,
    rows: Vec<Vec<TableCell>>,
    row_keys: Vec<ArrayKey>,
    separators: std::collections::BTreeSet<usize>,
    max_integer_key: Option<i64>,
    orientation: Orientation,
    style: TableStyle,
    column_styles: BTreeMap<usize, TableStyle>,
    column_widths: BTreeMap<usize, i64>,
    column_max_widths: BTreeMap<usize, i64>,
    header_title: Option<String>,
    footer_title: Option<String>,
    rendered: bool,
}
impl<'a> Table<'a> {
    pub fn new(output: &'a mut (impl super::AsOutput + ?Sized)) -> Result<Self, ConsoleError> {
        Self::from_reference(output.as_output())
    }
    pub(super) fn from_reference(output: OutputReference<'a>) -> Result<Self, ConsoleError> {
        let definitions = definitions::current(false)?;
        let style = definitions.get("default")?;
        Ok(Self {
            definitions,
            output,
            headers: Vec::new(),
            rows: Vec::new(),
            row_keys: Vec::new(),
            separators: std::collections::BTreeSet::new(),
            max_integer_key: None,
            orientation: Orientation::Default,
            style,
            column_styles: BTreeMap::new(),
            column_widths: BTreeMap::new(),
            column_max_widths: BTreeMap::new(),
            header_title: None,
            footer_title: None,
            rendered: false,
        })
    }
    pub(super) fn style_definition_snapshot(&self, name: &str) -> Result<TableStyle, ConsoleError> {
        let shared = self.definitions.get(name)?;
        // 命名定义返回共享句柄，工厂必须深拷贝其状态。
        Ok(shared.clone())
    }
    pub fn set_headers<I, T>(&mut self, headers: I) -> &mut Self
    where
        I: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        let headers: Vec<_> = headers.into_iter().map(Into::into).collect();
        self.headers = if headers.is_empty() {
            Vec::new()
        } else {
            vec![headers]
        };
        self
    }
    pub fn set_rows<I, R>(&mut self, rows: I) -> &mut Self
    where
        I: IntoIterator<Item = R>,
        R: Into<TableRow>,
    {
        self.rows.clear();
        self.row_keys.clear();
        self.separators.clear();
        self.max_integer_key = None;
        for row in rows {
            self.add_row(row).expect("内存中的行索引可表示");
        }
        self
    }
    pub fn add_row(&mut self, row: impl Into<TableRow>) -> Result<&mut Self, ConsoleError> {
        let key = match self.max_integer_key {
            None => 0,
            Some(index) => index
                .checked_add(1)
                .ok_or_else(|| ConsoleError::from_source(ConsoleTableError::RowIndexOverflow))?,
        };
        self.insert_row(key.into(), row.into());
        Ok(self)
    }
    pub fn set_style(&mut self, style: impl Into<TableStyle>) -> &mut Self {
        self.style = style.into();
        self
    }
    pub fn get_style(&self) -> &TableStyle {
        &self.style
    }
    pub fn set_column_style(&mut self, column: usize, style: impl Into<TableStyle>) -> &mut Self {
        self.column_styles.insert(column, style.into());
        self
    }
    pub fn get_column_style(&self, column: usize) -> &TableStyle {
        self.column_styles.get(&column).unwrap_or(&self.style)
    }
    pub fn append_row(&mut self, row: impl Into<TableRow>) -> Result<&mut Self, ConsoleError> {
        if self.output.direct_section().is_none() {
            return Err(ConsoleError::new("追加并重绘表格需要控制台分区输出"));
        }
        if self.rendered {
            let orientation = std::mem::replace(&mut self.orientation, Orientation::Default);
            let result = self.prepare_layout();
            self.orientation = orientation;
            let layout = result?;
            let count = layout.headers.len()
                + layout.rows.len()
                + 1
                + usize::from(!self.headers.is_empty())
                + usize::from(!self.rows.is_empty());
            self.output
                .clear_section(i64::try_from(count).map_err(ConsoleError::from_source)?)?;
        }
        self.add_row(row)?;
        self.render()?;
        Ok(self)
    }
    pub fn render(&mut self) -> Result<(), ConsoleError> {
        let prepared = self.prepare_layout()?;
        let (headers, rows) = (&prepared.headers, &prepared.rows);
        let count = prepared.number_of_columns;
        if count == 0 && headers.iter().chain(rows).all(Vec::is_empty) {
            self.rendered = true;
            return Ok(());
        }
        let widths = self.calculate_widths(&prepared, count)?;
        let mut title_pending = true;
        for (index, row) in headers.iter().enumerate() {
            if !row.is_empty()
                && !prepared.header_continuations.contains(&index)
                && self.style.display_outside_border()
            {
                self.separator(&widths, 0, title_pending)?;
                title_pending = false;
            }
            self.row(
                row,
                &widths,
                true,
                prepared.unsplit_headers.get(&index),
                prepared.header_attributes.get(&index),
            )?;
        }
        let mut first_row = true;
        for (index, row) in rows.iter().enumerate() {
            if prepared.separators.contains(&index) {
                self.separator(&widths, 3, false)?;
                continue;
            }
            if first_row && !row.is_empty() {
                if self.orientation == Orientation::Horizontal
                    || self.orientation == Orientation::Vertical
                        && self.style.display_outside_border()
                {
                    self.separator(&widths, 0, title_pending)?;
                } else if self.orientation == Orientation::Default {
                    self.separator(&widths, 1, title_pending)?;
                }
                first_row = false;
            }
            self.row(
                row,
                &widths,
                false,
                prepared.unsplit_rows.get(&index),
                prepared.row_attributes.get(&index),
            )?;
        }
        if self.style.display_outside_border() {
            self.separator(&widths, 2, false)?;
        }
        self.rendered = true;
        Ok(())
    }
    fn separator(
        &mut self,
        widths: &[i64],
        position: usize,
        include_header: bool,
    ) -> Result<(), ConsoleError> {
        if widths.is_empty() {
            return Ok(());
        }
        let style = self.style.snapshot();
        let b = &style.borders;
        let c = &style.crossings;
        if b[0].is_empty() && b[2].is_empty() && c[0].is_empty() {
            return Ok(());
        }
        let (horizontal, left, mid, right) = match position {
            0 => (&b[0], &c[1], &c[2], &c[3]),
            2 => (&b[0], &c[7], &c[6], &c[5]),
            3 => (&b[2], &c[8], &c[0], &c[4]),
            _ => (&b[0], &c[9], &c[10], &c[11]),
        };
        let mut line = left.clone();
        for (column, width) in widths.iter().enumerate() {
            line.push_str(&horizontal.repeat(usize::try_from(*width).map_err(|_| {
                ConsoleError::from_source(ConsoleTableError::InvalidColumnWidth(*width))
            })?));
            line.push_str(if column + 1 == widths.len() {
                right
            } else {
                mid
            });
        }
        let title = if position == 2 {
            self.footer_title
                .clone()
                .map(|title| (title, self.style.get_footer_title_format()))
        } else if include_header {
            self.header_title
                .as_ref()
                .filter(|title| !title.is_empty() && title.as_str() != "0")
                .cloned()
                .map(|title| (title, self.style.get_header_title_format()))
        } else {
            None
        };
        if let Some((title, format)) = title {
            line = self.with_title(&line, &title, &format)?;
        }
        let line = template(&self.style.get_border_format(), &line)?;
        self.output.writeln(line.as_str(), OutputOptions::default())
    }
    fn row(
        &mut self,
        row: &[String],
        widths: &[i64],
        header: bool,
        unsplit: Option<&std::collections::BTreeSet<usize>>,
        attributes: Option<&BTreeMap<usize, Attributes>>,
    ) -> Result<(), ConsoleError> {
        if row.is_empty() {
            return Ok(());
        }
        let parts = row_parts(row, unsplit);
        let height = parts.iter().map(Vec::len).max().unwrap_or_default();
        for index in 0..height {
            let cell_format = if header {
                self.style.get_cell_header_format()
            } else {
                self.style.get_cell_row_format()
            };
            let first_format = (self.orientation == Orientation::Horizontal)
                .then(|| self.style.get_cell_header_format());
            let mut line = self.column_separator(false)?;
            let mut columns: Vec<Option<usize>> = if widths.is_empty() {
                vec![Some(0), None]
            } else {
                (0..widths.len()).map(Some).collect()
            };
            if let Some(attributes) = attributes {
                for (index, attribute) in attributes {
                    if attribute.colspan > 1 {
                        let end = index
                            .checked_add(
                                usize::try_from(attribute.colspan)
                                    .map_err(ConsoleError::from_source)?,
                            )
                            .ok_or_else(|| {
                                ConsoleError::from_source(ConsoleTableError::InvalidSpanLayout)
                            })?;
                        columns.retain(|column| {
                            column.is_none_or(|column| column <= *index || column >= end)
                        });
                    }
                }
            }
            let last = columns.len().saturating_sub(1);
            for (position, column) in columns.into_iter().enumerate() {
                let key = column.unwrap_or(usize::MAX);
                let cell = parts
                    .get(key)
                    .and_then(|part| part.get(index))
                    .map(String::as_str)
                    .unwrap_or("");
                let attribute = column.and_then(|key| attributes.and_then(|row| row.get(&key)));
                let mut target = column
                    .and_then(|column| widths.get(column))
                    .copied()
                    .unwrap_or(0);
                if let Some(attribute) = attribute.filter(|attribute| attribute.colspan > 1) {
                    let start = column.ok_or_else(|| {
                        ConsoleError::from_source(ConsoleTableError::InvalidSpanLayout)
                    })?;
                    let border = i64::try_from(width(&self.column_separator(true)?))
                        .map_err(ConsoleError::from_source)?;
                    for offset in 1..attribute.colspan {
                        let next = start
                            .checked_add(
                                usize::try_from(offset).map_err(ConsoleError::from_source)?,
                            )
                            .ok_or_else(|| {
                                ConsoleError::from_source(ConsoleTableError::InvalidSpanLayout)
                            })?;
                        target = target
                            .checked_add(border)
                            .and_then(|value| {
                                value.checked_add(widths.get(next).copied().unwrap_or(0))
                            })
                            .ok_or_else(|| {
                                ConsoleError::from_source(ConsoleTableError::InvalidColumnWidth(
                                    target,
                                ))
                            })?;
                    }
                }
                let rendered = self.render_cell(
                    cell,
                    target,
                    column,
                    if position == 0 {
                        first_format.as_deref().unwrap_or(&cell_format)
                    } else {
                        &cell_format
                    },
                    attribute,
                )?;
                line.push_str(&rendered);
                line.push_str(&self.column_separator(column.unwrap_or(position) != last)?);
            }
            self.output
                .writeln(line.as_str(), OutputOptions::default())?;
        }
        Ok(())
    }
    fn column_separator(&self, inside: bool) -> Result<String, ConsoleError> {
        template(
            &self.style.get_border_format(),
            &self.style.get_border_chars()[if inside { 3 } else { 1 }],
        )
    }
    fn calculate_widths(
        &mut self,
        prepared: &layout::PreparedLayout,
        count: usize,
    ) -> Result<Vec<i64>, ConsoleError> {
        let mut physical = Vec::new();
        for (body, rows, masks, attributes) in [
            (
                false,
                &prepared.headers,
                &prepared.unsplit_headers,
                &prepared.header_attributes,
            ),
            (
                true,
                &prepared.rows,
                &prepared.unsplit_rows,
                &prepared.row_attributes,
            ),
        ] {
            for (index, row) in rows.iter().enumerate() {
                if body && prepared.separators.contains(&index) {
                    continue;
                }
                let parts = row_parts(row, masks.get(&index));
                let height = parts.iter().map(Vec::len).max().unwrap_or_default();
                for line in 0..height {
                    let values = parts
                        .iter()
                        .map(|column| column.get(line).cloned().unwrap_or_default())
                        .collect::<Vec<_>>();
                    let attrs = attributes.get(&index).cloned().unwrap_or_default();
                    physical.push((values, attrs));
                }
            }
        }
        let mut widths = Vec::new();
        widths
            .try_reserve(count)
            .map_err(|error| ConsoleError::from_source(ConsoleTableError::Allocation(error)))?;
        for column in 0..count {
            let mut maximum: Option<i64> = None;
            for (row, attributes) in &physical {
                let mut transformed;
                let values = if attributes.is_empty() {
                    row
                } else {
                    transformed = row.clone();
                    for (index, attribute) in attributes {
                        if let Some(value) = row.get(*index) {
                            let visible = super::formatting::unformatted_text(
                                &mut super::formatting::Formatter::from_handle(
                                    self.output.get_formatter(),
                                ),
                                value,
                            )
                            .map_err(ConsoleError::from_source)?;
                            let length = width(&visible);
                            if length > 0 {
                                let chars: Vec<_> = visible.chars().collect();
                                if attribute.colspan <= 0 {
                                    return Err(ConsoleError::from_source(
                                        ConsoleTableError::InvalidSpanLayout,
                                    ));
                                }
                                let divisor = usize::try_from(attribute.colspan)
                                    .map_err(ConsoleError::from_source)?;
                                let split = length.div_ceil(divisor);
                                for (position, chunk) in chars.chunks(split).enumerate() {
                                    let target = index.checked_add(position).ok_or_else(|| {
                                        ConsoleError::from_source(
                                            ConsoleTableError::InvalidColumnWidth(i64::MAX),
                                        )
                                    })?;
                                    if transformed.len() <= target {
                                        transformed.resize(target + 1, String::new());
                                    }
                                    transformed[target] = chunk.iter().collect();
                                }
                            }
                        }
                    }
                    &transformed
                };
                let length = if let Some(value) = values.get(column) {
                    let plain = super::formatting::unformatted_text(
                        &mut super::formatting::Formatter::from_handle(self.output.get_formatter()),
                        value,
                    )
                    .map_err(ConsoleError::from_source)?;
                    i64::try_from(width(&plain)).map_err(ConsoleError::from_source)?
                } else {
                    0
                };
                let mut length = length.max(self.column_widths.get(&column).copied().unwrap_or(0));
                if let Some(max) = self.column_max_widths.get(&column) {
                    length = length.min(*max);
                }
                maximum = Some(maximum.map_or(length, |old| old.max(length)));
            }
            let padding = i64::try_from(width(&self.style.get_cell_row_content_format()))
                .map_err(ConsoleError::from_source)?
                - 2;
            let maximum = maximum.unwrap_or(0);
            widths.push(maximum.checked_add(padding).ok_or_else(|| {
                ConsoleError::from_source(ConsoleTableError::InvalidColumnWidth(maximum))
            })?);
        }
        Ok(widths)
    }

    fn render_cell(
        &mut self,
        cell: &str,
        target: i64,
        column: Option<usize>,
        cell_format: &str,
        attributes: Option<&Attributes>,
    ) -> Result<String, ConsoleError> {
        if attributes.is_some_and(|attributes| attributes.separator) {
            let style = column
                .map(|column| self.get_column_style(column))
                .unwrap_or(&self.style)
                .snapshot();
            let target = usize::try_from(target).map_err(|_| {
                ConsoleError::from_source(ConsoleTableError::InvalidColumnWidth(target))
            })?;
            return template(&style.border_format, &style.borders[2].repeat(target));
        }
        let visible = super::formatting::unformatted_text(
            &mut super::formatting::Formatter::from_handle(self.output.get_formatter()),
            cell,
        )
        .map_err(ConsoleError::from_source)?;
        let style = column
            .map(|column| self.get_column_style(column))
            .unwrap_or(&self.style)
            .snapshot();
        let mut content = template(&style.cell_row_content_format, cell)?;
        let mut target = target + i64::try_from(cell.len()).map_err(ConsoleError::from_source)?
            - i64::try_from(width(&visible)).map_err(ConsoleError::from_source)?
            - i64::try_from(cell.bytes().filter(|byte| *byte == 0).count())
                .map_err(ConsoleError::from_source)?;
        let mut format = cell_format.to_owned();
        let mut pad_type = style.pad_type;
        if let Some(cell_style) = attributes.and_then(|attributes| attributes.style.as_ref()) {
            if !tag_styled(cell) {
                format = cell_style.format()?;
                if content.contains("</>") {
                    content = content.replace("</>", "");
                    target -= 3;
                }
                if content.contains("<fg=default;bg=default>") {
                    content = content.replace("<fg=default;bg=default>", "");
                    target -= i64::try_from("<fg=default;bg=default>".len())
                        .map_err(ConsoleError::from_source)?;
                }
            }
            pad_type = cell_style.get_pad_by_align()?;
        }
        let missing = usize::try_from(
            target
                .saturating_sub(i64::try_from(content.len()).map_err(ConsoleError::from_source)?)
                .max(0),
        )
        .map_err(ConsoleError::from_source)?;
        let (left, right) = match pad_type {
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
        template(&format, &content)
    }
}
fn lines(value: &str) -> impl Iterator<Item = &str> {
    let separator = if value.contains("\r\n") { "\r\n" } else { "\n" };
    value.split(separator)
}

fn cell_lines(value: &str, unsplit: bool) -> impl Iterator<Item = &str> {
    std::iter::once(value)
        .take(usize::from(unsplit))
        .chain(lines(value).take(if unsplit { 0 } else { usize::MAX }))
}

impl super::CommandIo<'_> {
    pub fn table<H, HT, R, RT>(&mut self, headers: H, rows: R) -> Result<(), ConsoleError>
    where
        H: IntoIterator<Item = HT>,
        HT: Into<TableCell>,
        R: IntoIterator<Item = RT>,
        RT: Into<TableRow>,
    {
        let mut table = Table::new(self.get_output())?;
        table.set_headers(headers).set_rows(rows).render()
    }
    pub fn table_named<H, HT, R, RT>(
        &mut self,
        headers: H,
        rows: R,
        style: &str,
    ) -> Result<(), ConsoleError>
    where
        H: IntoIterator<Item = HT>,
        HT: Into<TableCell>,
        R: IntoIterator<Item = RT>,
        RT: Into<TableRow>,
    {
        self.table_with_named_columns(headers, rows, style, std::iter::empty())
    }
    pub fn table_with_named_columns<'s, H, HT, R, RT, S>(
        &mut self,
        headers: H,
        rows: R,
        style: &str,
        column_styles: S,
    ) -> Result<(), ConsoleError>
    where
        H: IntoIterator<Item = HT>,
        HT: Into<TableCell>,
        R: IntoIterator<Item = RT>,
        RT: Into<TableRow>,
        S: IntoIterator<Item = (usize, &'s str)>,
    {
        let mut table = Table::new(self.get_output())?;
        table
            .set_headers(headers)
            .set_rows(rows)
            .set_style_named(style)?;
        for (column, name) in column_styles {
            table.set_column_style_named(column, name)?;
        }
        table.render()
    }
    pub fn table_with<H, HT, R, RT>(
        &mut self,
        headers: H,
        rows: R,
        style: TableStyle,
    ) -> Result<(), ConsoleError>
    where
        H: IntoIterator<Item = HT>,
        HT: Into<TableCell>,
        R: IntoIterator<Item = RT>,
        RT: Into<TableRow>,
    {
        self.table_with_columns(headers, rows, style, std::iter::empty())
    }
    pub fn table_with_columns<H, HT, R, RT, S>(
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
        RT: Into<TableRow>,
        S: IntoIterator<Item = (usize, TableStyle)>,
    {
        let mut table = Table::new(self.get_output())?;
        table.set_headers(headers).set_rows(rows).set_style(style);
        for (column, style) in column_styles {
            table.set_column_style(column, style);
        }
        table.render()
    }
}

fn template(format: &str, value: &str) -> Result<String, ConsoleError> {
    super::formatting::format_template(format, &serde_json::Value::String(value.to_owned()))
        .map_err(|error| {
            ConsoleError::from_source(ConsoleTableError::InvalidFormat(ConsoleError::from_source(
                error,
            )))
        })
}
fn padding(value: &str, count: usize) -> Result<String, ConsoleError> {
    let mut result = Vec::with_capacity(count);
    result.extend(value.as_bytes().iter().copied().cycle().take(count));
    String::from_utf8(result).map_err(|error| {
        ConsoleError::from_source(ConsoleTableError::InvalidPaddingEncoding(error))
    })
}

fn tag_styled(cell: &str) -> bool {
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"^<([A-Za-z0-9_]+|([A-Za-z0-9_]+=[A-Za-z0-9_,]+;?)*)>.+</([A-Za-z0-9_]+|([A-Za-z0-9_]+=[A-Za-z0-9_]+;?)*)?>(?:\n)?\z").expect("固定单元格标签模式有效")
    });
    PATTERN.is_match(cell)
}

fn row_parts(
    row: &[String],
    unsplit: Option<&std::collections::BTreeSet<usize>>,
) -> Vec<Vec<String>> {
    row.iter()
        .enumerate()
        .map(|(column, cell)| {
            let mut parts: Vec<_> = cell_lines(
                cell,
                unsplit.is_some_and(|columns| columns.contains(&column)),
            )
            .map(str::to_owned)
            .collect();
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
        .collect()
}
