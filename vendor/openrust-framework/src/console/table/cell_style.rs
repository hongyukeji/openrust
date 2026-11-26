use super::{ConsoleError, ConsoleTableError, TablePadding};
use serde_json::{Map, Value};
/// 单元格不可变样式；外部读取配置得到副本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCellStyle {
    options: Map<String, Value>,
}
impl Default for TableCellStyle {
    fn default() -> Self {
        Self {
            options: Map::from_iter([
                ("fg".into(), Value::String("default".into())),
                ("bg".into(), Value::String("default".into())),
                ("options".into(), Value::Null),
                ("align".into(), Value::String(Self::DEFAULT_ALIGN.into())),
                ("cellFormat".into(), Value::Null),
            ]),
        }
    }
}
impl TableCellStyle {
    pub const DEFAULT_ALIGN: &'static str = "left";
    pub fn new<K: Into<String>>(
        options: impl IntoIterator<Item = (K, Value)>,
    ) -> Result<Self, ConsoleError> {
        let options: Map<String, Value> = options
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect();
        let mut style = Self::default();
        let unknown: Vec<_> = options
            .keys()
            .filter(|key| !style.options.contains_key(*key))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(ConsoleError::from_source(
                ConsoleTableError::UnknownCellStyleOptions(unknown),
            ));
        }
        if let Some(align) = options.get("align").filter(|value| !value.is_null()) {
            Self::alignment(align)?;
        }
        style.options.extend(options);
        Ok(style)
    }
    pub fn get_options(&self) -> Map<String, Value> {
        self.options.clone()
    }
    pub fn get_tag_options(&self) -> Map<String, Value> {
        ["fg", "bg", "options"]
            .into_iter()
            .filter_map(|key| {
                self.options
                    .get(key)
                    .filter(|value| !value.is_null())
                    .map(|value| (key.to_owned(), value.clone()))
            })
            .collect()
    }
    pub fn get_pad_by_align(&self) -> Result<TablePadding, ConsoleError> {
        Self::alignment(&self.options["align"])
    }
    fn alignment(align: &Value) -> Result<TablePadding, ConsoleError> {
        match align.as_str() {
            Some("left") => Ok(TablePadding::Right),
            Some("center") => Ok(TablePadding::Both),
            Some("right") => Ok(TablePadding::Left),
            _ => Err(ConsoleError::from_source(
                ConsoleTableError::InvalidCellAlignment(align.clone()),
            )),
        }
    }
    pub fn get_cell_format(&self) -> Result<Option<String>, ConsoleError> {
        match &self.options["cellFormat"] {
            Value::Null => Ok(None),
            Value::Array(_) | Value::Object(_) => Err(ConsoleError::from_source(
                ConsoleTableError::InvalidCellFormat,
            )),
            value => Ok(Some(crate::support::input_scalars::string(value.clone()))),
        }
    }
    pub(super) fn format(&self) -> Result<String, ConsoleError> {
        if let Some(format) = self.get_cell_format()? {
            return Ok(format);
        }
        let tags = self.get_tag_options();
        let mut parts = Vec::new();
        for key in ["fg", "bg", "options"] {
            if let Some(value) = tags.get(key) {
                let query = crate::support::Arr::query(&Value::Object(Map::from_iter([(
                    key.into(),
                    value.clone(),
                )])))
                .map_err(ConsoleError::from_source)?;
                if !query.is_empty() {
                    parts.push(
                        query
                            .replace("%20", "+")
                            .replace('~', "%7E")
                            .replace('&', ";"),
                    );
                }
            }
        }
        Ok(format!("<{}>%s</>", parts.join(";")))
    }
}
#[derive(Clone)]
pub(super) struct Attributes {
    pub(super) style: Option<TableCellStyle>,
    pub(super) separator: bool,
    pub(super) colspan: i64,
    pub(super) rowspan: i64,
}
pub(super) type AttributeRows =
    std::collections::BTreeMap<usize, std::collections::BTreeMap<usize, Attributes>>;
