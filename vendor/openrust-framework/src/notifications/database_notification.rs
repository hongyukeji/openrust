use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::database::{Value, query::Builder, schema::Blueprint};

use super::{NotificationResult, database_store, delivery};

/// The database key of a notifiable model.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum DatabaseKey {
    Unsigned(u64),
    Integer(i64),
    String(String),
}

impl DatabaseKey {
    pub(super) fn binding(&self) -> Value {
        match self {
            Self::Unsigned(key) => Value::Unsigned(*key),
            Self::Integer(key) => Value::Integer(*key),
            Self::String(key) => Value::Text(key.clone()),
        }
    }
}

/// The polymorphic notification relation of a model.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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
        let key = match model.get_key() {
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
        database_store::query(self)
    }

    pub async fn find(&self, id: &str) -> NotificationResult<Option<DatabaseNotification>> {
        database_store::find(self, id).await
    }
}

/// Represents a stored notification related to a notifiable model.
#[derive(Debug, Deserialize, Serialize)]
pub struct DatabaseNotification {
    pub id: String,
    pub kind: String,
    pub route: DatabaseRoute,
    pub data: Json,
    pub read_at: Option<Timestamp>,
}

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

    pub async fn mark_as_read(&mut self) -> NotificationResult<()> {
        if self.unread() {
            self.set_read_at(Some(now())).await?;
        }
        Ok(())
    }

    pub async fn mark_as_unread(&mut self) -> NotificationResult<()> {
        if !self.unread() {
            self.set_read_at(None).await?;
        }
        Ok(())
    }

    async fn set_read_at(&mut self, time: Option<Timestamp>) -> NotificationResult<()> {
        if database_store::set_read_at(self, time).await? {
            self.read_at = time;
        }
        Ok(())
    }

    pub(super) async fn insert(&self) -> NotificationResult<()> {
        database_store::insert(self).await
    }
}

pub(super) fn now() -> Timestamp {
    Timestamp::from_second(crate::support::timestamp_seconds(Timestamp::now()))
        .expect("current timestamp")
}
