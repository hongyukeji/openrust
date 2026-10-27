//! 验证集成宿主可通过 feature 使用公开的数据交换 API。

use openrust::data_exchange::{CellType, ImportColumn, ImportLimits, ImportSchema, for_each_csv};
use serde_json::json;

#[test]
fn server_feature_exposes_bounded_csv_import_api() -> openrust::Result<()> {
    let schema = ImportSchema::new(vec![
        ImportColumn::new("email", "email", CellType::Text),
        ImportColumn::new("active", "active", CellType::Boolean),
    ])?;
    let mut imported = Vec::new();
    let report = for_each_csv(
        &b"email,active\nuser@example.test,true\n"[..],
        &schema,
        &ImportLimits::default(),
        |row| {
            imported.push(row);
            Ok(())
        },
    )?;

    assert_eq!(report.rows_valid, 1);
    assert_eq!(imported[0].values["active"], json!(true));
    Ok(())
}
