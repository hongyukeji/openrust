//! 展示独立应用通过公开 API 导入并导出 CSV 数据。

use openrust::Result;
use openrust::data_exchange::CellType;
use openrust::data_exchange::ExportOptions;
use openrust::data_exchange::ImportColumn;
use openrust::data_exchange::ImportLimits;
use openrust::data_exchange::ImportSchema;
use openrust::data_exchange::for_each_csv;
use openrust::data_exchange::tasks::ExchangeFormat;
use openrust::data_exchange::tasks::ExportTaskOptions;
use openrust::data_exchange::tasks::ExportTaskReport;
use openrust::data_exchange::tasks::export_storage_task;
use openrust::data_exchange::write_csv;
use openrust::filesystem::StoragePath;
use openrust::queue::JobContext;
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

/// 以小型 fixture 演示独立应用如何提供已授权的导出行。
///
/// 实际应用应以 `AuthorizedSelect` 等受保护查询逐批产生行，而不是先载入整份数据。
pub async fn export_authorized_rows(
    context: &JobContext,
    output_path: StoragePath,
    rows: Vec<Map<String, Value>>,
) -> Result<ExportTaskReport> {
    export_storage_task(
        context,
        "private",
        output_path,
        ExchangeFormat::Csv,
        vec!["email".to_owned()],
        &ExportTaskOptions::default(),
        |sink| async move {
            for row in rows {
                sink.send(row).await?;
            }
            Ok(())
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn public_data_exchange_flow_imports_and_exports() -> openrust::Result<()> {
        run()
    }
}
