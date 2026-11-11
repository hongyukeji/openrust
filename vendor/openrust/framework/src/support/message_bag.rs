use std::collections::BTreeMap;

/// Ordered validation messages grouped by field.
#[derive(Clone, Debug, Default)]
pub struct MessageBag(Vec<(String, Vec<String>)>);

impl MessageBag {
    pub fn add(&mut self, field: impl Into<String>, message: impl Into<String>) {
        let field = field.into();
        let message = message.into();
        if let Some((_, messages)) = self.0.iter_mut().find(|(key, _)| *key == field) {
            if !messages.contains(&message) {
                messages.push(message);
            }
        } else {
            self.0.push((field, vec![message]));
        }
    }

    pub fn get(&self, field: &str) -> &[String] {
        self.0
            .iter()
            .find(|(key, _)| key == field)
            .map_or(&[], |(_, messages)| messages)
    }

    pub fn first(&self) -> Option<&str> {
        self.0.first()?.1.first().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.0.iter().map(|(_, messages)| messages.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn messages(&self) -> BTreeMap<String, Vec<String>> {
        self.0.iter().cloned().collect()
    }
}
