//! 展示独立应用通过公开 API 导入并导出 CSV 数据。

use openrust::{
    Result,
    data_exchange::{
        CellType, ExportOptions, ImportColumn, ImportLimits, ImportSchema, for_each_csv, write_csv,
    },
};
use serde_json::{Map, Value};

fn main() -> Result<()> {
    run()
}

fn run() -> Result<()> {
    let input = b"email,age\nada@example.test,37\n";
    let schema = ImportSchema::new(vec![
        ImportColumn::new("email", "email", CellType::Text),
        ImportColumn::new("age", "age", CellType::Integer).optional(),
    ])?;
    let mut rows = Vec::<Map<String, Value>>::new();
    let report = for_each_csv(input.as_slice(), &schema, &ImportLimits::default(), |row| {
        if row.errors.is_empty() {
            rows.push(row.values);
        }
        Ok(())
    })?;
    let mut output = Vec::new();
    write_csv(
        &mut output,
        &["email".to_owned(), "age".to_owned()],
        rows.into_iter().map(Ok),
        &ExportOptions::default(),
    )?;
    if report.rows_valid != 1 || output.is_empty() {
        return Err(openrust::Error::custom(
            "example_data_exchange_failed",
            "示例数据交换未完成",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn public_data_exchange_flow_imports_and_exports() -> openrust::Result<()> {
        run()
    }
}
