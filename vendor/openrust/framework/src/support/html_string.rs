use std::fmt;

use serde::Serialize;

use crate::contracts::support::Htmlable;

/// An owned HTML string. Constructing it preserves the supplied markup.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct HtmlString(String);

#[allow(non_snake_case, reason = "Matches Laravel's HtmlString API")]
impl HtmlString {
    pub fn new(html: impl Into<String>) -> Self {
        Self(html.into())
    }

    pub fn toHtml(&self) -> &str {
        &self.0
    }

    pub fn isEmpty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn isNotEmpty(&self) -> bool {
        !self.isEmpty()
    }
}

impl Htmlable for HtmlString {
    fn toHtml(&self) -> &str {
        self.toHtml()
    }
}

impl fmt::Display for HtmlString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.toHtml())
    }
}
