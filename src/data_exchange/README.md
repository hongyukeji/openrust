# CSV/XLSX 数据交换

`openrust::data_exchange` 在显式启用 `data-exchange` feature 后提供有界 CSV/XLSX 解析、类型转换和导出。解析器不写数据库，也不管理事务；调用方通过逐行 callback 决定验证通过后的保存边界。

## 导入

用 `ImportSchema` 显式映射来源表头、目标字段和 `CellConverter`。可选字段、框架 `Validate` 校验器及每行错误上限都由 schema/limits 指定。格式错误、公式、超限输入和 callback 错误会立即停止；字段转换错误进入有界 `ImportReport`。

```rust,ignore
use openrust::data_exchange::{CellType, ImportColumn, ImportLimits, ImportSchema, for_each_csv};

let schema = ImportSchema::new(vec![
    ImportColumn::new("email", "email", CellType::Text),
    ImportColumn::new("age", "age", CellType::Integer).optional(),
])?;
let report = for_each_csv(input, &schema, &ImportLimits::default(), |row| {
    if row.errors.is_empty() {
        save_validated_row(row.values)?;
    }
    Ok(())
})?;
```

`for_each_xlsx` 同样逐 cell 读取单个工作表。导入拒绝公式和宏文件，并在解析前检查 ZIP 展开预算与 shared strings 限制。callback 可能已执行部分副作用，callback 返回错误不表示已自动回滚；需要原子性时，由应用按批次开启数据库事务。

## 导出

`write_csv` 与 `write_xlsx` 接收显式表头和逐行结果。CSV 默认转义电子表格公式注入前缀；XLSX 将字符串作为字面文本写出，数字与布尔值保持对应单元格类型。两者均执行行、列、单元格与输出大小限制。对象/数组不能作为 XLSX 单元格导出。

`examples/data-exchange` 展示不依赖内部 crate 的独立 consumer 用法。后续任务化导入、授权导出、持久化失败报告和重试由独立功能簇集成现有 Queue 与事务能力。
