use serde::Serialize;
use serde_json::{Value, json};

use crate::support::Str;

pub(super) fn plain_text(text: impl Into<String>) -> Value {
    json!({"type":"plain_text","text":text.into()})
}

#[derive(Serialize)]
pub struct TextObject {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    verbatim: Option<bool>,
}

impl TextObject {
    fn new(text: impl Into<String>) -> Self {
        Self {
            kind: "plain_text",
            text: text.into(),
            verbatim: None,
        }
    }

    pub fn markdown(&mut self) -> &mut Self {
        self.kind = "mrkdwn";
        self
    }

    pub fn verbatim(&mut self) -> &mut Self {
        self.verbatim = Some(true);
        self
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum ContextElement {
    Text(TextObject),
    Image(Value),
}

/// Text or images grouped in a Slack context block.
#[derive(Default)]
pub struct ContextBlock {
    elements: Vec<ContextElement>,
    id: Option<String>,
}

impl ContextBlock {
    pub fn text(&mut self, text: impl Into<String>) -> &mut TextObject {
        self.elements
            .push(ContextElement::Text(TextObject::new(text)));
        let Some(ContextElement::Text(element)) = self.elements.last_mut() else {
            unreachable!("inserted a text element")
        };
        element
    }

    pub fn image(&mut self, url: impl Into<String>, alt: impl Into<String>) -> &mut Self {
        self.elements.push(ContextElement::Image(
            json!({"type":"image","image_url":url.into(),"alt_text":alt.into()}),
        ));
        self
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.id = Some(id.into());
        self
    }

    pub(super) fn into_json(self) -> Value {
        with_id(json!({"type":"context","elements":self.elements}), self.id)
    }
}

/// Slack formatted text, fields, and an optional accessory.
#[derive(Default)]
pub struct SectionBlock {
    text: Option<TextObject>,
    fields: Vec<TextObject>,
    accessory: Option<Value>,
    id: Option<String>,
}

impl SectionBlock {
    pub fn text(&mut self, text: impl Into<String>) -> &mut TextObject {
        self.text.insert(TextObject::new(text))
    }

    pub fn field(&mut self, text: impl Into<String>) -> &mut TextObject {
        self.fields.push(TextObject::new(text));
        self.fields.last_mut().expect("inserted a field")
    }

    pub fn accessory(&mut self, accessory: Value) -> &mut Self {
        self.accessory = Some(accessory);
        self
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.id = Some(id.into());
        self
    }

    pub(super) fn into_json(self) -> Value {
        let mut block = json!({"type":"section"});
        if let Some(text) = self.text {
            block["text"] = json!(text);
        }
        if !self.fields.is_empty() {
            block["fields"] = json!(self.fields);
        }
        if let Some(accessory) = self.accessory {
            block["accessory"] = accessory;
        }
        with_id(block, self.id)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum Action {
    Button(ButtonElement),
    Custom(Value),
}

/// Slack buttons or other Block Kit action elements.
#[derive(Default)]
pub struct ActionsBlock {
    elements: Vec<Action>,
    id: Option<String>,
}

impl ActionsBlock {
    pub fn button(&mut self, text: impl Into<String>) -> &mut ButtonElement {
        let text = text.into();
        let id = format!("button_{}", Str::slug(&text).replace('-', "_"));
        self.elements
            .push(Action::Button(ButtonElement::new(text, id)));
        let Some(Action::Button(element)) = self.elements.last_mut() else {
            unreachable!("inserted a button")
        };
        element
    }

    pub fn element(&mut self, element: Value) -> &mut Self {
        self.elements.push(Action::Custom(element));
        self
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.id = Some(id.into());
        self
    }

    pub(super) fn into_json(self) -> Value {
        with_id(json!({"type":"actions","elements":self.elements}), self.id)
    }
}

#[derive(Serialize)]
pub struct ButtonElement {
    #[serde(rename = "type")]
    kind: &'static str,
    text: Value,
    action_id: String,
    #[serde(flatten)]
    options: serde_json::Map<String, Value>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Slack button API")]
impl ButtonElement {
    fn new(text: String, action_id: String) -> Self {
        Self {
            kind: "button",
            text: plain_text(text),
            action_id,
            options: Default::default(),
        }
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.action_id = id.into();
        self
    }

    pub fn primary(&mut self) -> &mut Self {
        self.options.insert("style".into(), "primary".into());
        self
    }

    pub fn danger(&mut self) -> &mut Self {
        self.options.insert("style".into(), "danger".into());
        self
    }

    pub fn url(&mut self, url: impl Into<String>) -> &mut Self {
        self.options.insert("url".into(), url.into().into());
        self
    }

    pub fn value(&mut self, value: impl Into<String>) -> &mut Self {
        self.options.insert("value".into(), value.into().into());
        self
    }

    pub fn accessibilityLabel(&mut self, label: impl Into<String>) -> &mut Self {
        self.options
            .insert("accessibility_label".into(), label.into().into());
        self
    }

    pub fn confirm(
        &mut self,
        text: impl Into<String>,
        build: impl FnOnce(&mut ConfirmObject),
    ) -> &mut Self {
        let mut dialog = ConfirmObject(json!({
            "title":plain_text("Are you sure?"),"text":plain_text(text),
            "confirm":plain_text("Yes"),"deny":plain_text("No"),
        }));
        build(&mut dialog);
        self.options.insert("confirm".into(), dialog.0);
        self
    }
}

/// Slack's confirmation dialog for an interactive element.
pub struct ConfirmObject(Value);

impl ConfirmObject {
    pub fn title(&mut self, title: impl Into<String>) -> &mut Self {
        self.0["title"] = plain_text(title);
        self
    }

    pub fn confirm(&mut self, label: impl Into<String>) -> &mut Self {
        self.0["confirm"] = plain_text(label);
        self
    }

    pub fn deny(&mut self, label: impl Into<String>) -> &mut Self {
        self.0["deny"] = plain_text(label);
        self
    }

    pub fn danger(&mut self) -> &mut Self {
        self.0["style"] = "danger".into();
        self
    }
}

fn with_id(mut block: Value, id: Option<String>) -> Value {
    if let Some(id) = id {
        block["block_id"] = id.into();
    }
    block
}
