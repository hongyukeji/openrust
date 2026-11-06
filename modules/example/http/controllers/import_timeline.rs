//! 认证账户导入时间线 API；账户与观察时点均由服务器确定。

use crate::modules::example::services::import_timeline::{self, ImportSearchMode, ImportTimeline};
use openrust::auth::CurrentUser;
use openrust::http::{Inject, Json, Query};
use openrust::{Db, Result};

/// 可选的导入标识／来源名称检索；缺省不改变时间线范围。
#[derive(serde::Deserialize)]
pub(crate) struct TimelineQuery {
    search: Option<String>,
    #[serde(default)]
    mode: ImportSearchMode,
}

pub(crate) async fn show(
    user: CurrentUser,
    Inject(db): Inject<Db>,
    Query(query): Query<TimelineQuery>,
) -> Result<Json<ImportTimeline>> {
    let observed_at_ms = chrono::Utc::now().timestamp_millis();
    let report = if query.search.is_none() {
        import_timeline::report(&db, user.integer_id()?, observed_at_ms).await?
    } else {
        import_timeline::report_filtered(
            &db,
            user.integer_id()?,
            observed_at_ms,
            query.search.as_deref(),
            query.mode,
        )
        .await?
    };
    Ok(Json(report))
}
