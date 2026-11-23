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

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl MailFake {
    pub fn sent(&self, kind: &str) -> Vec<RecordedMail> {
        self.matching(|mail| mail.queue.is_none() && mail.kind == kind)
    }

    pub fn queued(&self, kind: &str) -> Vec<RecordedMail> {
        self.matching(|mail| mail.queue.is_some() && mail.kind == kind)
    }

    pub fn assertSent(&self, kind: &str) -> &Self {
        self.assertSentMatching(kind, |_| true)
    }

    pub fn assertSentMatching(
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

    pub fn assertQueued(&self, kind: &str) -> &Self {
        self.assertQueuedMatching(kind, |_| true)
    }

    pub fn assertQueuedMatching(
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

    pub fn assertSentTimes(&self, kind: &str, count: usize) -> &Self {
        assert_eq!(self.sent(kind).len(), count, "Sent count for [{kind}].");
        self
    }

    pub fn assertQueuedTimes(&self, kind: &str, count: usize) -> &Self {
        assert_eq!(self.queued(kind).len(), count, "Queued count for [{kind}].");
        self
    }

    pub fn assertSentOnce(&self, kind: &str) -> &Self {
        self.assertSentTimes(kind, 1)
    }

    pub fn assertQueuedOnce(&self, kind: &str) -> &Self {
        self.assertQueuedTimes(kind, 1)
    }

    pub fn assertNotSent(&self, kind: &str) -> &Self {
        self.assertSentTimes(kind, 0)
    }

    pub fn assertNotQueued(&self, kind: &str) -> &Self {
        self.assertQueuedTimes(kind, 0)
    }

    pub fn assertNotOutgoing(&self, kind: &str) -> &Self {
        self.assertNotSent(kind).assertNotQueued(kind)
    }

    pub fn assertSentCount(&self, count: usize) -> &Self {
        assert_eq!(
            self.count(|mail| mail.queue.is_none()),
            count,
            "Total sent mail."
        );
        self
    }

    pub fn assertQueuedCount(&self, count: usize) -> &Self {
        assert_eq!(
            self.count(|mail| mail.queue.is_some()),
            count,
            "Total queued mail."
        );
        self
    }

    pub fn assertOutgoingCount(&self, count: usize) -> &Self {
        assert_eq!(self.count(|_| true), count, "Total outgoing mail.");
        self
    }

    pub fn assertNothingSent(&self) -> &Self {
        self.assertSentCount(0)
    }

    pub fn assertNothingQueued(&self) -> &Self {
        self.assertQueuedCount(0)
    }

    pub fn assertNothingOutgoing(&self) -> &Self {
        self.assertOutgoingCount(0)
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
