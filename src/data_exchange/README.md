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

`tests/consumers/data-exchange` 展示不依赖内部 crate 的独立 consumer 用法。

## 队列化导入与导出

应用显式启用 `data-exchange-tasks`，并注册 `DatabasePlugin`、`QueuePlugin` 与 `StoragePlugin`。该 feature 组合 `data-exchange`、数据库队列和临时文件能力；本地或对象存储 driver 仍由应用单独启用。迁移中必须包含 `QueueMigration` 与 `CoordinationMigration`。

`import_storage_task` 接收应用已授权的源对象、稳定 `import_id`、格式和 `ImportSchema`，再通过有界流复制到自动回收的临时文件。每个有效批次由框架单独开启数据库事务，应用 callback 在事务内写入业务数据；业务写入和批次幂等完成记录原子提交。重试会跳过已提交批次；批次失败会保留此前提交的数据并保存有界错误报告。幂等记录保留 24 小时，安全的自动恢复窗口也为 24 小时；超过窗口后，应用须使用业务唯一约束或人工核对防止重复写入。`import_id` 应绑定输入、schema 和业务写入语义。`ImportTaskOptions.report_path` 是基础路径，框架按 job ID、attempt 和 import ID 生成唯一报告对象路径，并在成功返回的报告中给出路径；错误报告路径可用 `report_path_for_attempt` 确定。报告默认保留 7 天且最多 30 天。队列取消会保存报告并返回 `queue_job_cancelled`，由现有队列重试策略接管。

```rust,ignore
use openrust::data_exchange::tasks::{
    ExchangeFormat, ImportTaskInput, ImportTaskOptions, import_storage_task,
};

let report = import_storage_task(
    &context,
    ImportTaskInput {
        disk: "private".into(),
        path: source_path,
        version: source_version,
        import_id: job.import_id.clone(),
        format: ExchangeFormat::Csv,
        schema,
    },
    &options,
    |transaction, rows| Box::pin(async move {
        save_import_batch(transaction, rows).await
    }),
).await?;
```

应用负责验证上传者对源对象的访问权限，并在 callback 中执行业务授权和租户约束。`export_storage_task` 同样不接受 SQL 或模型反射；应用提供的 producer 应从 `AuthorizedSelect` 等已授权查询逐批调用 `ExportRowSink::send`。导出先写临时文件，完成后才流式上传对象；`output_path` 是基础路径，框架按 job ID 和 attempt 派生本次尝试的唯一对象路径，实际路径在 `ExportTaskReport.path` 返回。应用必须自行保护下载入口。框架使用数据库 `SharedLocks` 将对象发布与清理串行化；只有完整上传后才写入到期标记并派发清理，避免未完成对象被定时删除。每个尝试路径独立，旧尝试的迟到发布也不会覆盖新尝试的对象。对象操作期间租约每 5 分钟续期，单次续期最多等待 30 秒；续租失败会让任务报错并交由队列重试，但不会丢弃可能已提交的 Storage future，因此数据库不可用时不承诺 Storage exactly-once。worker 请求取消时，上传开始前会检查取消；已开始的上传会等待完成/中止边界后再清理并返回取消错误。应用需确保协调数据库可用，使用 `CoordinationMigration` 创建租约表。清理成功后会同时移除对象和标记。清理 job 由 `QueuePlugin` 自动注册，清理失败交给队列的重试和失败任务管理。

`.openrust-data-exchange-retention/` 是框架内部保留的 Storage key 前缀，不可用于源、报告或导出对象。
