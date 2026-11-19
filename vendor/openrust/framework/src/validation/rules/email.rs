mod rfc;

use crate::validation::{Rule, rule::Kind};

/// Default RFC email validation. Warning-only RFC addresses remain valid.
#[derive(Clone, Default)]
pub struct Email {
    _private: (),
}

impl Email {
    pub fn new() -> Self {
        Self::default()
    }

    pub(in crate::validation) fn passes(value: &str) -> bool {
        // Laravel rejects header line breaks before invoking Egulias.
        !value.contains(['\r', '\n']) && rfc::passes(value)
    }
}

impl From<Email> for Rule {
    fn from(_: Email) -> Self {
        Self(Kind::Email)
    }
}
