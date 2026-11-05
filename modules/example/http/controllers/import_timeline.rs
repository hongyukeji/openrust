//! 认证账户导入时间线 API；账户与观察时点均由服务器确定。

use crate::modules::example::services::import_timeline::{self, ImportTimeline};
use openrust::auth::CurrentUser;
use openrust::http::{Inject, Json};
use openrust::{Db, Result};

pub(crate) async fn show(
    user: CurrentUser,
    Inject(db): Inject<Db>,
) -> Result<Json<ImportTimeline>> {
    let observed_at_ms = chrono::Utc::now().timestamp_millis();
    Ok(Json(
        import_timeline::report(&db, user.integer_id()?, observed_at_ms).await?,
    ))
}
