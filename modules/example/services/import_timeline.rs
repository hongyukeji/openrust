//! 账户导入时间线：以服务器观察时点识别正常与异常终结时间。

use openrust::db::query::{ColumnOperand, ExistsOrResult, RawSql};
use openrust::{Db, Error, Result};

/// 当前账户已终结导入的时间线统计。
#[derive(Debug, serde::Serialize)]
pub(crate) struct ImportTimeline {
    /// 创建时间至服务器观察时点之间的记录数，包含两端。
    pub(crate) valid_count: u64,
    /// 终结时间早于创建时间或晚于观察时点的记录数。
    pub(crate) invalid_count: u64,
    /// 当前账户是否仍有等待或执行中的导入。
    pub(crate) has_unfinished_imports: bool,
    /// 服务器本次观察的 Unix 毫秒。
    pub(crate) observed_at_ms: i64,
}

/// 从公开查询构造器读取当前账户；显式时点仅供服务器调用及确定性回归。
pub(crate) async fn report(db: &Db, owner_id: i64, observed_at_ms: i64) -> Result<ImportTimeline> {
    if observed_at_ms < 0 {
        return Err(Error::custom(
            "example_import_clock_invalid",
            "服务器观察时间不可早于 Unix 纪元",
        ));
    }
    // 查询克隆各自消费账户条件回调，账户范围在实际编译前追加到同一构造器。
    let owner_imports = db
        .table("example_article_imports")?
        .before_query(move |query| {
            *query = query.clone().where_eq("owner_id", owner_id)?;
            Ok(())
        });
    let has_unfinished_imports = match owner_imports
        .clone()
        .where_eq("state", "open")?
        .exists_or(|| async { Ok(false) })
        .await?
    {
        ExistsOrResult::True => true,
        ExistsOrResult::Callback(fallback) => fallback,
    };
    let records = owner_imports
        .where_in("state", ["completed", "failed"])?
        .where_not_null("terminal_at_ms")?;
    // SQL 上界只来自校验后的服务器整数，不接收用户提供的 SQL 或时钟。
    let bounds = || {
        [
            ColumnOperand::from("created_at_ms"),
            ColumnOperand::from(RawSql::new(observed_at_ms.to_string())),
        ]
    };
    let valid_count = records
        .clone()
        .where_between_columns("terminal_at_ms", bounds())?
        .count()
        .await?;
    let invalid_count = records
        .where_not_between_columns("terminal_at_ms", bounds())?
        .count()
        .await?;
    Ok(ImportTimeline {
        valid_count,
        invalid_count,
        has_unfinished_imports,
        observed_at_ms,
    })
}
