use openrust::database::Connection;
use openrust_activitylog::{Activity, ActivityRepository, Redaction, Subject};
use serde_json::json;
async fn record(
    connection: Connection,
    authenticated_user_id: &str,
    request_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let logger = ActivityRepository::new(connection.clone()).with_redaction(
        Redaction::new()
            .log_properties(&["module"])
            .log_only(&["name", "enabled"]),
    );
    let activity = Activity::new("settings.updated")
        .use_log("settings")
        .event("updated")
        .performed_on(Subject::new("settings", "1")?)
        .caused_by(Subject::new("user", authenticated_user_id)?)
        .with_request_id(request_id)
        .with_properties(json!({"module":"site"}))
        .with_changes(json!({"enabled":false}), json!({"enabled":true}));
    let prepared = logger.prepare(activity)?;
    connection
        .transaction(
            move |tx| {
                let prepared = prepared.clone();
                Box::pin(async move {
                    if let Some(record) = prepared {
                        record.log_in(tx).await?;
                    }
                    Ok(())
                })
            },
            1,
        )
        .await?;
    let page = logger
        .query()
        .in_log("settings")
        .for_subject(&Subject::new("settings", "1")?)
        .limit(50)
        .get()
        .await?;
    if let Some(last) = page.last() {
        let _next = logger
            .query()
            .before(last.id.unwrap())
            .limit(50)
            .get()
            .await?;
    }
    Ok(())
}
fn main() {
    let _ = record;
}
