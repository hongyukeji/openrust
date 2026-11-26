use serde::Serialize;

use crate::{
    database::{Connection, DatabaseError, Value},
    support::facades::Db,
};

use super::{
    NotificationError, NotificationResult, channels::ChannelResponse, delivery, sms::SmsFailure,
    sms::SmsFailureKind,
};

const TABLE: &str = "sms_deliveries";

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Status {
    Submitting,
    Accepted,
    Retryable,
    Rejected,
    Unknown,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Submitting => "submitting",
            Self::Accepted => "accepted",
            Self::Retryable => "retryable",
            Self::Rejected => "rejected",
            Self::Unknown => "unknown",
        }
    }
}

pub(super) struct SmsDelivery<'a> {
    connection: Connection,
    id: &'a str,
}

impl<'a> SmsDelivery<'a> {
    pub(super) fn new(id: &'a str) -> NotificationResult<Self> {
        Ok(Self {
            connection: Db::connection(None).map_err(NotificationError::Database)?,
            id,
        })
    }

    pub(super) async fn status(&self) -> NotificationResult<Option<Status>> {
        let value = self
            .row()
            .value("status")
            .await
            .map_err(NotificationError::Database)?;
        match value {
            Value::Null => Ok(None),
            Value::Text(status) => match status.as_str() {
                "submitting" => Ok(Some(Status::Submitting)),
                "accepted" => Ok(Some(Status::Accepted)),
                "retryable" => Ok(Some(Status::Retryable)),
                "rejected" => Ok(Some(Status::Rejected)),
                "unknown" => Ok(Some(Status::Unknown)),
                _ => Err(delivery("The stored SMS delivery status is invalid.")),
            },
            _ => Err(delivery("The stored SMS delivery status is invalid.")),
        }
    }

    /// The primary key and conditional update let exactly one worker enter a given attempt.
    pub(super) async fn claim(&self, previous: Option<Status>) -> NotificationResult<bool> {
        let now = crate::support::timestamp_seconds(jiff::Timestamp::now());
        if previous == Some(Status::Retryable) {
            let changed = self
                .row()
                .r#where("status", "=", Status::Retryable.as_str())
                .update(
                    [
                        ("status".into(), Status::Submitting.as_str().into()),
                        ("submission".into(), Value::Null),
                        ("updated_at".into(), now.into()),
                    ]
                    .into_iter()
                    .collect(),
                )
                .await
                .map_err(NotificationError::Database)?;
            return Ok(changed == 1);
        }

        let row = [
            ("id".into(), self.id.into()),
            ("status".into(), Status::Submitting.as_str().into()),
            ("created_at".into(), now.into()),
            ("updated_at".into(), now.into()),
        ]
        .into_iter()
        .collect();
        match self.connection.table(TABLE).insert(&[row]).await {
            Ok(inserted) => Ok(inserted),
            Err(DatabaseError::Driver { source, .. })
                if source
                    .as_database_error()
                    .is_some_and(|error| error.is_unique_violation()) =>
            {
                Ok(false)
            }
            Err(error) => Err(NotificationError::Database(error)),
        }
    }

    pub(super) async fn finish(
        &self,
        result: &NotificationResult<Option<ChannelResponse>>,
    ) -> NotificationResult<Status> {
        #[derive(Serialize)]
        struct Failure<'a> {
            code: &'a str,
        }

        let (status, submission) = match result {
            Ok(Some(ChannelResponse::Sms(submission))) => (
                Status::Accepted,
                serde_json::to_string(submission)
                    .map(Value::from)
                    .map_err(delivery)?,
            ),
            Ok(_) => (Status::Accepted, Value::Null),
            Err(error) => {
                let failure = SmsFailure::from_notification(error);
                let status = match failure.map(|failure| failure.kind) {
                    Some(SmsFailureKind::Retryable) => Status::Retryable,
                    Some(SmsFailureKind::Rejected) => Status::Rejected,
                    _ => Status::Unknown,
                };
                let code = failure.and_then(|failure| failure.code.as_deref());
                let submission = match code {
                    Some(code) => serde_json::to_string(&Failure { code })
                        .map(Value::from)
                        .map_err(delivery)?,
                    None => Value::Null,
                };
                (status, submission)
            }
        };
        let changed = self
            .row()
            .r#where("status", "=", Status::Submitting.as_str())
            .update(
                [
                    ("status".into(), status.as_str().into()),
                    ("submission".into(), submission),
                    (
                        "updated_at".into(),
                        crate::support::timestamp_seconds(jiff::Timestamp::now()).into(),
                    ),
                ]
                .into_iter()
                .collect(),
            )
            .await
            .map_err(NotificationError::Database)?;
        if changed != 1 {
            return Err(delivery("Could not confirm the stored SMS delivery state."));
        }
        Ok(status)
    }

    fn row(&self) -> crate::database::query::Builder<'static> {
        self.connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", self.id)
    }
}
