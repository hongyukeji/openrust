use std::{collections::HashMap, io, sync::Arc};

use super::{Factory, ViewData, ViewError, ViewResult};

/// State owned by one render tree. A layout inherits sections from its child.
pub(super) struct RenderContext<'a> {
    factory: Option<&'a Factory>,
    depth: usize,
    sections: HashMap<String, String>,
    layout: Option<String>,
    slots: HashMap<String, String>,
    parent_marker: Arc<str>,
}

impl<'a> RenderContext<'a> {
    pub(super) fn new(factory: Option<&'a Factory>) -> Self {
        Self {
            factory,
            depth: 0,
            sections: HashMap::new(),
            layout: None,
            slots: HashMap::new(),
            parent_marker: format!("\0blade-parent-{:032x}\0", fastrand::u128(..)).into(),
        }
    }

    pub(super) fn section(&mut self, name: String, html: String) {
        self.sections
            .entry(name)
            .and_modify(|child| {
                if child.contains(self.parent_marker.as_ref()) {
                    *child = child.replace(self.parent_marker.as_ref(), &html);
                }
            })
            .or_insert(html);
    }

    pub(super) fn yield_to(&self, name: &str, output: &mut String) -> bool {
        let Some(section) = self.sections.get(name) else {
            return false;
        };
        for text in section.split(self.parent_marker.as_ref()) {
            output.push_str(text);
        }
        true
    }

    pub(super) fn parent(&self) -> &str {
        &self.parent_marker
    }

    pub(super) fn extend(&mut self, name: String) -> ViewResult<()> {
        if self.layout.is_some() {
            return Err(render_error("A Blade view may extend one layout."));
        }
        self.layout = Some(name);
        Ok(())
    }

    pub(super) fn render_layout(&mut self, data: &ViewData) -> ViewResult<String> {
        let Some(name) = self.layout.take() else {
            return Ok(String::new());
        };
        let (factory, mut nested) = self.nested()?;
        nested.sections = std::mem::take(&mut self.sections);
        factory.render_nested(&name, data, &mut nested)
    }

    pub(super) fn include(&self, name: &str, data: &ViewData) -> ViewResult<String> {
        let (factory, mut nested) = self.nested()?;
        factory.render_nested(name, data, &mut nested)
    }

    pub(super) fn slot(&self, name: &str) -> Option<&str> {
        self.slots.get(name).map(String::as_str)
    }

    pub(super) fn component(
        &self,
        name: &str,
        properties: &ViewData,
        slots: HashMap<String, String>,
    ) -> ViewResult<String> {
        let (factory, mut nested) = self.nested()?;
        nested.slots = slots;
        factory.render_component(name, properties, &mut nested)
    }

    fn nested(&self) -> ViewResult<(&'a Factory, Self)> {
        if self.depth >= 64 {
            return Err(render_error("Blade views are nested too deeply."));
        }
        let factory = self
            .factory
            .ok_or_else(|| render_error("Nested views require a view factory."))?;
        Ok((
            factory,
            Self {
                depth: self.depth + 1,
                slots: self.slots.clone(),
                parent_marker: self.parent_marker.clone(),
                factory: Some(factory),
                sections: HashMap::new(),
                layout: None,
            },
        ))
    }
}

pub(super) fn render_error(message: &'static str) -> ViewError {
    ViewError::RenderFailed(io::Error::new(io::ErrorKind::InvalidData, message).into())
}
