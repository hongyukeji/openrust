use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    io,
    rc::Rc,
    sync::Arc,
};

use super::{
    Factory, FragmentSelection, ViewData, ViewError, ViewResult, ViewValue, compilers::Directives,
};

/// State owned by one render tree. A layout inherits sections from its child.
pub(super) struct RenderContext<'a> {
    factory: Option<&'a Factory>,
    depth: usize,
    sections: HashMap<String, String>,
    layout: Option<String>,
    slots: HashMap<String, String>,
    parent_marker: Arc<str>,
    pub(super) active_loop: Option<Arc<ViewValue>>,
    pub(super) component_attributes: Option<ViewData>,
    pub(super) csrf_token: Option<Arc<str>>,
    pub(super) directives: Arc<Directives>,
    shared: Rc<RefCell<RenderState>>,
}

#[derive(Default)]
struct RenderState {
    stacks: HashMap<String, Stack>,
    once: HashSet<(bool, String)>,
    fragments: Vec<(String, String)>,
}

#[derive(Default)]
struct Stack {
    pushed: Vec<(usize, String)>,
    prepended: Vec<(usize, String)>,
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
            active_loop: None,
            component_attributes: None,
            csrf_token: None,
            directives: Arc::default(),
            shared: Rc::default(),
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

    pub(super) fn exists(&self, name: &str) -> ViewResult<bool> {
        self.factory
            .map(|factory| factory.exists(name))
            .ok_or_else(|| render_error("Nested views require a view factory."))
    }

    pub(super) fn slot(&self, name: &str) -> Option<&str> {
        self.slots.get(name).map(String::as_str)
    }

    pub(super) fn has_stack(&self, name: &str) -> bool {
        self.shared.borrow().stacks.contains_key(name)
    }

    pub(super) fn first_render_of(&self, id: &str, generated: bool) -> bool {
        self.shared
            .borrow_mut()
            .once
            .insert((generated, id.to_owned()))
    }

    pub(super) fn add_to_stack(&self, name: String, html: String, prepend: bool) {
        let mut shared = self.shared.borrow_mut();
        let stack = shared.stacks.entry(name).or_default();
        let group = if prepend {
            &mut stack.prepended
        } else {
            &mut stack.pushed
        };
        if let Some((_, existing)) = group.iter_mut().find(|(depth, _)| *depth == self.depth) {
            if prepend {
                existing.insert_str(0, &html);
            } else {
                existing.push_str(&html);
            }
        } else {
            group.push((self.depth, html));
        }
    }

    pub(super) fn stack(&self, name: &str, output: &mut String) -> bool {
        let shared = self.shared.borrow();
        let Some(stack) = shared.stacks.get(name) else {
            return false;
        };
        for (_, html) in stack.prepended.iter().rev().chain(&stack.pushed) {
            output.push_str(html);
        }
        true
    }

    pub(super) fn capture_fragment(&self, name: String, html: String) {
        let mut shared = self.shared.borrow_mut();
        if let Some((_, content)) = shared.fragments.iter_mut().find(|(key, _)| *key == name) {
            *content = html;
        } else {
            shared.fragments.push((name, html));
        }
    }

    pub(super) fn selected_fragments(&self, selection: FragmentSelection) -> String {
        let shared = self.shared.borrow();
        match selection {
            FragmentSelection::Named(names) => names
                .iter()
                .filter_map(|name| shared.fragments.iter().find(|(key, _)| key == name))
                .map(|(_, html)| html.as_str())
                .collect(),
            FragmentSelection::All => shared
                .fragments
                .iter()
                .map(|(_, html)| html.as_str())
                .collect(),
        }
    }

    pub(super) fn component(
        &self,
        name: &str,
        properties: &ViewData,
        slots: HashMap<String, String>,
    ) -> ViewResult<String> {
        let (factory, mut nested) = self.nested()?;
        nested.slots = slots;
        nested.component_attributes = Some(properties.clone());
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
                active_loop: self.active_loop.clone(),
                component_attributes: self.component_attributes.clone(),
                csrf_token: self.csrf_token.clone(),
                directives: self.directives.clone(),
                shared: self.shared.clone(),
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
