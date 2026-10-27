#![doc = include_str!("data_exchange/README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, Read, Seek, SeekFrom, Write},
    marker::PhantomData,
    sync::Arc,
};

use calamine::{DataRef, Reader, Xlsx};
use serde_json::{Map, Number, Value};
use zip::ZipArchive;

use crate::core::{Error, Result};

/// 格式解析和导出资源上限。
#[derive(Debug, Clone)]
pub struct ImportLimits {
    /// 输入文件的最大字节数。
    pub max_input_bytes: u64,
    /// 最多处理的数据行数，不包含表头。
    pub max_rows: u64,
    /// 每行允许的最大列数。
    pub max_columns: usize,
    /// 单个 cell 允许的最大 UTF-8 字节数。
    pub max_cell_bytes: usize,
    /// 每行最多保留的转换/校验错误数。
    pub max_errors_per_row: usize,
    /// 整个导入报告最多保留的错误数。
    pub max_errors: usize,
    /// XLSX ZIP 内全部 entry 声明的最大展开字节数。
    pub max_archive_bytes: u64,
    /// XLSX ZIP entry 最大数量。
    pub max_archive_entries: usize,
    /// XLSX sharedStrings.xml 的最大展开字节数。
    pub max_shared_strings_bytes: u64,
    /// XLSX shared string 数量上限，避免恶意 count 属性触发大额预分配。
    pub max_shared_string_count: u64,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 32 * 1024 * 1024,
            max_rows: 100_000,
            max_columns: 256,
            max_cell_bytes: 64 * 1024,
            max_errors_per_row: 32,
            max_errors: 10_000,
            max_archive_bytes: 64 * 1024 * 1024,
            max_archive_entries: 256,
            max_shared_strings_bytes: 8 * 1024 * 1024,
            max_shared_string_count: 100_000,
        }
    }
}

impl ImportLimits {
    fn validate(&self) -> Result<()> {
        if self.max_input_bytes == 0
            || self.max_input_bytes > 256 * 1024 * 1024
            || self.max_rows == 0
            || self.max_rows > 1_000_000
            || self.max_columns == 0
            || self.max_columns > 16_384
            || self.max_cell_bytes == 0
            || self.max_cell_bytes > 4 * 1024 * 1024
            || self.max_errors_per_row == 0
            || self.max_errors_per_row > 256
            || self.max_errors == 0
            || self.max_errors > 100_000
            || self.max_archive_bytes == 0
            || self.max_archive_bytes > 512 * 1024 * 1024
            || self.max_archive_entries == 0
            || self.max_archive_entries > 4096
            || self.max_shared_strings_bytes == 0
            || self.max_shared_strings_bytes > self.max_archive_bytes
            || self.max_shared_string_count == 0
            || self.max_shared_string_count > 1_000_000
        {
            return Err(data_error(
                "exchange_limits_invalid",
                "data exchange limits are outside supported bounds",
            ));
        }
        Ok(())
    }
}

/// 可供内建或自定义转换器处理的单元格值。
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    /// 空白单元格。
    Empty,
    /// 文本单元格。
    Text(String),
    /// 有符号整数。
    Integer(i64),
    /// 浮点数值。
    Number(f64),
    /// 布尔值。
    Boolean(bool),
    /// ISO 8601 日期时间文本。
    DateTime(String),
}

impl CellValue {
    fn from_csv(value: &str) -> Self {
        if value.is_empty() {
            Self::Empty
        } else {
            Self::Text(value.to_owned())
        }
    }

    fn text(&self) -> Option<String> {
        match self {
            Self::Empty => Some(String::new()),
            Self::Text(value) | Self::DateTime(value) => Some(value.clone()),
            Self::Integer(value) => Some(value.to_string()),
            Self::Number(value) if value.is_finite() => Some(value.to_string()),
            Self::Number(_) => None,
            Self::Boolean(value) => Some(value.to_string()),
        }
    }
}

/// 单元格转换失败的安全说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellConversionError {
    /// 稳定错误码。
    pub code: String,
    /// 可展示给调用方的安全说明；不得包含原始整行或秘密。
    pub message: String,
}

impl CellConversionError {
    /// 创建转换错误。
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// 将单元格值转换为应用 row 中的 JSON 字段值。
pub trait CellConverter: Send + Sync + 'static {
    /// 将输入 cell 转换为 JSON 值。
    fn convert(&self, value: &CellValue) -> std::result::Result<Value, CellConversionError>;
}

/// 常见类型的内建转换器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellType {
    /// 保留文本；数字与布尔值转换为文本。
    Text,
    /// 转换为有符号 64 位整数。
    Integer,
    /// 转换为有限浮点 JSON 数字。
    Number,
    /// 转换为布尔值，接受 true/false 与 1/0。
    Boolean,
    /// 转换为 ISO 8601 文本。
    DateTime,
}

impl CellConverter for CellType {
    fn convert(&self, value: &CellValue) -> std::result::Result<Value, CellConversionError> {
        let invalid = || CellConversionError::new("cell_type_invalid", "cell type is invalid");
        match self {
            Self::Text => value.text().map(Value::String).ok_or_else(invalid),
            Self::Integer => match value {
                CellValue::Empty => Ok(Value::Null),
                CellValue::Integer(value) => Ok(Value::Number((*value).into())),
                _ => value
                    .text()
                    .and_then(|text| text.trim().parse::<i64>().ok())
                    .map(|value| Value::Number(value.into()))
                    .ok_or_else(invalid),
            },
            Self::Number => match value {
                CellValue::Empty => Ok(Value::Null),
                CellValue::Integer(value) => Ok(Value::Number((*value).into())),
                CellValue::Number(value) => Number::from_f64(*value)
                    .map(Value::Number)
                    .ok_or_else(invalid),
                _ => value
                    .text()
                    .and_then(|text| text.trim().parse::<f64>().ok())
                    .and_then(Number::from_f64)
                    .map(Value::Number)
                    .ok_or_else(invalid),
            },
            Self::Boolean => match value {
                CellValue::Empty => Ok(Value::Null),
                CellValue::Boolean(value) => Ok(Value::Bool(*value)),
                _ => match value.text().as_deref().map(str::trim) {
                    Some("true" | "1") => Ok(Value::Bool(true)),
                    Some("false" | "0") => Ok(Value::Bool(false)),
                    _ => Err(invalid()),
                },
            },
            Self::DateTime => match value {
                CellValue::Empty => Ok(Value::Null),
                CellValue::DateTime(value) | CellValue::Text(value) => {
                    Ok(Value::String(value.clone()))
                }
                _ => Err(invalid()),
            },
        }
    }
}

/// 一列输入表头、目标字段与转换器的绑定。
#[derive(Clone)]
pub struct ImportColumn {
    source: String,
    target: String,
    required: bool,
    converter: Arc<dyn CellConverter>,
}

impl ImportColumn {
    /// 创建必需列绑定。
    pub fn new(
        source: impl Into<String>,
        target: impl Into<String>,
        converter: impl CellConverter,
    ) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            required: true,
            converter: Arc::new(converter),
        }
    }

    /// 将输入列声明为可选；缺少时转换为 JSON null。
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }
}

/// 对转换完成的 row 执行应用级校验。
pub trait ImportRowValidator: Send + Sync + 'static {
    /// 返回字段路径及安全错误说明。
    fn validate(&self, values: &Map<String, Value>) -> Vec<ImportFieldError>;
}

/// 使用已有 `Validate` derive 与 OpenRust 统一校验器构建 row validator。
pub fn typed_validator<T>() -> impl ImportRowValidator
where
    T: serde::de::DeserializeOwned + crate::validation::Validate + Send + Sync + 'static,
{
    TypedValidator::<T>(PhantomData)
}

struct TypedValidator<T>(PhantomData<T>);

impl<T> ImportRowValidator for TypedValidator<T>
where
    T: serde::de::DeserializeOwned + crate::validation::Validate + Send + Sync + 'static,
{
    fn validate(&self, values: &Map<String, Value>) -> Vec<ImportFieldError> {
        let Ok(value) = serde_json::from_value::<T>(Value::Object(values.clone())) else {
            return vec![ImportFieldError::new(
                None::<String>,
                "row_decode_invalid",
                "converted row does not match the application data type",
            )];
        };
        match crate::validation::validate(value) {
            Ok(_) => Vec::new(),
            Err(Error::Validation(errors)) => errors
                .iter()
                .flat_map(|(field, messages)| {
                    messages.iter().map(move |message| {
                        ImportFieldError::new(Some(field), "row_validation_failed", message)
                    })
                })
                .collect(),
            Err(_) => vec![ImportFieldError::new(
                None::<String>,
                "row_validation_failed",
                "converted row failed application validation",
            )],
        }
    }
}

/// 输入表头到应用字段的显式映射与验证规则。
#[derive(Clone)]
pub struct ImportSchema {
    columns: Vec<ImportColumn>,
    validator: Option<Arc<dyn ImportRowValidator>>,
}

impl ImportSchema {
    /// 创建列映射；重复输入表头或目标字段会返回错误。
    pub fn new(columns: Vec<ImportColumn>) -> Result<Self> {
        let mut sources = BTreeSet::new();
        let mut targets = BTreeSet::new();
        for column in &columns {
            validate_name(&column.source)?;
            validate_name(&column.target)?;
            if !sources.insert(column.source.clone()) || !targets.insert(column.target.clone()) {
                return Err(data_error(
                    "exchange_schema_duplicate",
                    "import schema contains duplicate column names",
                ));
            }
        }
        if columns.is_empty() || columns.len() > 16_384 {
            return Err(data_error(
                "exchange_schema_invalid",
                "import schema must contain a supported number of columns",
            ));
        }
        Ok(Self {
            columns,
            validator: None,
        })
    }

    /// 添加 row 级应用校验。
    #[must_use]
    pub fn with_validator(mut self, validator: impl ImportRowValidator) -> Self {
        self.validator = Some(Arc::new(validator));
        self
    }
}

/// 单条 row 的字段错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportFieldError {
    /// 目标字段路径；整行错误为 `None`。
    pub field: Option<String>,
    /// 稳定错误码。
    pub code: String,
    /// 可展示的安全说明。
    pub message: String,
}

impl ImportFieldError {
    /// 创建字段错误。
    pub fn new(
        field: Option<impl Into<String>>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            field: field.map(Into::into),
            code: code.into(),
            message: message.into(),
        }
    }
}

/// 已解析并转换的一条输入 row。
#[derive(Debug, Clone, PartialEq)]
pub struct ImportRow {
    /// 源文件中的 1-based 数据行号。
    pub row_number: u64,
    /// 已转换的目标字段值；转换失败的字段不在 map 中。
    pub values: Map<String, Value>,
    /// 当前 row 的格式、转换和业务校验错误。
    pub errors: Vec<ImportFieldError>,
}

/// 流式解析与 callback 结果统计。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// 遇到的数据行数，不包括表头。
    pub rows_read: u64,
    /// 没有错误的 row 数。
    pub rows_valid: u64,
    /// 含错误的 row 数。
    pub rows_invalid: u64,
    /// 有界保留的错误详情。
    pub errors: Vec<ImportRowError>,
    /// 错误详情超过 `ImportLimits::max_errors` 时为 true。
    pub errors_truncated: bool,
}

/// 导入报告中的一条有界 row 错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRowError {
    /// 源文件中的 1-based 行号。
    pub row_number: u64,
    /// 错误字段路径。
    pub field: Option<String>,
    /// 稳定错误码。
    pub code: String,
    /// 安全错误说明。
    pub message: String,
}

/// 解析 CSV 并逐行回调；格式/限制错误立即中止，行校验错误进入报告。
pub fn for_each_csv<R, F>(
    input: R,
    schema: &ImportSchema,
    limits: &ImportLimits,
    mut on_row: F,
) -> Result<ImportReport>
where
    R: Read,
    F: FnMut(ImportRow) -> Result<()>,
{
    limits.validate()?;
    let bounded = LimitedReader::new(input, limits.max_input_bytes);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(false)
        .from_reader(bounded);
    let mut rows = reader.records();
    let headers = rows
        .next()
        .transpose()
        .map_err(|_| data_error("exchange_csv_invalid", "CSV header could not be parsed"))?
        .ok_or_else(|| data_error("exchange_header_missing", "CSV header row is missing"))?;
    let mut header_names: Vec<String> = headers.iter().map(str::to_owned).collect();
    if let Some(first) = header_names.first_mut()
        && let Some(without_bom) = first.strip_prefix('\u{feff}')
    {
        *first = without_bom.to_owned();
    }
    validate_headers(&header_names, schema, limits)?;
    let mut report = ImportReport::default();
    for record in rows {
        let record = record
            .map_err(|_| data_error("exchange_csv_invalid", "CSV row could not be parsed"))?;
        report.rows_read = report.rows_read.saturating_add(1);
        if report.rows_read > limits.max_rows {
            return Err(data_error(
                "exchange_row_limit_exceeded",
                "input contains more rows than the configured limit",
            ));
        }
        let row_number = record.position().map_or(0, csv::Position::line);
        let cells: Vec<CellValue> = record.iter().map(CellValue::from_csv).collect();
        let row = convert_row(row_number, &header_names, &cells, schema, limits);
        account_row(&mut report, row, limits, &mut on_row)?;
    }
    if reader.get_ref().exceeded() {
        return Err(data_error(
            "exchange_input_limit_exceeded",
            "input exceeds the configured byte limit",
        ));
    }
    Ok(report)
}

/// 选择 XLSX 工作表；默认读取第一张工作表。
#[derive(Debug, Clone, Default)]
pub struct XlsxImportOptions {
    /// 可选的精确工作表名称。
    pub sheet_name: Option<String>,
}

/// 预检后使用 calamine streaming cell reader 解析 XLSX。
pub fn for_each_xlsx<R, F>(
    mut input: R,
    options: &XlsxImportOptions,
    schema: &ImportSchema,
    limits: &ImportLimits,
    mut on_row: F,
) -> Result<ImportReport>
where
    R: Read + Seek,
    F: FnMut(ImportRow) -> Result<()>,
{
    limits.validate()?;
    preflight_xlsx(&mut input, limits)?;
    input
        .seek(SeekFrom::Start(0))
        .map_err(|_| data_error("exchange_xlsx_invalid", "XLSX input could not be rewound"))?;
    let mut workbook = Xlsx::new(input)
        .map_err(|_| data_error("exchange_xlsx_invalid", "XLSX workbook is invalid"))?;
    let sheet_names = workbook.sheet_names().clone();
    let sheet = options
        .sheet_name
        .as_deref()
        .or_else(|| sheet_names.first().map(String::as_str))
        .ok_or_else(|| data_error("exchange_sheet_missing", "XLSX contains no worksheet"))?;
    let mut cells = workbook
        .worksheet_cells_reader(sheet)
        .map_err(|_| data_error("exchange_sheet_invalid", "XLSX worksheet could not be read"))?;
    let dimensions = cells.dimensions();
    validate_dimensions(dimensions.start, dimensions.end, limits)?;
    stream_xlsx_rows(&mut cells, schema, limits, &mut on_row)
}

fn stream_xlsx_rows<R, F>(
    cells: &mut calamine::XlsxCellReader<'_, R>,
    schema: &ImportSchema,
    limits: &ImportLimits,
    on_row: &mut F,
) -> Result<ImportReport>
where
    R: Read + Seek,
    F: FnMut(ImportRow) -> Result<()>,
{
    let mut report = ImportReport::default();
    let mut header_names: Option<Vec<String>> = None;
    let mut current_row: Option<u32> = None;
    let mut row_cells = BTreeMap::<usize, CellValue>::new();
    let mut cell_count = 0_u64;
    let max_row_index = u32::try_from(limits.max_rows.saturating_add(1)).map_err(|_| {
        data_error(
            "exchange_limits_invalid",
            "row limit exceeds XLSX index bounds",
        )
    })?;
    let max_column_index = u32::try_from(limits.max_columns).map_err(|_| {
        data_error(
            "exchange_limits_invalid",
            "column limit exceeds XLSX index bounds",
        )
    })?;
    loop {
        let cell = cells
            .next_cell_with_formula()
            .map_err(|_| data_error("exchange_xlsx_invalid", "XLSX cell could not be parsed"))?;
        let Some(cell) = cell else { break };
        if cell.formula.is_some() {
            return Err(data_error(
                "exchange_formula_unsupported",
                "formula cells are not accepted during import",
            ));
        }
        let (row_index, col_index) = cell.pos;
        if row_index >= max_row_index || col_index >= max_column_index {
            return Err(data_error(
                "exchange_cell_limit_exceeded",
                "XLSX cell is outside configured row or column limits",
            ));
        }
        cell_count = cell_count.saturating_add(1);
        if cell_count > limits.max_rows.saturating_mul(limits.max_columns as u64) {
            return Err(data_error(
                "exchange_cell_limit_exceeded",
                "XLSX contains more cells than the configured limit",
            ));
        }
        if current_row.is_some_and(|current| current != row_index) {
            process_xlsx_row(
                current_row.unwrap_or_default(),
                std::mem::take(&mut row_cells),
                &mut header_names,
                schema,
                limits,
                &mut report,
                on_row,
            )?;
        }
        current_row = Some(row_index);
        let value = xlsx_cell_value(cell.value)
            .map_err(|()| data_error("exchange_cell_invalid", "XLSX cell value is invalid"))?;
        if value
            .text()
            .is_some_and(|text| text.len() > limits.max_cell_bytes)
        {
            return Err(data_error(
                "exchange_cell_limit_exceeded",
                "XLSX cell exceeds the configured byte limit",
            ));
        }
        row_cells.insert(col_index as usize, value);
    }
    if let Some(row_index) = current_row {
        process_xlsx_row(
            row_index,
            row_cells,
            &mut header_names,
            schema,
            limits,
            &mut report,
            on_row,
        )?;
    }
    if header_names.is_none() {
        return Err(data_error(
            "exchange_header_missing",
            "XLSX header row is missing",
        ));
    }
    Ok(report)
}

fn process_xlsx_row<F>(
    row_index: u32,
    mut cells: BTreeMap<usize, CellValue>,
    headers: &mut Option<Vec<String>>,
    schema: &ImportSchema,
    limits: &ImportLimits,
    report: &mut ImportReport,
    on_row: &mut F,
) -> Result<()>
where
    F: FnMut(ImportRow) -> Result<()>,
{
    let width = cells.keys().next_back().map_or(0, |index| index + 1);
    let values: Vec<CellValue> = (0..width)
        .map(|index| cells.remove(&index).unwrap_or(CellValue::Empty))
        .collect();
    if headers.is_none() {
        let header_names: Vec<String> = values
            .iter()
            .map(|value| value.text().unwrap_or_default())
            .collect();
        validate_headers(&header_names, schema, limits)?;
        *headers = Some(header_names);
        return Ok(());
    }
    report.rows_read = report.rows_read.saturating_add(1);
    if report.rows_read > limits.max_rows {
        return Err(data_error(
            "exchange_row_limit_exceeded",
            "input contains more rows than the configured limit",
        ));
    }
    let row = convert_row(
        u64::from(row_index).saturating_add(1),
        headers.as_deref().unwrap_or_default(),
        &values,
        schema,
        limits,
    );
    account_row(report, row, limits, on_row)
}

fn xlsx_cell_value(value: DataRef<'_>) -> std::result::Result<CellValue, ()> {
    match value {
        DataRef::Empty => Ok(CellValue::Empty),
        DataRef::String(value) => Ok(CellValue::Text(value)),
        DataRef::SharedString(value) => Ok(CellValue::Text(value.to_owned())),
        DataRef::Int(value) => Ok(CellValue::Integer(value)),
        DataRef::Float(value) if value.is_finite() => Ok(CellValue::Number(value)),
        DataRef::Float(_) | DataRef::Error(_) => Err(()),
        DataRef::Bool(value) => Ok(CellValue::Boolean(value)),
        DataRef::DateTime(value) => value
            .as_datetime()
            .map(|datetime| {
                CellValue::DateTime(datetime.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
            })
            .ok_or(()),
        DataRef::DateTimeIso(value) | DataRef::DurationIso(value) => Ok(CellValue::DateTime(value)),
    }
}

fn preflight_xlsx<R: Read + Seek>(input: &mut R, limits: &ImportLimits) -> Result<()> {
    let file_size = input
        .seek(SeekFrom::End(0))
        .map_err(|_| data_error("exchange_xlsx_invalid", "XLSX input size is unavailable"))?;
    if file_size == 0 || file_size > limits.max_input_bytes {
        return Err(data_error(
            "exchange_input_limit_exceeded",
            "XLSX input is empty or exceeds the configured byte limit",
        ));
    }
    input
        .seek(SeekFrom::Start(0))
        .map_err(|_| data_error("exchange_xlsx_invalid", "XLSX input could not be rewound"))?;
    let mut archive = ZipArchive::new(&mut *input)
        .map_err(|_| data_error("exchange_xlsx_invalid", "XLSX archive is invalid"))?;
    let overlapping = archive.has_overlapping_entries().map_err(|_| {
        data_error(
            "exchange_archive_invalid",
            "XLSX archive layout could not be checked",
        )
    })?;
    if archive.is_empty() || archive.len() > limits.max_archive_entries || overlapping {
        return Err(data_error(
            "exchange_archive_invalid",
            "XLSX archive entry count or layout is invalid",
        ));
    }
    let mut names = BTreeSet::new();
    let mut expanded_total = 0_u64;
    let mut shared_strings_name = None;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| data_error("exchange_archive_invalid", "XLSX archive entry is invalid"))?;
        let name = entry
            .name()
            .map_err(|_| {
                data_error(
                    "exchange_archive_invalid",
                    "XLSX archive entry name is invalid",
                )
            })?
            .into_owned();
        let size = entry.size();
        if !names.insert(name.clone())
            || size > limits.max_archive_bytes
            || name.ends_with("vbaProject.bin")
        {
            return Err(data_error(
                "exchange_archive_invalid",
                "XLSX archive contains duplicate, oversized, or macro entries",
            ));
        }
        expanded_total = expanded_total.checked_add(size).ok_or_else(|| {
            data_error(
                "exchange_archive_limit_exceeded",
                "XLSX expanded size overflowed",
            )
        })?;
        if expanded_total > limits.max_archive_bytes {
            return Err(data_error(
                "exchange_archive_limit_exceeded",
                "XLSX archive expands beyond the configured limit",
            ));
        }
        if name == "xl/sharedStrings.xml" {
            if size > limits.max_shared_strings_bytes {
                return Err(data_error(
                    "exchange_shared_strings_limit_exceeded",
                    "XLSX shared strings exceed the configured limit",
                ));
            }
            shared_strings_name = Some(name);
        }
    }
    if let Some(name) = shared_strings_name {
        let entry = archive.by_name(&name).map_err(|_| {
            data_error(
                "exchange_archive_invalid",
                "XLSX shared strings are invalid",
            )
        })?;
        let mut bytes = Vec::with_capacity(entry.size().min(1024 * 1024) as usize);
        entry
            .take(limits.max_shared_strings_bytes.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|_| {
                data_error(
                    "exchange_archive_invalid",
                    "XLSX shared strings are invalid",
                )
            })?;
        if bytes.len() as u64 > limits.max_shared_strings_bytes {
            return Err(data_error(
                "exchange_shared_strings_limit_exceeded",
                "XLSX shared strings exceed the configured limit",
            ));
        }
        validate_shared_strings(&bytes, limits)?;
    }
    Ok(())
}

fn validate_shared_strings(bytes: &[u8], limits: &ImportLimits) -> Result<()> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    let mut buffer = Vec::new();
    let mut seen_root = false;
    let mut actual_count = 0_u64;
    loop {
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(quick_xml::events::Event::Start(element))
                if element.local_name().as_ref() == "sst" =>
            {
                if seen_root {
                    return Err(data_error(
                        "exchange_xlsx_invalid",
                        "XLSX shared strings are malformed",
                    ));
                }
                seen_root = true;
                for attribute in element.attributes().flatten() {
                    if attribute.key.local_name().as_ref() == "uniqueCount" {
                        let declared =
                            attribute
                                .value
                                .as_ref()
                                .parse::<u64>()
                                .ok()
                                .ok_or_else(|| {
                                    data_error(
                                        "exchange_shared_strings_invalid",
                                        "XLSX shared string count is invalid",
                                    )
                                })?;
                        if declared > limits.max_shared_string_count {
                            return Err(data_error(
                                "exchange_shared_strings_limit_exceeded",
                                "XLSX shared strings exceed the configured cell limit",
                            ));
                        }
                    }
                }
            }
            Ok(quick_xml::events::Event::Start(element))
                if element.local_name().as_ref() == "si" =>
            {
                actual_count = actual_count.saturating_add(1);
                if actual_count > limits.max_shared_string_count {
                    return Err(data_error(
                        "exchange_shared_strings_limit_exceeded",
                        "XLSX shared strings exceed the configured cell limit",
                    ));
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => {
                return Err(data_error(
                    "exchange_xlsx_invalid",
                    "XLSX shared strings are malformed",
                ));
            }
            _ => {}
        }
    }
    if !seen_root {
        return Err(data_error(
            "exchange_xlsx_invalid",
            "XLSX shared strings are malformed",
        ));
    }
    Ok(())
}

fn validate_dimensions(start: (u32, u32), end: (u32, u32), limits: &ImportLimits) -> Result<()> {
    if end.0 < start.0
        || end.1 < start.1
        || u64::from(end.0 - start.0).saturating_add(1) > limits.max_rows.saturating_add(1)
        || u64::from(end.1 - start.1).saturating_add(1) > limits.max_columns as u64
    {
        return Err(data_error(
            "exchange_dimension_limit_exceeded",
            "XLSX worksheet dimensions exceed configured limits",
        ));
    }
    Ok(())
}

fn validate_headers(
    headers: &[String],
    schema: &ImportSchema,
    limits: &ImportLimits,
) -> Result<()> {
    if headers.is_empty() || headers.len() > limits.max_columns {
        return Err(data_error(
            "exchange_column_limit_exceeded",
            "input contains more columns than the configured limit",
        ));
    }
    let mut seen = BTreeSet::new();
    for header in headers {
        if header.trim().is_empty() || header.len() > limits.max_cell_bytes || !seen.insert(header)
        {
            return Err(data_error(
                "exchange_header_invalid",
                "input headers must be nonempty, unique, and within the cell limit",
            ));
        }
    }
    for column in &schema.columns {
        if column.required && !seen.contains(&column.source) {
            return Err(data_error(
                "exchange_required_column_missing",
                "a required import column is missing",
            ));
        }
    }
    if headers
        .iter()
        .any(|header| !schema.columns.iter().any(|column| &column.source == header))
    {
        return Err(data_error(
            "exchange_column_unmapped",
            "input contains a column without an explicit mapping",
        ));
    }
    Ok(())
}

fn convert_row(
    row_number: u64,
    headers: &[String],
    cells: &[CellValue],
    schema: &ImportSchema,
    limits: &ImportLimits,
) -> ImportRow {
    let mut values = Map::new();
    let mut errors = Vec::new();
    if cells.len() > headers.len() {
        errors.push(ImportFieldError::new(
            None::<String>,
            "exchange_column_count_mismatch",
            "row contains a different number of columns than the header",
        ));
        return ImportRow {
            row_number,
            values,
            errors,
        };
    }
    let indexes: BTreeMap<&str, usize> = headers
        .iter()
        .enumerate()
        .map(|(index, header)| (header.as_str(), index))
        .collect();
    for column in &schema.columns {
        let Some(index) = indexes.get(column.source.as_str()).copied() else {
            values.insert(column.target.clone(), Value::Null);
            continue;
        };
        let empty = CellValue::Empty;
        let cell = cells.get(index).unwrap_or(&empty);
        if cell
            .text()
            .is_some_and(|text| text.len() > limits.max_cell_bytes)
        {
            push_field_error(
                &mut errors,
                limits,
                ImportFieldError::new(
                    Some(column.target.as_str()),
                    "exchange_cell_limit_exceeded",
                    "cell exceeds the configured byte limit",
                ),
            );
            continue;
        }
        match column.converter.convert(cell) {
            Ok(value) => {
                values.insert(column.target.clone(), value);
            }
            Err(error) => push_field_error(
                &mut errors,
                limits,
                ImportFieldError::new(Some(column.target.as_str()), error.code, error.message),
            ),
        }
    }
    if let Some(validator) = &schema.validator {
        for error in validator.validate(&values) {
            push_field_error(&mut errors, limits, error);
        }
    }
    ImportRow {
        row_number,
        values,
        errors,
    }
}

fn push_field_error(
    errors: &mut Vec<ImportFieldError>,
    limits: &ImportLimits,
    error: ImportFieldError,
) {
    if errors.len() < limits.max_errors_per_row {
        errors.push(error);
    }
}

fn account_row<F>(
    report: &mut ImportReport,
    row: ImportRow,
    limits: &ImportLimits,
    on_row: &mut F,
) -> Result<()>
where
    F: FnMut(ImportRow) -> Result<()>,
{
    if row.errors.is_empty() {
        report.rows_valid = report.rows_valid.saturating_add(1);
    } else {
        report.rows_invalid = report.rows_invalid.saturating_add(1);
        for error in &row.errors {
            if report.errors.len() < limits.max_errors {
                report.errors.push(ImportRowError {
                    row_number: row.row_number,
                    field: error.field.clone(),
                    code: error.code.clone(),
                    message: error.message.clone(),
                });
            } else {
                report.errors_truncated = true;
            }
        }
    }
    on_row(row)
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err(data_error(
            "exchange_schema_invalid",
            "import column names must be nonempty and bounded",
        ));
    }
    Ok(())
}

fn data_error(code: &'static str, message: &'static str) -> Error {
    Error::custom(code, message)
}

struct LimitedReader<R> {
    inner: R,
    limit: u64,
    read: u64,
}

impl<R> LimitedReader<R> {
    fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            limit,
            read: 0,
        }
    }

    fn exceeded(&self) -> bool {
        self.read > self.limit
    }
}

impl<R: Read> Read for LimitedReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.limit.saturating_add(1).saturating_sub(self.read);
        if remaining == 0 {
            return Ok(0);
        }
        let allowed = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let count = self.inner.read(&mut buffer[..allowed])?;
        self.read = self.read.saturating_add(count as u64);
        Ok(count)
    }
}

/// 表格导出允许的行列与文本大小。
#[derive(Debug, Clone)]
pub struct ExportLimits {
    /// 最大数据行数。
    pub max_rows: u64,
    /// 最大列数。
    pub max_columns: usize,
    /// 最大单元格 UTF-8 字节数。
    pub max_cell_bytes: usize,
    /// 最大导出文件字节数。
    pub max_output_bytes: u64,
}

impl Default for ExportLimits {
    fn default() -> Self {
        Self {
            max_rows: 100_000,
            max_columns: 256,
            max_cell_bytes: 64 * 1024,
            max_output_bytes: 64 * 1024 * 1024,
        }
    }
}

impl ExportLimits {
    fn validate(&self) -> Result<()> {
        if self.max_rows == 0
            || self.max_rows > 1_000_000
            || self.max_columns == 0
            || self.max_columns > 16_384
            || self.max_cell_bytes == 0
            || self.max_cell_bytes > 4 * 1024 * 1024
            || self.max_output_bytes == 0
            || self.max_output_bytes > 512 * 1024 * 1024
        {
            return Err(data_error(
                "exchange_limits_invalid",
                "export limits are outside supported bounds",
            ));
        }
        Ok(())
    }
}

/// CSV 导出公式注入策略。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CsvFormulaPolicy {
    /// 对可能被电子表格解释为公式的文本加 apostrophe 前缀。
    #[default]
    Escape,
    /// 保留原文，仅适用于不会被电子表格执行的下游消费者。
    Preserve,
}

/// CSV/XLSX 导出配置。
#[derive(Debug, Clone, Default)]
pub struct ExportOptions {
    /// 对行、列、cell 和输出文件应用的上限。
    pub limits: ExportLimits,
    /// CSV 文本公式注入处理策略；XLSX 始终写入字面文本 cell。
    pub csv_formula_policy: CsvFormulaPolicy,
}

/// 用显式列顺序流式写入 CSV；文本默认执行电子表格公式注入转义。
pub fn write_csv<W, I>(
    output: W,
    headers: &[String],
    rows: I,
    options: &ExportOptions,
) -> Result<()>
where
    W: Write,
    I: IntoIterator<Item = Result<Map<String, Value>>>,
{
    options.limits.validate()?;
    validate_export_headers(headers, &options.limits)?;
    let bounded = BoundedWriter::new(output, options.limits.max_output_bytes);
    let mut writer = csv::WriterBuilder::new().from_writer(bounded);
    writer.write_record(headers).map_err(|_| {
        data_error(
            "exchange_csv_write_failed",
            "CSV output could not be written",
        )
    })?;
    for (index, row) in rows.into_iter().enumerate() {
        if index as u64 >= options.limits.max_rows {
            return Err(data_error(
                "exchange_row_limit_exceeded",
                "export contains more rows than the configured limit",
            ));
        }
        let row = row?;
        let fields = export_fields(headers, &row, &options.limits, options.csv_formula_policy)?;
        writer.write_record(fields).map_err(|_| {
            data_error(
                "exchange_csv_write_failed",
                "CSV output could not be written",
            )
        })?;
    }
    writer.flush().map_err(|_| {
        data_error(
            "exchange_csv_write_failed",
            "CSV output could not be written",
        )
    })?;
    if writer.get_ref().exceeded() {
        return Err(data_error(
            "exchange_output_limit_exceeded",
            "output exceeds the configured byte limit",
        ));
    }
    Ok(())
}

/// 使用 constant-memory worksheet 流式写入一个 XLSX 工作表。
pub fn write_xlsx<W, I>(
    output: W,
    sheet_name: &str,
    headers: &[String],
    rows: I,
    options: &ExportOptions,
) -> Result<()>
where
    W: Write + Send,
    I: IntoIterator<Item = Result<Map<String, Value>>>,
{
    options.limits.validate()?;
    validate_export_headers(headers, &options.limits)?;
    if sheet_name.is_empty() || sheet_name.len() > 31 || sheet_name.chars().any(char::is_control) {
        return Err(data_error(
            "exchange_sheet_name_invalid",
            "XLSX worksheet name is invalid",
        ));
    }
    let mut workbook = rust_xlsxwriter::Workbook::new();
    let worksheet = workbook.add_worksheet_with_constant_memory().map_err(|_| {
        data_error(
            "exchange_xlsx_write_failed",
            "XLSX worksheet could not be created",
        )
    })?;
    worksheet.set_name(sheet_name).map_err(|_| {
        data_error(
            "exchange_sheet_name_invalid",
            "XLSX worksheet name is invalid",
        )
    })?;
    for (index, header) in headers.iter().enumerate() {
        let column = u16::try_from(index).map_err(|_| {
            data_error(
                "exchange_column_limit_exceeded",
                "export column index is invalid",
            )
        })?;
        worksheet.write_string(0, column, header).map_err(|_| {
            data_error(
                "exchange_xlsx_write_failed",
                "XLSX header could not be written",
            )
        })?;
    }
    let max_rows = usize::try_from(options.limits.max_rows)
        .map_err(|_| data_error("exchange_limits_invalid", "export row limit is invalid"))?;
    for (row_index, row) in rows.into_iter().enumerate() {
        if row_index >= max_rows {
            return Err(data_error(
                "exchange_row_limit_exceeded",
                "export contains more rows than the configured limit",
            ));
        }
        let row = row?;
        let output_row = u32::try_from(row_index.saturating_add(1)).map_err(|_| {
            data_error("exchange_row_limit_exceeded", "export row index overflowed")
        })?;
        for (column, header) in headers.iter().enumerate() {
            let column = u16::try_from(column).map_err(|_| {
                data_error(
                    "exchange_column_limit_exceeded",
                    "export column index is invalid",
                )
            })?;
            write_xlsx_cell(
                worksheet,
                output_row,
                column,
                row.get(header),
                &options.limits,
            )?;
        }
    }
    workbook
        .save_to_writer(BoundedWriter::new(output, options.limits.max_output_bytes))
        .map_err(|_| {
            data_error(
                "exchange_xlsx_write_failed",
                "XLSX output could not be written",
            )
        })
}

fn write_xlsx_cell(
    worksheet: &mut rust_xlsxwriter::Worksheet,
    row: u32,
    column: u16,
    value: Option<&Value>,
    limits: &ExportLimits,
) -> Result<()> {
    let write_error = || {
        data_error(
            "exchange_xlsx_write_failed",
            "XLSX cell could not be written",
        )
    };
    match value.unwrap_or(&Value::Null) {
        Value::Null => Ok(()),
        Value::String(value) => {
            if value.len() > limits.max_cell_bytes {
                return Err(data_error(
                    "exchange_cell_limit_exceeded",
                    "export cell exceeds the configured byte limit",
                ));
            }
            worksheet
                .write_string(row, column, value)
                .map(|_| ())
                .map_err(|_| write_error())
        }
        Value::Bool(value) => worksheet
            .write_boolean(row, column, *value)
            .map(|_| ())
            .map_err(|_| write_error()),
        Value::Number(value) => {
            let number = value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| {
                    data_error(
                        "exchange_cell_invalid",
                        "XLSX numeric value is outside the supported range",
                    )
                })?;
            worksheet
                .write_number(row, column, number)
                .map(|_| ())
                .map_err(|_| write_error())
        }
        Value::Array(_) | Value::Object(_) => Err(data_error(
            "exchange_cell_invalid",
            "XLSX cells support only scalar values",
        )),
    }
}

fn validate_export_headers(headers: &[String], limits: &ExportLimits) -> Result<()> {
    if headers.is_empty() || headers.len() > limits.max_columns {
        return Err(data_error(
            "exchange_column_limit_exceeded",
            "export contains more columns than the configured limit",
        ));
    }
    let mut seen = BTreeSet::new();
    if headers.iter().any(|header| {
        header.trim().is_empty() || header.len() > limits.max_cell_bytes || !seen.insert(header)
    }) {
        return Err(data_error(
            "exchange_header_invalid",
            "export headers must be nonempty, unique, and within limits",
        ));
    }
    Ok(())
}

fn export_fields(
    headers: &[String],
    row: &Map<String, Value>,
    limits: &ExportLimits,
    policy: CsvFormulaPolicy,
) -> Result<Vec<String>> {
    headers
        .iter()
        .map(|header| {
            let value = row.get(header).unwrap_or(&Value::Null);
            let mut output = match value {
                Value::Null => String::new(),
                Value::String(value) => value.clone(),
                Value::Bool(value) => value.to_string(),
                Value::Number(value) => value.to_string(),
                Value::Array(_) | Value::Object(_) => {
                    return Err(data_error(
                        "exchange_export_value_invalid",
                        "nested JSON values cannot be exported as cells",
                    ));
                }
            };
            if output.len() > limits.max_cell_bytes {
                return Err(data_error(
                    "exchange_cell_limit_exceeded",
                    "export cell exceeds the configured byte limit",
                ));
            }
            if policy == CsvFormulaPolicy::Escape && is_spreadsheet_formula(&output) {
                output.insert(0, '\'');
            }
            Ok(output)
        })
        .collect()
}

fn is_spreadsheet_formula(value: &str) -> bool {
    let value = value.trim_start_matches([' ', '\t', '\r', '\n']);
    value.starts_with(['=', '+', '-', '@', '\t', '\r', '\n'])
}

struct BoundedWriter<W> {
    inner: W,
    limit: u64,
    written: u64,
}

impl<W> BoundedWriter<W> {
    fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            limit,
            written: 0,
        }
    }

    fn exceeded(&self) -> bool {
        self.written > self.limit
    }
}

impl<W: Write> Write for BoundedWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let remaining = self.limit.saturating_add(1).saturating_sub(self.written);
        if remaining == 0 {
            return Ok(0);
        }
        let allowed = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let written = self.inner.write(&buffer[..allowed])?;
        self.written = self.written.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        CellType, CsvFormulaPolicy, ExportLimits, ExportOptions, ImportColumn, ImportLimits,
        ImportSchema, XlsxImportOptions, for_each_csv, for_each_xlsx, write_csv, write_xlsx,
    };
    use crate::core::Error;
    use calamine::Reader;
    use serde_json::{Map, Value, json};

    fn schema() -> crate::Result<ImportSchema> {
        ImportSchema::new(vec![
            ImportColumn::new("email", "email", CellType::Text),
            ImportColumn::new("age", "age", CellType::Integer).optional(),
        ])
    }

    #[test]
    fn csv_stream_converts_rows_and_collects_bounded_field_errors() -> crate::Result<()> {
        let input = b"\xef\xbb\xbfemail,age\na@example.test,28\nb@example.test,not-a-number\n";
        let mut values = Vec::new();
        let schema = schema()?;
        let report = for_each_csv(&input[..], &schema, &ImportLimits::default(), |row| {
            values.push(row);
            Ok(())
        })?;
        assert_eq!(
            (report.rows_read, report.rows_valid, report.rows_invalid),
            (2, 1, 1)
        );
        assert_eq!(report.errors[0].code, "cell_type_invalid");
        assert_eq!(values[0].values["age"], json!(28));
        Ok(())
    }

    #[test]
    fn csv_rejects_duplicate_headers_and_byte_limit_overflow() -> crate::Result<()> {
        let schema = schema()?;
        let duplicate = for_each_csv(
            &b"email,email\na,b\n"[..],
            &schema,
            &ImportLimits::default(),
            |_| Ok(()),
        );
        assert!(matches!(duplicate, Err(ref error) if error.code() == "exchange_header_invalid"));

        let limits = ImportLimits {
            max_input_bytes: 8,
            ..ImportLimits::default()
        };
        let oversized = for_each_csv(&b"email,age\na,b\n"[..], &schema, &limits, |_| Ok(()));
        assert!(oversized.is_err());
        Ok(())
    }

    #[test]
    fn csv_stops_when_the_row_callback_fails() -> crate::Result<()> {
        let schema = schema()?;
        let callbacks = AtomicUsize::new(0);
        let error = for_each_csv(
            &b"email,age\na@example.test,1\nb@example.test,2\n"[..],
            &schema,
            &ImportLimits::default(),
            |_| {
                callbacks.fetch_add(1, Ordering::SeqCst);
                Err(Error::custom("stop_import", "stop"))
            },
        );
        assert!(matches!(error, Err(ref error) if error.code() == "stop_import"));
        assert_eq!(callbacks.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn csv_error_reports_respect_row_and_global_limits() -> crate::Result<()> {
        let schema = ImportSchema::new(vec![
            ImportColumn::new("age", "age", CellType::Integer),
            ImportColumn::new("active", "active", CellType::Boolean),
        ])?;
        let limits = ImportLimits {
            max_errors_per_row: 1,
            max_errors: 1,
            ..ImportLimits::default()
        };
        let mut rows = Vec::new();
        let report = for_each_csv(
            &b"age,active\ninvalid,invalid\ninvalid,invalid\n"[..],
            &schema,
            &limits,
            |row| {
                rows.push(row);
                Ok(())
            },
        )?;
        assert!(rows.iter().all(|row| row.errors.len() == 1));
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors_truncated);
        Ok(())
    }

    #[test]
    fn csv_export_escapes_formula_prefixes_and_enforces_output_limit() -> crate::Result<()> {
        let headers = vec!["value".to_owned()];
        let mut row = Map::new();
        row.insert("value".to_owned(), Value::String("  =1+1".to_owned()));
        let mut output = Vec::new();
        write_csv(
            &mut output,
            &headers,
            [Ok(row.clone())],
            &ExportOptions::default(),
        )?;
        let output = String::from_utf8(output).map_err(Error::internal)?;
        assert_eq!(output, "value\n'  =1+1\n");

        let limits = ExportOptions {
            limits: ExportLimits {
                max_output_bytes: 3,
                ..ExportLimits::default()
            },
            csv_formula_policy: CsvFormulaPolicy::Preserve,
        };
        assert!(write_csv(&mut Vec::new(), &headers, [Ok(row)], &limits).is_err());
        Ok(())
    }

    #[test]
    fn xlsx_stream_roundtrips_text_and_rejects_formula_cells() -> crate::Result<()> {
        let headers = vec!["email".to_owned(), "age".to_owned()];
        let mut row = Map::new();
        row.insert(
            "email".to_owned(),
            Value::String("=not-a-formula".to_owned()),
        );
        row.insert("age".to_owned(), json!(32));
        let mut output = Vec::new();
        write_xlsx(
            &mut output,
            "Data",
            &headers,
            [Ok(row)],
            &ExportOptions::default(),
        )?;
        let mut raw_workbook =
            calamine::Xlsx::new(Cursor::new(output.clone())).map_err(Error::internal)?;
        let mut raw_cells = raw_workbook
            .worksheet_cells_reader("Data")
            .map_err(Error::internal)?;
        let mut age_cell = None;
        while let Some(cell) = raw_cells
            .next_cell_with_formula()
            .map_err(Error::internal)?
        {
            if cell.pos == (1, 1) {
                age_cell = Some(cell.value);
                break;
            }
        }
        assert!(matches!(
            age_cell,
            Some(calamine::DataRef::Int(32) | calamine::DataRef::Float(32.0))
        ));
        let schema = schema()?;
        let mut rows = Vec::new();
        let report = for_each_xlsx(
            Cursor::new(output),
            &XlsxImportOptions {
                sheet_name: Some("Data".to_owned()),
            },
            &schema,
            &ImportLimits::default(),
            |row| {
                rows.push(row);
                Ok(())
            },
        )?;
        assert_eq!(report.rows_valid, 1);
        assert_eq!(rows[0].values["email"], json!("=not-a-formula"));

        let mut workbook = rust_xlsxwriter::Workbook::new();
        workbook
            .add_worksheet()
            .write_formula(0, 0, "=1+1")
            .map_err(Error::internal)?;
        let formula_book = workbook.save_to_buffer().map_err(Error::internal)?;
        let error = for_each_xlsx(
            Cursor::new(formula_book),
            &XlsxImportOptions::default(),
            &schema,
            &ImportLimits::default(),
            |_| Ok(()),
        );
        assert!(matches!(error, Err(ref error) if error.code() == "exchange_formula_unsupported"));
        Ok(())
    }

    #[test]
    fn xlsx_shared_string_declared_count_is_bounded() {
        let error = super::validate_shared_strings(
            br#"<sst uniqueCount="100001"><si><t>x</t></si></sst>"#,
            &ImportLimits::default(),
        );
        assert!(matches!(
            error,
            Err(ref error) if error.code() == "exchange_shared_strings_limit_exceeded"
        ));
    }

    #[test]
    fn xlsx_preflight_rejects_macro_entries_and_expansion_overflow() -> crate::Result<()> {
        let schema = schema()?;
        for (name, expected_code) in [
            ("xl/vbaProject.bin", "exchange_archive_invalid"),
            ("xl/workbook.xml", "exchange_archive_limit_exceeded"),
        ] {
            let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
            if expected_code == "exchange_archive_limit_exceeded" {
                for entry_name in ["xl/workbook.xml", "xl/worksheets/sheet1.xml"] {
                    archive
                        .start_file(entry_name, zip::write::SimpleFileOptions::default())
                        .map_err(Error::internal)?;
                    archive
                        .write_all(&vec![b'x'; 300])
                        .map_err(Error::internal)?;
                }
            } else {
                archive
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .map_err(Error::internal)?;
                archive
                    .write_all(&vec![b'x'; 1024])
                    .map_err(Error::internal)?;
            }
            let input = archive.finish().map_err(Error::internal)?.into_inner();
            let limits = if expected_code == "exchange_archive_limit_exceeded" {
                ImportLimits {
                    max_archive_bytes: 512,
                    max_shared_strings_bytes: 512,
                    ..ImportLimits::default()
                }
            } else {
                ImportLimits::default()
            };
            let error = for_each_xlsx(
                Cursor::new(input),
                &XlsxImportOptions::default(),
                &schema,
                &limits,
                |_| Ok(()),
            );
            assert!(
                matches!(error, Err(ref error) if error.code() == expected_code),
                "entry {name} returned {error:?}"
            );
        }
        Ok(())
    }
}
