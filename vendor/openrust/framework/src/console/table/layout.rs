use super::{ConsoleError, ConsoleTableError, Table, TableStyle, template, width};
use std::collections::{BTreeMap, BTreeSet};
type TextRows = Vec<Vec<String>>;
type ColumnMasks = BTreeMap<usize, BTreeSet<usize>>;
pub(super) struct PreparedLayout {
    pub(super) headers: TextRows,
    pub(super) rows: TextRows,
    pub(super) unsplit_headers: ColumnMasks,
    pub(super) rules: BTreeSet<usize>,
    pub(super) separators: BTreeSet<usize>,
    pub(super) unsplit_rows: ColumnMasks,
}
impl Table<'_, '_> {
    pub fn get_style_mut(&mut self) -> &mut TableStyle {
        &mut self.style
    }
    pub fn get_column_style_mut(&mut self, column: usize) -> &mut TableStyle {
        self.column_styles
            .get_mut(&column)
            .unwrap_or(&mut self.style)
    }
    pub fn set_column_width(&mut self, column: usize, width: i64) -> &mut Self {
        self.column_widths.insert(column, width);
        self
    }
    pub fn set_column_widths(&mut self, widths: impl IntoIterator<Item = i64>) -> &mut Self {
        self.column_widths = widths.into_iter().enumerate().collect();
        self
    }
    pub fn set_column_widths_with_keys(
        &mut self,
        widths: impl IntoIterator<Item = (usize, i64)>,
    ) -> &mut Self {
        self.column_widths = widths.into_iter().collect();
        self
    }
    pub fn set_column_max_width(
        &mut self,
        column: usize,
        width: i64,
    ) -> Result<&mut Self, ConsoleError> {
        if self.output.get_formatter().as_wrappable().is_none() {
            return Err(ConsoleError::from_source(
                ConsoleTableError::UnsupportedWrappingFormatter,
            ));
        }
        self.column_max_widths.insert(column, width);
        Ok(self)
    }
    pub fn set_header_title(&mut self, title: Option<&str>) -> &mut Self {
        self.header_title = title.map(str::to_owned);
        self
    }
    pub fn set_footer_title(&mut self, title: Option<&str>) -> &mut Self {
        self.footer_title = title.map(str::to_owned);
        self
    }
    pub(super) fn prepare_layout(&mut self) -> Result<PreparedLayout, ConsoleError> {
        let oriented = self.oriented_rows()?;
        let mut prepared = PreparedLayout {
            headers: oriented.headers,
            rows: oriented.rows,
            rules: oriented.rules,
            separators: oriented.separators,
            unsplit_headers: ColumnMasks::new(),
            unsplit_rows: ColumnMasks::new(),
        };
        let formatter = self.output.get_formatter();
        for (is_body, rows, masks) in [
            (false, &mut prepared.headers, &mut prepared.unsplit_headers),
            (true, &mut prepared.rows, &mut prepared.unsplit_rows),
        ] {
            for (row_index, row) in rows.iter_mut().enumerate() {
                for (column, cell) in row.iter_mut().enumerate() {
                    if let Some(maximum) = self
                        .column_max_widths
                        .get(&column)
                        .copied()
                        .filter(|width| *width != 0)
                    {
                        let visible = super::super::formatting::unformatted_text(
                            &mut self.output.get_output().formatter,
                            cell,
                        )
                        .map_err(ConsoleError::from_source)?;
                        if i64::try_from(width(&visible)).map_err(ConsoleError::from_source)?
                            > maximum
                        {
                            let wrapper = formatter.as_wrappable().ok_or_else(|| {
                                ConsoleError::from_source(
                                    ConsoleTableError::UnsupportedWrappingFormatter,
                                )
                            })?;
                            let wrapped = wrapper
                                .format_and_wrap(Some(cell), maximum)
                                .map_err(ConsoleError::from_source)?;
                            // 只有带换行的结果进入拆行；其他结果保留原单元格与原始换行。
                            if wrapped.contains('\n') {
                                *cell = wrapped;
                                if is_body {
                                    prepared.rules.remove(&row_index);
                                }
                            } else if cell.contains('\n') {
                                masks.entry(row_index).or_default().insert(column);
                            }
                        }
                    }
                }
            }
        }
        Ok(prepared)
    }
    pub(super) fn with_title(
        &mut self,
        markup: &str,
        title: &str,
        format: &str,
    ) -> Result<String, ConsoleError> {
        let mut formatted = template(format, title)?;
        let visible = super::super::formatting::unformatted_text(
            &mut self.output.get_output().formatter,
            &formatted,
        )
        .map_err(ConsoleError::from_source)?;
        let mut title_length = i64::try_from(width(&visible)).map_err(ConsoleError::from_source)?;
        let markup_length = i64::try_from(width(markup)).map_err(ConsoleError::from_source)?;
        let limit = markup_length - 4;
        if title_length > limit {
            title_length = limit;
            let empty = template(format, "")?;
            let visible = super::super::formatting::unformatted_text(
                &mut self.output.get_output().formatter,
                &empty,
            )
            .map_err(ConsoleError::from_source)?;
            let format_length =
                i64::try_from(width(&visible)).map_err(ConsoleError::from_source)?;
            let prefix = grapheme_prefix(title, limit - format_length - 3)?;
            formatted = template(format, &format!("{prefix}..."))?;
        }
        let start = (markup_length - title_length) / 2;
        let start = isize::try_from(start).map_err(ConsoleError::from_source)?;
        let end =
            start.saturating_add(isize::try_from(title_length).map_err(ConsoleError::from_source)?);
        Ok(format!(
            "{}{}{}",
            crate::support::Str::substr(markup, 0, Some(start)),
            formatted,
            crate::support::Str::substr(markup, end, None)
        ))
    }
}
fn grapheme_prefix(text: &str, length: i64) -> Result<String, ConsoleError> {
    let slices =
        super::super::formatting::unicode::Slices::new(text).map_err(ConsoleError::from_source)?;
    let count =
        super::super::formatting::grapheme_length(text).map_err(ConsoleError::from_source)?;
    let end = if length < 0 {
        count.saturating_sub(usize::try_from(length.unsigned_abs()).unwrap_or(usize::MAX))
    } else {
        usize::try_from(length).unwrap_or(usize::MAX).min(count)
    };
    Ok(slices.slice(0, Some(end)).to_owned())
}
