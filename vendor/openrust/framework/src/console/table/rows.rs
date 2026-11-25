use super::{ArrayKey, ConsoleError, Table, TableCell, TableRow};
use std::collections::BTreeSet;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Orientation {
    #[default]
    Default,
    Horizontal,
    Vertical,
}
pub(super) struct OrientedRows {
    pub(super) headers: Vec<Vec<String>>,
    pub(super) rows: Vec<Vec<String>>,
    pub(super) header_attributes: super::AttributeRows,
    pub(super) row_attributes: super::AttributeRows,
    pub(super) separators: BTreeSet<usize>,
}
impl Table<'_, '_> {
    pub fn add_rows<I, R>(&mut self, rows: I) -> Result<&mut Self, ConsoleError>
    where
        I: IntoIterator<Item = R>,
        R: Into<TableRow>,
    {
        for row in rows {
            self.add_row(row)?;
        }
        Ok(self)
    }
    pub fn set_row<I, T>(&mut self, key: impl Into<ArrayKey>, row: I) -> &mut Self
    where
        I: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        self.insert_row(key.into().normalized(), TableRow::from(row));
        self
    }
    pub(super) fn insert_row(&mut self, key: ArrayKey, row: TableRow) {
        let index = self
            .row_keys
            .iter()
            .position(|old| *old == key)
            .unwrap_or(self.rows.len());
        let row = match row {
            TableRow::Cells(cells) => {
                self.separators.remove(&index);
                cells
            }
            TableRow::Separator(_) => {
                self.separators.insert(index);
                Vec::new()
            }
        };
        if index < self.rows.len() {
            self.rows[index] = row;
        } else {
            if let ArrayKey::Integer(index) = &key {
                self.max_integer_key =
                    Some(self.max_integer_key.map_or(*index, |old| old.max(*index)));
            }
            self.row_keys.push(key);
            self.rows.push(row);
        }
    }
    pub fn set_headers_rows<I, R, T>(&mut self, rows: I) -> &mut Self
    where
        I: IntoIterator<Item = R>,
        R: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        self.headers = rows
            .into_iter()
            .map(|row| row.into_iter().map(Into::into).collect())
            .collect();
        self
    }
    pub fn set_horizontal(&mut self, value: bool) -> &mut Self {
        self.orientation = if value {
            Orientation::Horizontal
        } else {
            Orientation::Default
        };
        self
    }
    pub fn set_vertical(&mut self, value: bool) -> &mut Self {
        self.orientation = if value {
            Orientation::Vertical
        } else {
            Orientation::Default
        };
        self
    }
    pub(super) fn oriented_rows(&mut self) -> Result<OrientedRows, ConsoleError> {
        let mut result = OrientedRows {
            headers: Vec::new(),
            rows: Vec::new(),
            header_attributes: super::AttributeRows::new(),
            row_attributes: super::AttributeRows::new(),
            separators: BTreeSet::new(),
        };
        match self.orientation {
            Orientation::Default => {
                result.headers = project_cells(&self.headers, &mut result.header_attributes);
                result.rows = project_cells(&self.rows, &mut result.row_attributes);
                result.separators = self.separators.clone();
            }
            Orientation::Horizontal => {
                for (column, header) in self.headers.first().into_iter().flatten().enumerate() {
                    let row_index = result.rows.len();
                    let mut row = vec![header.to_string()];
                    if let Some(attributes) = header.attributes() {
                        result
                            .row_attributes
                            .entry(row_index)
                            .or_default()
                            .insert(0, attributes);
                    }
                    for (_, source) in self
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !self.separators.contains(index))
                    {
                        let index = row.len();
                        if let Some(cell) = source.get(column) {
                            if let Some(attributes) = cell.attributes() {
                                result
                                    .row_attributes
                                    .entry(row_index)
                                    .or_default()
                                    .insert(index, attributes);
                            }
                            row.push(cell.to_string());
                        } else if header.get_colspan() < 2 {
                            row.push(String::new());
                        }
                    }
                    result.rows.push(row);
                }
            }
            Orientation::Vertical => {
                let headers = self.headers.first().cloned().unwrap_or_default();
                let mut maximum = 0;
                for header in &headers {
                    maximum = maximum.max(self.visible_width(header.as_str())?);
                }
                let sources: Vec<_> = self
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !self.separators.contains(index))
                    .map(|(_, row)| row.clone())
                    .collect();
                for source in sources {
                    let contains_colspan = source.iter().any(|cell| cell.get_colspan() >= 2);
                    if !result.rows.is_empty() {
                        result
                            .row_attributes
                            .entry(result.rows.len())
                            .or_default()
                            .insert(
                                0,
                                super::Attributes {
                                    style: None,
                                    separator: true,
                                    colspan: 1,
                                    rowspan: 1,
                                },
                            );
                        result.rows.push(vec![String::new()]);
                    }
                    for column in 0..headers.len().max(source.len()) {
                        let cell = source
                            .get(column)
                            .map(TableCell::as_str)
                            .unwrap_or_default();
                        let separator = if cell.contains("\r\n") { "\r\n" } else { "\n" };
                        for (index, part) in cell.split(separator).enumerate() {
                            if !headers.is_empty() && !contains_colspan {
                                if index == 0 {
                                    let header = headers
                                        .get(column)
                                        .map(TableCell::as_str)
                                        .unwrap_or_default();
                                    let length = self.visible_width(header)?;
                                    result.rows.push(vec![format!(
                                        "<comment>{}{header}</>: {part}",
                                        " ".repeat(maximum.saturating_sub(length))
                                    )]);
                                } else {
                                    result
                                        .rows
                                        .push(vec![format!("{}  {part}", " ".repeat(maximum))]);
                                }
                            } else if !cell.is_empty() {
                                result.rows.push(vec![part.to_owned()]);
                            }
                        }
                    }
                }
            }
        }
        Ok(result)
    }
    fn visible_width(&mut self, text: &str) -> Result<usize, ConsoleError> {
        let plain = super::super::formatting::unformatted_text(
            &mut self.output.get_output().formatter,
            text,
        )
        .map_err(ConsoleError::from_source)?;
        Ok(super::width(&plain))
    }
}

fn project_cells(
    rows: &[Vec<TableCell>],
    attributes: &mut super::AttributeRows,
) -> Vec<Vec<String>> {
    rows.iter()
        .enumerate()
        .map(|(row_index, row)| {
            row.iter()
                .enumerate()
                .map(|(column, cell)| {
                    if let Some(attribute) = cell.attributes() {
                        attributes
                            .entry(row_index)
                            .or_default()
                            .insert(column, attribute);
                    }
                    cell.to_string()
                })
                .collect()
        })
        .collect()
}
