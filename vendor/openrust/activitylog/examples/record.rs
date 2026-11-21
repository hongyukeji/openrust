use openrust::database::Connection;
use openrust_activitylog::{Activity, ActivityRepository, Redaction, Subject};
use serde_json::json;
async fn record(
    connection: Connection,
    authenticated_user_id: &str,
    request_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let logger = ActivityRepository::new(connection.clone()).withRedaction(
        Redaction::new()
            .logProperties(&["module"])
            .logOnly(&["name", "enabled"]),
    );
    let activity = Activity::new("settings.updated")
        .useLog("settings")
        .event("updated")
        .performedOn(Subject::new("settings", "1")?)
        .causedBy(Subject::new("user", authenticated_user_id)?)
        .withRequestId(request_id)
        .withProperties(json!({"module":"site"}))
        .withChanges(json!({"enabled":false}), json!({"enabled":true}));
    let prepared = logger.prepare(activity)?;
    connection
        .transaction(
            move |tx| {
                let prepared = prepared.clone();
                Box::pin(async move {
                    if let Some(record) = prepared {
                        record.logIn(tx).await?;
                    }
                    Ok(())
                })
            },
            1,
        )
        .await?;
    let page = logger
        .query()
        .inLog("settings")
        .forSubject(&Subject::new("settings", "1")?)
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
