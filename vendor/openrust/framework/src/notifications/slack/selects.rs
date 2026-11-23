use indexmap::IndexMap;
use serde::Serialize;

use crate::notifications::{NotificationResult, delivery};

use super::blocks::{PlainText, TextObject};

#[derive(Serialize)]
struct SelectElement {
    action_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    placeholder: Option<PlainText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus_on_load: Option<bool>,
}

impl SelectElement {
    fn new(prefix: &str) -> Self {
        Self {
            action_id: format!("{prefix}{}", uuid::Uuid::now_v7().simple()),
            placeholder: None,
            focus_on_load: None,
        }
    }
}

/// A menu whose options are defined in the message.
#[derive(Serialize)]
pub struct StaticSelectElement {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(flatten)]
    common: SelectElement,
    #[serde(serialize_with = "serialize_options")]
    options: IndexMap<String, SelectOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    initial_option: Option<SelectOption>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl StaticSelectElement {
    pub(super) fn new(placeholder: impl Into<String>) -> Self {
        let mut menu = Self {
            kind: "static_select",
            common: SelectElement::new("static_select_"),
            options: IndexMap::new(),
            initial_option: None,
        };
        menu.placeholder(placeholder);
        menu
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.common.action_id = id.into();
        self
    }

    pub fn placeholder(&mut self, text: impl Into<String>) -> &mut Self {
        self.common.placeholder = Some(PlainText::new(text, 3000));
        self
    }

    pub fn focus(&mut self, enable: bool) -> &mut Self {
        self.common.focus_on_load = Some(enable);
        self
    }

    pub fn addOption(&mut self, text: impl Into<String>, value: impl Into<String>) -> &mut Self {
        let value = value.into();
        let option = SelectOption {
            text: TextObject::with_limit(text, 75),
            value: value
                .to_lowercase()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                .collect(),
        };
        self.options.insert(value, option);
        self
    }

    pub fn initialOption(&mut self, value: &str) -> NotificationResult<&mut Self> {
        self.initial_option = Some(
            self.options
                .get(value)
                .ok_or_else(|| delivery("The Slack menu initial option is unknown."))?
                .clone(),
        );
        Ok(self)
    }
}

/// A menu populated by Slack with users from the workspace.
#[derive(Serialize)]
pub struct UsersSelectElement {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(flatten)]
    common: SelectElement,
    #[serde(skip_serializing_if = "Option::is_none")]
    initial_user: Option<String>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl UsersSelectElement {
    pub(super) fn new(placeholder: impl Into<String>) -> Self {
        let mut menu = Self {
            kind: "users_select",
            common: SelectElement::new("users_select_"),
            initial_user: None,
        };
        menu.placeholder(placeholder);
        menu
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.common.action_id = id.into();
        self
    }

    pub fn placeholder(&mut self, text: impl Into<String>) -> &mut Self {
        self.common.placeholder = Some(PlainText::new(text, 3000));
        self
    }

    pub fn focus(&mut self, enable: bool) -> &mut Self {
        self.common.focus_on_load = Some(enable);
        self
    }

    pub fn initialUser(&mut self, value: impl Into<String>) -> &mut Self {
        self.initial_user = Some(value.into());
        self
    }
}

#[derive(Clone, Serialize)]
struct SelectOption {
    text: TextObject,
    value: String,
}

fn serialize_options<S: serde::Serializer>(
    options: &IndexMap<String, SelectOption>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.collect_seq(options.values())
}
