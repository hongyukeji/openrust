use super::{ArrayKey, ConsoleError, Table, TableCell, template};
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
    pub(super) rules: BTreeSet<usize>,
}
impl Table<'_, '_> {
    pub fn add_rows<I, R, T>(&mut self, rows: I) -> Result<&mut Self, ConsoleError>
    where
        I: IntoIterator<Item = R>,
        R: IntoIterator<Item = T>,
        T: Into<TableCell>,
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
        let key = key.into().normalized();
        let row = row
            .into_iter()
            .map(|cell| cell.into().to_string())
            .collect();
        if let Some(index) = self.row_keys.iter().position(|old| *old == key) {
            self.rows[index] = row;
        } else {
            if let ArrayKey::Integer(index) = &key {
                self.max_integer_key =
                    Some(self.max_integer_key.map_or(*index, |old| old.max(*index)));
            }
            self.row_keys.push(key);
            self.rows.push(row);
        }
        self
    }
    pub fn set_headers_rows<I, R, T>(&mut self, rows: I) -> &mut Self
    where
        I: IntoIterator<Item = R>,
        R: IntoIterator<Item = T>,
        T: Into<TableCell>,
    {
        self.headers = rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|cell| cell.into().to_string())
                    .collect()
            })
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
            rules: BTreeSet::new(),
        };
        match self.orientation {
            Orientation::Default => {
                result.headers = self.headers.clone();
                result.rows = self.rows.clone();
            }
            Orientation::Horizontal => {
                for (column, header) in self.headers.first().into_iter().flatten().enumerate() {
                    let mut row = vec![header.clone()];
                    for source in &self.rows {
                        row.push(source.get(column).cloned().unwrap_or_default());
                    }
                    result.rows.push(row);
                }
            }
            Orientation::Vertical => {
                let headers = self.headers.first().cloned().unwrap_or_default();
                let mut maximum = 0;
                for header in &headers {
                    maximum = maximum.max(self.visible_width(header)?);
                }
                for source in self.rows.clone() {
                    if !result.rows.is_empty() {
                        result.rules.insert(result.rows.len());
                        result.rows.push(vec![String::new()]);
                    }
                    for column in 0..headers.len().max(source.len()) {
                        let cell = source.get(column).map(String::as_str).unwrap_or_default();
                        let separator = if cell.contains("\r\n") { "\r\n" } else { "\n" };
                        for (index, part) in cell.split(separator).enumerate() {
                            if !headers.is_empty() {
                                if index == 0 {
                                    let header =
                                        headers.get(column).map(String::as_str).unwrap_or_default();
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
    pub(super) fn rule_row(&mut self, widths: &[i64]) -> Result<(), ConsoleError> {
        let width = widths.first().copied().unwrap_or(0);
        let width = usize::try_from(width).map_err(ConsoleError::from_source)?;
        let style = self.get_column_style(0);
        let cell = template(&style.border_format, &style.borders[2].repeat(width))?;
        let border = template(&self.style.border_format, &self.style.borders[1])?;
        self.output.writeln(
            format!("{border}{cell}{border}").as_str(),
            super::OutputVerbosity::Normal,
        )
    }
}
