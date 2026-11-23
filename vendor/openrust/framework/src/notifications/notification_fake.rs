use std::{collections::BTreeMap, sync::Mutex};

use serde_json::Value;

use super::{DatabaseRoute, NotificationRoute};

/// The identity available to a notification assertion.
#[derive(Clone, PartialEq)]
pub enum NotificationTarget {
    Database(DatabaseRoute),
    Key(String),
    OnDemand(BTreeMap<String, NotificationRoute>),
    Unkeyed,
}

impl From<DatabaseRoute> for NotificationTarget {
    fn from(route: DatabaseRoute) -> Self {
        Self::Database(route)
    }
}

/// A notification dispatch recorded before entering any delivery channel.
#[derive(Clone)]
pub struct RecordedNotification {
    pub id: String,
    pub kind: String,
    pub channels: Vec<String>,
    pub locale: Option<String>,
    pub target: NotificationTarget,
    pub data: Value,
}

#[derive(Default)]
pub struct NotificationFake {
    records: Mutex<Vec<RecordedNotification>>,
}

impl NotificationFake {
    pub fn sent_notifications(&self) -> Vec<RecordedNotification> {
        self.matching(|_| true)
    }

    pub fn sent(&self, target: &NotificationTarget, kind: &str) -> Vec<RecordedNotification> {
        self.matching(|notice| &notice.target == target && notice.kind == kind)
    }

    pub fn assert_sent_to(&self, target: &NotificationTarget, kind: &str) -> &Self {
        self.assert_sent_to_matching(target, kind, |_| true)
    }

    pub fn assert_sent_to_matching(
        &self,
        target: &NotificationTarget,
        kind: &str,
        predicate: impl Fn(&RecordedNotification) -> bool,
    ) -> &Self {
        assert!(
            self.sent(target, kind).iter().any(predicate),
            "Notification [{kind}] was not sent to the recipient."
        );
        self
    }

    pub fn assert_sent_to_times(
        &self,
        target: &NotificationTarget,
        kind: &str,
        count: usize,
    ) -> &Self {
        assert_eq!(
            self.sent(target, kind).len(),
            count,
            "Count for notification [{kind}] to the recipient."
        );
        self
    }

    pub fn assert_sent_to_once(&self, target: &NotificationTarget, kind: &str) -> &Self {
        self.assert_sent_to_times(target, kind, 1)
    }

    pub fn assert_not_sent_to(&self, target: &NotificationTarget, kind: &str) -> &Self {
        self.assert_sent_to_times(target, kind, 0)
    }

    pub fn assert_nothing_sent_to(&self, target: &NotificationTarget) -> &Self {
        assert!(
            self.matching(|notice| &notice.target == target).is_empty(),
            "Unexpected notifications for the recipient."
        );
        self
    }

    pub fn assert_sent_on_demand(&self, kind: &str) -> &Self {
        self.assert_sent_on_demand_matching(kind, |_| true)
    }

    pub fn assert_sent_on_demand_matching(
        &self,
        kind: &str,
        predicate: impl Fn(&RecordedNotification) -> bool,
    ) -> &Self {
        assert!(
            self.on_demand(kind).iter().any(predicate),
            "Notification [{kind}] was not sent on demand."
        );
        self
    }

    pub fn assert_sent_on_demand_times(&self, kind: &str, count: usize) -> &Self {
        assert_eq!(
            self.on_demand(kind).len(),
            count,
            "On-demand count for notification [{kind}]."
        );
        self
    }

    pub fn assert_sent_on_demand_once(&self, kind: &str) -> &Self {
        self.assert_sent_on_demand_times(kind, 1)
    }

    pub fn assert_sent_times(&self, kind: &str, count: usize) -> &Self {
        assert_eq!(
            self.matching(|notice| notice.kind == kind).len(),
            count,
            "Total count for notification [{kind}]."
        );
        self
    }

    pub fn assert_count(&self, count: usize) -> &Self {
        assert_eq!(
            self.records
                .lock()
                .expect("notification fake lock poisoned")
                .len(),
            count,
            "Total notifications."
        );
        self
    }

    pub fn assert_nothing_sent(&self) -> &Self {
        self.assert_count(0)
    }

    pub(super) fn record(&self, notice: RecordedNotification) {
        self.records
            .lock()
            .expect("notification fake lock poisoned")
            .push(notice);
    }

    fn on_demand(&self, kind: &str) -> Vec<RecordedNotification> {
        self.matching(|notice| {
            matches!(notice.target, NotificationTarget::OnDemand(_)) && notice.kind == kind
        })
    }

    fn matching(
        &self,
        predicate: impl Fn(&RecordedNotification) -> bool,
    ) -> Vec<RecordedNotification> {
        self.records
            .lock()
            .expect("notification fake lock poisoned")
            .iter()
            .filter(|notice| predicate(notice))
            .cloned()
            .collect()
    }
}
