use serde::{Serialize, ser::SerializeMap};
use serde_json::{Value, json};

use crate::support::Str;

use super::selects::{StaticSelectElement, UsersSelectElement};

fn truncate(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit - 3;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("...");
    }
    text
}

#[derive(Serialize)]
pub struct PlainText {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    emoji: Option<bool>,
}

impl PlainText {
    pub(super) fn new(text: impl Into<String>, limit: usize) -> Self {
        Self {
            kind: "plain_text",
            text: truncate(text.into(), limit),
            emoji: None,
        }
    }

    pub fn emoji(&mut self) -> &mut Self {
        self.emoji = Some(true);
        self
    }
}

#[derive(Clone)]
pub struct TextObject {
    kind: &'static str,
    text: String,
    verbatim: bool,
    emoji: bool,
}

impl TextObject {
    fn new(text: impl Into<String>) -> Self {
        Self::with_limit(text, 3000)
    }

    pub(super) fn with_limit(text: impl Into<String>, limit: usize) -> Self {
        Self {
            kind: "plain_text",
            text: truncate(text.into(), limit),
            verbatim: false,
            emoji: false,
        }
    }

    pub fn markdown(&mut self) -> &mut Self {
        self.kind = "mrkdwn";
        self
    }

    pub fn verbatim(&mut self) -> &mut Self {
        self.verbatim = true;
        self
    }

    pub fn emoji(&mut self) -> &mut Self {
        self.emoji = true;
        self
    }
}

impl Serialize for TextObject {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("type", self.kind)?;
        map.serialize_entry("text", &self.text)?;
        match self.kind {
            "plain_text" if self.emoji => map.serialize_entry("emoji", &true)?,
            "mrkdwn" if self.verbatim => map.serialize_entry("verbatim", &true)?,
            _ => {}
        }
        map.end()
    }
}

/// A plain-text section heading.
pub struct HeaderBlock {
    text: PlainText,
    id: Option<String>,
}

impl HeaderBlock {
    pub(super) fn new(text: impl Into<String>) -> Self {
        Self {
            text: PlainText::new(text, 150),
            id: None,
        }
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.id = Some(id.into());
        self
    }

    pub fn text(&mut self) -> &mut PlainText {
        &mut self.text
    }

    pub(super) fn into_json(self) -> Value {
        with_id(json!({"type":"header","text":self.text}), self.id)
    }
}

/// A horizontal divider with an optional identifier.
#[derive(Default)]
pub struct DividerBlock {
    id: Option<String>,
}

impl DividerBlock {
    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.id = Some(id.into());
        self
    }

    pub(super) fn into_json(self) -> Value {
        with_id(json!({"type":"divider"}), self.id)
    }
}

/// A standalone image with alternative text and an optional title.
pub struct ImageBlock {
    image: ImageElement,
    title: Option<PlainText>,
    id: Option<String>,
}

impl ImageBlock {
    pub(super) fn new(url: impl Into<String>) -> Self {
        Self {
            image: ImageElement::new(url),
            title: None,
            id: None,
        }
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.id = Some(id.into());
        self
    }

    pub fn alt(&mut self, text: impl Into<String>) -> &mut Self {
        self.image.alt(text);
        self
    }

    pub fn title(&mut self, text: impl Into<String>) -> &mut PlainText {
        self.title.insert(PlainText::new(text, 2000))
    }

    pub(super) fn into_json(self) -> Value {
        let mut block = json!(self.image);
        if let Some(title) = self.title {
            block["title"] = json!(title);
        }
        with_id(block, self.id)
    }
}

/// An image displayed in a context block.
#[derive(Serialize)]
pub struct ImageElement {
    #[serde(rename = "type")]
    kind: &'static str,
    image_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    alt_text: Option<String>,
}

impl ImageElement {
    fn new(url: impl Into<String>) -> Self {
        Self {
            kind: "image",
            image_url: url.into(),
            alt_text: None,
        }
    }

    pub fn alt(&mut self, text: impl Into<String>) -> &mut Self {
        self.alt_text = Some(text.into());
        self
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum ContextElement {
    Text(TextObject),
    Image(ImageElement),
}

/// Text or images grouped in a Slack context block.
#[derive(Default)]
pub struct ContextBlock {
    elements: Vec<ContextElement>,
    id: Option<String>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Slack block API")]
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
        self.imageWith(url, |image| {
            image.alt(alt);
        })
    }

    pub fn imageWith(
        &mut self,
        url: impl Into<String>,
        build: impl FnOnce(&mut ImageElement),
    ) -> &mut Self {
        let mut image = ImageElement::new(url);
        build(&mut image);
        self.elements.push(ContextElement::Image(image));
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
    accessory: Option<Action>,
    id: Option<String>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Slack block API")]
impl SectionBlock {
    pub fn text(&mut self, text: impl Into<String>) -> &mut TextObject {
        self.text.insert(TextObject::new(text))
    }

    pub fn field(&mut self, text: impl Into<String>) -> &mut TextObject {
        self.fields.push(TextObject::with_limit(text, 2000));
        self.fields.last_mut().expect("inserted a field")
    }

    pub fn accessory(&mut self, accessory: Value) -> &mut Self {
        self.accessory = Some(Action::Custom(accessory));
        self
    }

    pub fn staticSelect(&mut self, placeholder: impl Into<String>) -> &mut StaticSelectElement {
        let Action::StaticSelect(menu) =
            self.accessory
                .insert(Action::StaticSelect(Box::new(StaticSelectElement::new(
                    placeholder,
                ))))
        else {
            unreachable!("inserted a static menu")
        };
        menu
    }

    pub fn usersSelect(&mut self, placeholder: impl Into<String>) -> &mut UsersSelectElement {
        let Action::UsersSelect(menu) = self
            .accessory
            .insert(Action::UsersSelect(UsersSelectElement::new(placeholder)))
        else {
            unreachable!("inserted a users menu")
        };
        menu
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
            block["accessory"] = json!(accessory);
        }
        with_id(block, self.id)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum Action {
    Button(Box<ButtonElement>),
    StaticSelect(Box<StaticSelectElement>),
    UsersSelect(UsersSelectElement),
    Custom(Value),
}

/// Slack buttons or other Block Kit action elements.
#[derive(Default)]
pub struct ActionsBlock {
    elements: Vec<Action>,
    id: Option<String>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Slack action API")]
impl ActionsBlock {
    pub fn button(&mut self, text: impl Into<String>) -> &mut ButtonElement {
        let text = text.into();
        let id = format!(
            "button_{}",
            Str::slug(&text[..text.floor_char_boundary(248)])
        );
        self.elements
            .push(Action::Button(Box::new(ButtonElement::new(text, id))));
        let Some(Action::Button(element)) = self.elements.last_mut() else {
            unreachable!("inserted a button")
        };
        element
    }

    pub fn staticSelect(&mut self, placeholder: impl Into<String>) -> &mut StaticSelectElement {
        self.elements
            .push(Action::StaticSelect(Box::new(StaticSelectElement::new(
                placeholder,
            ))));
        let Some(Action::StaticSelect(menu)) = self.elements.last_mut() else {
            unreachable!("inserted a static menu")
        };
        menu
    }

    pub fn usersSelect(&mut self, placeholder: impl Into<String>) -> &mut UsersSelectElement {
        self.elements
            .push(Action::UsersSelect(UsersSelectElement::new(placeholder)));
        let Some(Action::UsersSelect(menu)) = self.elements.last_mut() else {
            unreachable!("inserted a users menu")
        };
        menu
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
    text: PlainText,
    action_id: String,
    #[serde(flatten)]
    options: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confirm: Option<ConfirmObject>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Slack button API")]
impl ButtonElement {
    fn new(text: String, action_id: String) -> Self {
        Self {
            kind: "button",
            text: PlainText::new(text, 75),
            action_id,
            options: Default::default(),
            confirm: None,
        }
    }

    pub fn emoji(&mut self) -> &mut Self {
        self.text.emoji();
        self
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
        let mut dialog = ConfirmObject {
            title: PlainText::new("Are you sure?", 100),
            text: TextObject::with_limit(text, 300),
            confirm: PlainText::new("Yes", 30),
            deny: PlainText::new("No", 30),
            style: None,
        };
        build(&mut dialog);
        self.confirm = Some(dialog);
        self
    }
}

/// Slack's confirmation dialog for an interactive element.
#[derive(Serialize)]
pub struct ConfirmObject {
    title: PlainText,
    text: TextObject,
    confirm: PlainText,
    deny: PlainText,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<&'static str>,
}

impl ConfirmObject {
    pub fn title(&mut self, title: impl Into<String>) -> &mut Self {
        self.title = PlainText::new(title, 100);
        self
    }

    pub fn confirm(&mut self, label: impl Into<String>) -> &mut Self {
        self.confirm = PlainText::new(label, 30);
        self
    }

    pub fn deny(&mut self, label: impl Into<String>) -> &mut Self {
        self.deny = PlainText::new(label, 30);
        self
    }

    pub fn danger(&mut self) -> &mut Self {
        self.style = Some("danger");
        self
    }

    pub fn text(&mut self, text: impl Into<String>) -> &mut TextObject {
        self.text = TextObject::with_limit(text, 300);
        &mut self.text
    }
}

fn with_id(mut block: Value, id: Option<String>) -> Value {
    if let Some(id) = id.filter(|id| !id.is_empty() && id != "0") {
        block["block_id"] = id.into();
    }
    block
}
