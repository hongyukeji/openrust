use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::database::{DB, Value, query::Builder, schema::Blueprint};

use super::{NotificationError, NotificationResult, delivery};

/// The database key of a notifiable model.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DatabaseKey {
    Unsigned(u64),
    Integer(i64),
    String(String),
}

impl DatabaseKey {
    fn binding(&self) -> Value {
        match self {
            Self::Unsigned(key) => Value::Unsigned(*key),
            Self::Integer(key) => Value::Integer(*key),
            Self::String(key) => Value::Text(key.clone()),
        }
    }
}

/// The polymorphic notification relation of a model.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DatabaseRoute {
    pub connection: Option<String>,
    pub model_type: String,
    pub key: DatabaseKey,
}

impl DatabaseRoute {
    pub fn for_model(
        model: &impl crate::database::eloquent::Model,
        model_type: impl Into<String>,
        connection: Option<String>,
    ) -> NotificationResult<Self> {
        let key = match model.getKey() {
            Some(Value::Unsigned(key)) => DatabaseKey::Unsigned(*key),
            Some(Value::Integer(key)) => DatabaseKey::Integer(*key),
            Some(Value::Text(key)) => DatabaseKey::String(key.clone()),
            _ => {
                return Err(delivery(
                    "The notifiable model has no supported persisted key.",
                ));
            }
        };
        Ok(Self {
            connection,
            model_type: model_type.into(),
            key,
        })
    }

    pub fn query(&self) -> NotificationResult<Builder<'static>> {
        Ok(self
            .table()?
            .r#where("notifiable_type", "=", self.model_type.as_str())
            .r#where("notifiable_id", "=", self.key.binding()))
    }

    fn table(&self) -> NotificationResult<Builder<'static>> {
        Ok(DB::connection(self.connection.as_deref())
            .map_err(NotificationError::Database)?
            .table("notifications"))
    }

    pub async fn find(&self, id: &str) -> NotificationResult<Option<DatabaseNotification>> {
        let row = self
            .query()?
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
                    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
                        .map_err(delivery)?
                        .and_utc(),
                ),
                _ => return Err(delivery("Invalid database notification timestamp.")),
            };
            Ok(DatabaseNotification {
                id: text("id")?,
                kind: text("type")?,
                route: self.clone(),
                data,
                read_at,
            })
        })
        .transpose()
    }
}

/// Represents a stored notification related to a notifiable model.
#[derive(Debug, Deserialize, Serialize)]
pub struct DatabaseNotification {
    pub id: String,
    pub kind: String,
    pub route: DatabaseRoute,
    pub data: Json,
    pub read_at: Option<DateTime<Utc>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's DatabaseNotification API")]
impl DatabaseNotification {
    /// Defines the table while allowing the application to select the model key column type.
    pub fn schema(table: &mut Blueprint, key_column: impl FnOnce(&mut Blueprint)) {
        table.uuid("id");
        table.primary(&["id"], None);
        table.string("type", 255);
        table.string("notifiable_type", 255);
        key_column(table);
        table.index(&["notifiable_type", "notifiable_id"], None);
        table.text("data");
        table.timestamp("read_at", 0).nullable();
        table.timestamps();
    }

    pub fn unread(&self) -> bool {
        self.read_at.is_none()
    }

    pub async fn markAsRead(&mut self) -> NotificationResult<()> {
        if self.unread() {
            self.set_read_at(Some(now())).await?;
        }
        Ok(())
    }

    pub async fn markAsUnread(&mut self) -> NotificationResult<()> {
        if !self.unread() {
            self.set_read_at(None).await?;
        }
        Ok(())
    }

    async fn set_read_at(&mut self, time: Option<DateTime<Utc>>) -> NotificationResult<()> {
        let query = self.route.query()?.r#where("id", "=", self.id.as_str());
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
        if affected > 0 {
            self.read_at = time;
        }
        Ok(())
    }

    pub(super) async fn insert(&self) -> NotificationResult<()> {
        let time = timestamp(Some(now()));
        self.route
            .table()?
            .insert(&[[
                ("id".into(), self.id.as_str().into()),
                ("type".into(), self.kind.as_str().into()),
                (
                    "notifiable_type".into(),
                    self.route.model_type.as_str().into(),
                ),
                ("notifiable_id".into(), self.route.key.binding()),
                ("data".into(), self.data.to_string().into()),
                ("read_at".into(), timestamp(self.read_at)),
                ("created_at".into(), time.clone()),
                ("updated_at".into(), time),
            ]
            .into()])
            .await
            .map_err(NotificationError::Database)?;
        Ok(())
    }
}

fn now() -> DateTime<Utc> {
    let time = DateTime::<Utc>::from(std::time::SystemTime::now());
    DateTime::from_timestamp(time.timestamp(), 0).expect("current timestamp")
}

fn timestamp(time: Option<DateTime<Utc>>) -> Value {
    time.map_or(Value::Null, |time| {
        time.format("%Y-%m-%d %H:%M:%S").to_string().into()
    })
}
