use std::sync::{Arc, Mutex, RwLock};

use crate::queue::DispatchOptions;

use super::{SentMessage, mailables::Envelope};

pub(super) type FakeState = Arc<RwLock<Option<Arc<MailFake>>>>;

/// A mail dispatch captured by the application's fake.
#[derive(Clone)]
pub struct RecordedMail {
    pub kind: String,
    pub mailer: String,
    pub envelope: Envelope,
    pub locale: Option<String>,
    pub message: Option<SentMessage>,
    pub queue: Option<DispatchOptions>,
}

/// Records immediate and queued mail without delivering it.
#[derive(Default)]
pub struct MailFake {
    records: Mutex<Vec<RecordedMail>>,
}

impl MailFake {
    pub fn sent(&self, kind: &str) -> Vec<RecordedMail> {
        self.matching(|mail| mail.queue.is_none() && mail.kind == kind)
    }

    pub fn queued(&self, kind: &str) -> Vec<RecordedMail> {
        self.matching(|mail| mail.queue.is_some() && mail.kind == kind)
    }

    pub fn assert_sent(&self, kind: &str) -> &Self {
        self.assert_sent_matching(kind, |_| true)
    }

    pub fn assert_sent_matching(
        &self,
        kind: &str,
        predicate: impl Fn(&RecordedMail) -> bool,
    ) -> &Self {
        assert!(
            self.sent(kind).iter().any(predicate),
            "Mailable [{kind}] was not sent."
        );
        self
    }

    pub fn assert_queued(&self, kind: &str) -> &Self {
        self.assert_queued_matching(kind, |_| true)
    }

    pub fn assert_queued_matching(
        &self,
        kind: &str,
        predicate: impl Fn(&RecordedMail) -> bool,
    ) -> &Self {
        assert!(
            self.queued(kind).iter().any(predicate),
            "Mailable [{kind}] was not queued."
        );
        self
    }

    pub fn assert_sent_times(&self, kind: &str, count: usize) -> &Self {
        assert_eq!(self.sent(kind).len(), count, "Sent count for [{kind}].");
        self
    }

    pub fn assert_queued_times(&self, kind: &str, count: usize) -> &Self {
        assert_eq!(self.queued(kind).len(), count, "Queued count for [{kind}].");
        self
    }

    pub fn assert_sent_once(&self, kind: &str) -> &Self {
        self.assert_sent_times(kind, 1)
    }

    pub fn assert_queued_once(&self, kind: &str) -> &Self {
        self.assert_queued_times(kind, 1)
    }

    pub fn assert_not_sent(&self, kind: &str) -> &Self {
        self.assert_sent_times(kind, 0)
    }

    pub fn assert_not_queued(&self, kind: &str) -> &Self {
        self.assert_queued_times(kind, 0)
    }

    pub fn assert_not_outgoing(&self, kind: &str) -> &Self {
        self.assert_not_sent(kind).assert_not_queued(kind)
    }

    pub fn assert_sent_count(&self, count: usize) -> &Self {
        assert_eq!(
            self.count(|mail| mail.queue.is_none()),
            count,
            "Total sent mail."
        );
        self
    }

    pub fn assert_queued_count(&self, count: usize) -> &Self {
        assert_eq!(
            self.count(|mail| mail.queue.is_some()),
            count,
            "Total queued mail."
        );
        self
    }

    pub fn assert_outgoing_count(&self, count: usize) -> &Self {
        assert_eq!(self.count(|_| true), count, "Total outgoing mail.");
        self
    }

    pub fn assert_nothing_sent(&self) -> &Self {
        self.assert_sent_count(0)
    }

    pub fn assert_nothing_queued(&self) -> &Self {
        self.assert_queued_count(0)
    }

    pub fn assert_nothing_outgoing(&self) -> &Self {
        self.assert_outgoing_count(0)
    }

    pub(super) fn record(&self, mail: RecordedMail) {
        self.records
            .lock()
            .expect("mail fake lock poisoned")
            .push(mail);
    }

    fn matching(&self, predicate: impl Fn(&RecordedMail) -> bool) -> Vec<RecordedMail> {
        self.records
            .lock()
            .expect("mail fake lock poisoned")
            .iter()
            .filter(|mail| predicate(mail))
            .cloned()
            .collect()
    }

    fn count(&self, predicate: impl Fn(&RecordedMail) -> bool) -> usize {
        self.records
            .lock()
            .expect("mail fake lock poisoned")
            .iter()
            .filter(|mail| predicate(mail))
            .count()
    }
}
