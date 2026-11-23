//! 数据库通知的连接、行映射及持久化适配。
use super::database_notification::now;
use super::{DatabaseNotification, DatabaseRoute, NotificationError, NotificationResult, delivery};
use crate::database::{Db, Value, query::Builder};
use jiff::{Timestamp, civil::DateTime as CivilDateTime};

pub(super) fn query(route: &DatabaseRoute) -> NotificationResult<Builder<'static>> {
    Ok(table(route)?
        .r#where("notifiable_type", "=", route.model_type.as_str())
        .r#where("notifiable_id", "=", route.key.binding()))
}
fn table(route: &DatabaseRoute) -> NotificationResult<Builder<'static>> {
    Ok(Db::connection(route.connection.as_deref())
        .map_err(NotificationError::Database)?
        .table("notifications"))
}
pub(super) async fn find(
    route: &DatabaseRoute,
    id: &str,
) -> NotificationResult<Option<DatabaseNotification>> {
    let row = query(route)?
        .r#where("id", "=", id)
        .first()
        .await
        .map_err(NotificationError::Database)?;
    row.map(|row| {
        let text = |key| match row.get(key) {
            Some(Value::Text(text)) => Ok(text.clone()),
            _ => Err(delivery("Invalid database notification record.")),
        };
        let data = match row.get("data") {
            Some(Value::Json(json)) => json.clone(),
            Some(Value::Text(json)) => serde_json::from_str(json).map_err(delivery)?,
            _ => return Err(delivery("Invalid database notification data.")),
        };
        let read_at = match row.get("read_at") {
            None | Some(Value::Null) => None,
            Some(Value::Text(value)) => Some(
                CivilDateTime::strptime("%Y-%m-%d %H:%M:%S%.f", value)
                    .map_err(delivery)?
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .map_err(delivery)?
                    .timestamp(),
            ),
            _ => return Err(delivery("Invalid database notification timestamp.")),
        };
        Ok(DatabaseNotification {
            id: text("id")?,
            kind: text("type")?,
            route: route.clone(),
            data,
            read_at,
        })
    })
    .transpose()
}
pub(super) async fn set_read_at(
    record: &DatabaseNotification,
    time: Option<Timestamp>,
) -> NotificationResult<bool> {
    let query = query(&record.route)?.r#where("id", "=", record.id.as_str());
    let affected = query
        .update(
            [
                ("read_at".into(), timestamp(time)),
                ("updated_at".into(), timestamp(Some(now()))),
            ]
            .into(),
        )
        .await
        .map_err(NotificationError::Database)?;
    Ok(affected > 0)
}
pub(super) async fn insert(record: &DatabaseNotification) -> NotificationResult<()> {
    let time = timestamp(Some(now()));
    table(&record.route)?
        .insert(&[[
            ("id".into(), record.id.as_str().into()),
            ("type".into(), record.kind.as_str().into()),
            (
                "notifiable_type".into(),
                record.route.model_type.as_str().into(),
            ),
            ("notifiable_id".into(), record.route.key.binding()),
            ("data".into(), record.data.to_string().into()),
            ("read_at".into(), timestamp(record.read_at)),
            ("created_at".into(), time.clone()),
            ("updated_at".into(), time),
        ]
        .into()])
        .await
        .map_err(NotificationError::Database)?;
    Ok(())
}
fn timestamp(time: Option<Timestamp>) -> Value {
    time.map_or(Value::Null, |time| {
        time.strftime("%Y-%m-%d %H:%M:%S").to_string().into()
    })
}
