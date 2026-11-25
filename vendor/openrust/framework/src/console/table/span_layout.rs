use super::layout::PreparedLayout;
use super::rows::{Orientation, OrientedRows};
use super::{ArrayKey, Attributes, ConsoleError, ConsoleTableError, Table, width};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone)]
struct Cell {
    key: usize,
    text: String,
    attributes: Option<Attributes>,
}
#[derive(Clone)]
enum Kind {
    Data(Vec<Cell>),
    Separator,
    Divider,
}
#[derive(Clone)]
struct Row {
    key: ArrayKey,
    kind: Kind,
}
fn error() -> ConsoleError {
    ConsoleError::from_source(ConsoleTableError::InvalidSpanLayout)
}
fn count(cells: &[Cell]) -> Result<i64, ConsoleError> {
    let mut count = i64::try_from(cells.len()).map_err(ConsoleError::from_source)?;
    for cell in cells {
        if let Some(attributes) = &cell.attributes {
            count = count
                .checked_add(attributes.colspan.checked_sub(1).ok_or_else(error)?)
                .ok_or_else(error)?;
        }
    }
    Ok(count)
}
fn cells(text: Vec<String>, attributes: Option<&BTreeMap<usize, Attributes>>) -> Vec<Cell> {
    text.into_iter()
        .enumerate()
        .map(|(key, text)| Cell {
            key,
            text,
            attributes: attributes.and_then(|a| a.get(&key)).cloned(),
        })
        .collect()
}
fn typed(text: String, colspan: i64, style: Option<super::TableCellStyle>) -> Cell {
    Cell {
        key: 0,
        text,
        attributes: Some(Attributes {
            colspan,
            rowspan: 1,
            style,
            separator: false,
        }),
    }
}
fn put(cells: &mut Vec<Cell>, key: usize, mut cell: Cell) {
    cell.key = key;
    if let Some(index) = cells.iter().position(|old| old.key == key) {
        cells[index] = cell;
    } else {
        cells.push(cell);
    }
}
fn copy_row(kind: &Kind) -> Result<Vec<Cell>, ConsoleError> {
    let Kind::Data(cells) = kind else {
        return Err(error());
    };
    Ok(cells
        .iter()
        .map(|cell| Cell {
            key: cell.key,
            text: String::new(),
            attributes: cell.attributes.as_ref().map(|a| Attributes {
                colspan: a.colspan,
                rowspan: 1,
                style: None,
                separator: false,
            }),
        })
        .collect())
}
fn numeric(rows: &[Row], key: usize) -> Option<usize> {
    rows.iter()
        .position(|row| row.key == ArrayKey::Integer(i64::try_from(key).unwrap_or(i64::MAX)))
}
fn renumber(rows: &mut [Row]) {
    let mut next = 0_i64;
    for row in rows {
        if matches!(row.key, ArrayKey::Integer(_)) {
            row.key = ArrayKey::Integer(next);
            next += 1;
        }
    }
}
fn fill_cells(cells: Vec<Cell>) -> Result<Vec<Cell>, ConsoleError> {
    let mut result = Vec::new();
    for mut cell in cells {
        let span = cell.attributes.as_ref().map_or(1, |a| a.colspan).max(1);
        let additional = usize::try_from(span).map_err(ConsoleError::from_source)?;
        result
            .try_reserve(additional)
            .map_err(|error| ConsoleError::from_source(ConsoleTableError::Allocation(error)))?;
        cell.key = result.len();
        result.push(cell);
        for _ in 1..additional {
            result.push(Cell {
                key: result.len(),
                text: String::new(),
                attributes: None,
            });
        }
    }
    Ok(result)
}
impl Table<'_, '_> {
    pub(super) fn span_layout(
        &mut self,
        oriented: OrientedRows,
    ) -> Result<PreparedLayout, ConsoleError> {
        let mut rows = Vec::new();
        let mut next = 0_i64;
        for (index, row) in oriented.headers.into_iter().enumerate() {
            rows.push(Row {
                key: ArrayKey::Integer(next),
                kind: Kind::Data(cells(row, oriented.header_attributes.get(&index))),
            });
            next += 1;
        }
        if self.orientation == Orientation::Default {
            rows.push(Row {
                key: ArrayKey::Integer(next),
                kind: Kind::Divider,
            });
            next += 1;
        }
        for (index, row) in oriented.rows.into_iter().enumerate() {
            let key = if self.orientation == Orientation::Default {
                match &self.row_keys[index] {
                    ArrayKey::String(key) => ArrayKey::String(key.clone()),
                    _ => {
                        let key = ArrayKey::Integer(next);
                        next += 1;
                        key
                    }
                }
            } else {
                let key = ArrayKey::Integer(next);
                next += 1;
                key
            };
            let kind = if oriented.separators.contains(&index) {
                Kind::Separator
            } else {
                Kind::Data(cells(row, oriented.row_attributes.get(&index)))
            };
            rows.push(Row { key, kind });
        }
        let mut columns = 0_i64;
        for row in &rows {
            if let Kind::Data(cells) = &row.kind {
                columns = columns.max(count(cells)?);
            }
        }
        let columns = usize::try_from(columns).map_err(ConsoleError::from_source)?;
        let mut capacity: Vec<i64> = Vec::new();
        capacity
            .try_reserve(columns)
            .map_err(|error| ConsoleError::from_source(ConsoleTableError::Allocation(error)))?;
        drop(capacity);
        let mut unmerged: BTreeMap<usize, BTreeMap<usize, Vec<Cell>>> = BTreeMap::new();
        let mut line = 0;
        while line < rows.len() {
            let Some(index) = numeric(&rows, line) else {
                line += 1;
                continue;
            };
            self.fill_span_rows(&mut rows, index, line, columns)?;
            let Some(index) = numeric(&rows, line) else {
                return Err(error());
            };
            let Kind::Data(original) = &rows[index].kind else {
                line += 1;
                continue;
            };
            let original = original.clone();
            for cell in original {
                let span = cell.attributes.as_ref().map_or(1, |a| a.colspan);
                let end = if span > 0 {
                    cell.key
                        .checked_add(usize::try_from(span).map_err(ConsoleError::from_source)?)
                        .ok_or_else(error)?
                } else {
                    cell.key
                };
                let mut wrapped_width = 0_i64;
                let mut applied = Vec::new();
                let border = i64::try_from(width(&self.column_separator(true)?))
                    .map_err(ConsoleError::from_source)?
                    + i64::try_from(width(&self.style.get_cell_row_content_format()))
                        .map_err(ConsoleError::from_source)?
                    - 2;
                for column in cell.key..end {
                    if let Some(max) = self.column_max_widths.get(&column) {
                        wrapped_width = wrapped_width.checked_add(*max).ok_or_else(error)?;
                        applied.push((column, true));
                    } else if span > 1
                        && let Some(min) =
                            self.column_widths.get(&column).filter(|width| **width > 0)
                    {
                        wrapped_width = wrapped_width.checked_add(*min).ok_or_else(error)?;
                        applied.push((column, false));
                    }
                }
                if applied.len() == 1 && span > 1 {
                    wrapped_width = wrapped_width.checked_mul(span).ok_or_else(error)?;
                } else if applied.len() > 1 {
                    wrapped_width = wrapped_width
                        .checked_add(
                            i64::try_from(applied.len() - 1)
                                .map_err(ConsoleError::from_source)?
                                .checked_mul(border)
                                .ok_or_else(error)?,
                        )
                        .ok_or_else(error)?;
                }
                let visible = super::super::formatting::unformatted_text(
                    &mut self.output.get_output().formatter,
                    &cell.text,
                )
                .map_err(ConsoleError::from_source)?;
                let cell_width =
                    i64::try_from(width(&visible)).map_err(ConsoleError::from_source)?;
                let mut text = cell.text.clone();
                if wrapped_width != 0 && cell_width > wrapped_width {
                    let formatter = self.output.get_formatter();
                    let wrapper = formatter.as_wrappable().ok_or_else(|| {
                        ConsoleError::from_source(ConsoleTableError::UnsupportedWrappingFormatter)
                    })?;
                    text = wrapper
                        .format_and_wrap(Some(&text), wrapped_width)
                        .map_err(ConsoleError::from_source)?;
                }
                if span > 1 && wrapped_width > 0 {
                    let mut remaining = cell_width.min(wrapped_width);
                    let mut processed = BTreeSet::new();
                    for (column, max) in &applied {
                        if *max && remaining >= self.column_max_widths[column] {
                            let min = self.column_max_widths[column];
                            self.column_widths.insert(*column, min);
                            processed.insert(*column);
                            remaining = remaining
                                .checked_sub(min.checked_add(border).ok_or_else(error)?)
                                .ok_or_else(error)?;
                        }
                    }
                    for column in cell.key..end {
                        if !processed.contains(&column) {
                            self.column_widths
                                .insert(column, remaining.checked_add(border).ok_or_else(error)?);
                        }
                    }
                }
                if !text.contains('\n') {
                    continue;
                }
                let separator = if text.contains("\r\n") { "\r\n" } else { "\n" };
                let escaped = text
                    .split(separator)
                    .map(super::super::formatting::Formatter::escape_trailing_backslash)
                    .collect::<Vec<_>>()
                    .join(separator);
                let replaced =
                    escaped.replace(separator, &format!("<fg=default;bg=default></>{separator}"));
                for (offset, text) in replaced.split(separator).enumerate() {
                    let value = if span > 1 {
                        typed(text.to_owned(), span, None)
                    } else {
                        Cell {
                            key: cell.key,
                            text: text.to_owned(),
                            attributes: None,
                        }
                    };
                    if offset == 0 {
                        let Kind::Data(data) = &mut rows[index].kind else {
                            return Err(error());
                        };
                        put(data, cell.key, value);
                    } else {
                        if !unmerged
                            .get(&line)
                            .is_some_and(|lines| lines.contains_key(&offset))
                        {
                            let copy = copy_row(&rows[index].kind)?;
                            unmerged.entry(line).or_default().insert(offset, copy);
                        }
                        put(
                            unmerged.get_mut(&line).unwrap().get_mut(&offset).unwrap(),
                            cell.key,
                            value,
                        );
                    }
                }
            }
            line += 1;
        }
        let mut result = PreparedLayout {
            headers: Vec::new(),
            rows: Vec::new(),
            header_attributes: super::AttributeRows::new(),
            row_attributes: super::AttributeRows::new(),
            unsplit_headers: BTreeMap::new(),
            unsplit_rows: BTreeMap::new(),
            separators: BTreeSet::new(),
            number_of_columns: columns,
            header_continuations: BTreeSet::new(),
        };
        let mut header = self.orientation == Orientation::Default;
        for row in rows {
            match row.kind {
                Kind::Divider => {
                    header = false;
                }
                Kind::Separator => {
                    let index = result.rows.len();
                    result.rows.push(Vec::new());
                    result.separators.insert(index);
                }
                Kind::Data(data) => {
                    let mut group = vec![fill_cells(data)?];
                    if let ArrayKey::Integer(key) = row.key
                        && let Ok(key) = usize::try_from(key)
                        && let Some(extra) = unmerged.remove(&key)
                    {
                        for cells in extra.into_values() {
                            group.push(fill_cells(cells)?);
                        }
                    }
                    for (continuation, data) in group.into_iter().enumerate() {
                        let (target, attributes, masks) = if header {
                            (
                                &mut result.headers,
                                &mut result.header_attributes,
                                &mut result.unsplit_headers,
                            )
                        } else {
                            (
                                &mut result.rows,
                                &mut result.row_attributes,
                                &mut result.unsplit_rows,
                            )
                        };
                        let index = target.len();
                        let mut values = Vec::new();
                        for cell in data {
                            let column = values.len();
                            if cell.text.contains('\n') {
                                masks.entry(index).or_default().insert(column);
                            }
                            if let Some(attribute) = cell.attributes {
                                attributes
                                    .entry(index)
                                    .or_default()
                                    .insert(column, attribute);
                            }
                            values.push(cell.text);
                        }
                        target.push(values);
                        if header && continuation > 0 {
                            result.header_continuations.insert(index);
                        }
                    }
                }
            }
        }
        Ok(result)
    }
    fn fill_span_rows(
        &self,
        rows: &mut Vec<Row>,
        index: usize,
        line: usize,
        columns: usize,
    ) -> Result<(), ConsoleError> {
        let Kind::Data(original) = &rows[index].kind else {
            return Ok(());
        };
        let original = original.clone();
        let mut unmerged: BTreeMap<usize, BTreeMap<usize, Cell>> = BTreeMap::new();
        for cell in original {
            let Some(attributes) = &cell.attributes else {
                continue;
            };
            if attributes.rowspan <= 1 {
                continue;
            }
            let mut length =
                usize::try_from(attributes.rowspan - 1).map_err(ConsoleError::from_source)?;
            let mut parts = Vec::new();
            if cell.text.contains('\n') {
                let separator = if cell.text.contains("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let replaced = cell
                    .text
                    .replace(separator, &format!("<fg=default;bg=default>{separator}</>"));
                parts = replaced.split(separator).map(str::to_owned).collect();
                length = length.max(parts.len() - 1);
                let Kind::Data(cells) = &mut rows[index].kind else {
                    return Err(error());
                };
                put(
                    cells,
                    cell.key,
                    typed(
                        parts[0].clone(),
                        attributes.colspan,
                        attributes.style.clone(),
                    ),
                );
            }
            rows.try_reserve(length)
                .map_err(|error| ConsoleError::from_source(ConsoleTableError::Allocation(error)))?;
            for offset in 1..=length {
                let key = line.checked_add(offset).ok_or_else(error)?;
                unmerged.entry(key).or_default().insert(
                    cell.key,
                    typed(
                        parts.get(offset).cloned().unwrap_or_default(),
                        attributes.colspan,
                        attributes.style.clone(),
                    ),
                );
            }
        }
        for (key, cells) in unmerged {
            let extra: Vec<_> = cells.values().cloned().collect();
            let merge = if let Some(index) = numeric(rows, key) {
                matches!(&rows[index].kind,Kind::Data(data) if count(data)?.checked_add(count(&extra)?).ok_or_else(error)?<=i64::try_from(columns).map_err(ConsoleError::from_source)?)
            } else {
                false
            };
            if merge {
                let index = numeric(rows, key).ok_or_else(error)?;
                let Kind::Data(data) = &mut rows[index].kind else {
                    return Err(error());
                };
                for (column, cell) in cells {
                    let at = column.min(data.len());
                    data.insert(at, cell);
                    for (key, cell) in data.iter_mut().enumerate() {
                        cell.key = key;
                    }
                }
            } else {
                let previous =
                    numeric(rows, key.checked_sub(1).ok_or_else(error)?).ok_or_else(error)?;
                let mut copy = copy_row(&rows[previous].kind)?;
                for (column, cell) in cells {
                    put(&mut copy, column, cell);
                }
                let at = key.min(rows.len());
                // 在所有者前插入会重复处理同一跨度，拒绝这类无法收敛的键布局。
                if at <= index {
                    return Err(error());
                }
                rows.insert(
                    at,
                    Row {
                        key: ArrayKey::Integer(0),
                        kind: Kind::Data(copy),
                    },
                );
                renumber(rows);
            }
        }
        Ok(())
    }
}
