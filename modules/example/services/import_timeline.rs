//! 账户导入时间线：以服务器观察时点识别正常与异常终结时间。

use openrust::database::query::{
    ColumnOperand, ExistsOrResult, MultiComparison, RawSql, WhereOperator,
};
use openrust::{Db, Error, Result, ValidationErrors};

/// 导入标识和来源名称的组合匹配方式。
#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImportSearchMode {
    /// 任意字段匹配。
    #[default]
    Any,
    /// 两个字段均匹配。
    All,
    /// 两个字段均不匹配。
    None,
}

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
    report_filtered(db, owner_id, observed_at_ms, None, ImportSearchMode::Any).await
}

/// 在账户范围内按导入标识与来源名称检索；模式只控制同一个括号条件组。
pub(crate) async fn report_filtered(
    db: &Db,
    owner_id: i64,
    observed_at_ms: i64,
    search: Option<&str>,
    mode: ImportSearchMode,
) -> Result<ImportTimeline> {
    if observed_at_ms < 0 {
        return Err(Error::custom(
            "example_import_clock_invalid",
            "服务器观察时间不可早于 Unix 纪元",
        ));
    }
    if search.is_some_and(|value| value.len() > 128) {
        let mut errors = ValidationErrors::new();
        errors.add("search", "检索词不能超过128字节");
        return Err(Error::Validation(errors));
    }
    // 查询克隆各自消费账户条件回调，账户范围在实际编译前追加到同一构造器。
    let owner_imports = db
        .table("example_article_imports")?
        .before_query(move |query| {
            *query = query.clone().r#where("owner_id", owner_id)?;
            Ok(())
        });
    let owner_imports = if let Some(search) = search.filter(|value| !value.is_empty()) {
        let comparison = MultiComparison::compare(WhereOperator::Like, format!("%{search}%"));
        match mode {
            ImportSearchMode::Any => {
                owner_imports.where_any(["import_id", "source_key"], comparison)?
            }
            ImportSearchMode::All => {
                owner_imports.where_all(["import_id", "source_key"], comparison)?
            }
            ImportSearchMode::None => {
                owner_imports.where_none(["import_id", "source_key"], comparison)?
            }
        }
    } else {
        owner_imports
    };
    let has_unfinished_imports = match owner_imports
        .clone()
        .where_compare("state", WhereOperator::Eq, "open")?
        .exists_or(|| async { Ok(false) })
        .await?
    {
        ExistsOrResult::True => true,
        ExistsOrResult::Callback(fallback) => fallback,
    };
    let records = owner_imports
        .where_nested(|group| {
            group
                .where_raw(RawSql::new("state = ?"), vec!["completed".into()])?
                .or_where_raw(RawSql::new("state = ?"), vec!["failed".into()])
        })?
        .where_compare("terminal_at_ms", WhereOperator::NotEq, Option::<i64>::None)?;
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
