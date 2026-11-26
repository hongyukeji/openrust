/// Whether a failed submission may be attempted again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmsFailureKind {
    Rejected,
    Retryable,
    SubmissionUnknown,
}

#[derive(Debug)]
pub struct SmsFailure {
    pub kind: SmsFailureKind,
    pub code: Option<String>,
    reason: &'static str,
}

impl std::fmt::Display for SmsFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason)?;
        if let Some(code) = &self.code {
            write!(f, " ({code})")?;
        }
        Ok(())
    }
}

impl std::error::Error for SmsFailure {}

impl SmsFailure {
    pub fn new(kind: SmsFailureKind, code: Option<&str>, reason: &'static str) -> Self {
        Self {
            kind,
            code: code
                .filter(|code| {
                    !code.is_empty()
                        && code.len() <= 80
                        && code.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')
                        })
                })
                .map(str::to_owned),
            reason,
        }
    }

    pub fn from_notification(error: &crate::notifications::NotificationError) -> Option<&Self> {
        match error {
            crate::notifications::NotificationError::Delivery { source } => {
                source.downcast_ref::<Self>()
            }
            _ => None,
        }
    }
}
