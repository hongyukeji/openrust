use html5ever::{
    Attribute, QualName,
    tendril::{StrTendril, TendrilSink},
    tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink},
};
use std::{
    borrow::Cow,
    cell::{Ref, RefCell},
};
#[derive(Clone)]
pub(super) struct Node {
    pub name: Option<QualName>,
    pub attrs: Vec<Attribute>,
    pub text: String,
    pub children: Vec<usize>,
    parent: Option<usize>,
    template: Option<usize>,
}
impl Node {
    fn empty() -> Self {
        Self {
            name: None,
            attrs: vec![],
            text: String::new(),
            children: vec![],
            parent: None,
            template: None,
        }
    }
    pub fn attr(&self, name: &str) -> &str {
        self.attrs
            .iter()
            .find(|a| a.name.local.as_ref() == name)
            .map_or("", |a| a.value.as_ref())
    }
    pub fn tag(&self) -> &str {
        self.name.as_ref().map_or("", |n| n.local.as_ref())
    }
}
struct Sink(RefCell<Vec<Node>>);
impl Sink {
    fn push(&self, n: Node) -> usize {
        let mut nodes = self.0.borrow_mut();
        let id = nodes.len();
        nodes.push(n);
        id
    }
    fn detach(&self, id: usize) {
        let mut nodes = self.0.borrow_mut();
        if let Some(p) = nodes[id].parent.take() {
            nodes[p].children.retain(|n| *n != id);
        }
    }
    fn insert(&self, parent: usize, before: Option<usize>, child: NodeOrText<usize>) {
        let id = match child {
            NodeOrText::AppendNode(id) => {
                self.detach(id);
                id
            }
            NodeOrText::AppendText(text) => {
                let mut nodes = self.0.borrow_mut();
                let position = before
                    .and_then(|s| nodes[parent].children.iter().position(|n| *n == s))
                    .unwrap_or(nodes[parent].children.len());
                if position > 0 {
                    let previous = nodes[parent].children[position - 1];
                    if nodes[previous].name.is_none() {
                        nodes[previous].text.push_str(&text);
                        return;
                    }
                }
                drop(nodes);
                let mut n = Node::empty();
                n.text = text.to_string();
                self.push(n)
            }
        };
        let mut nodes = self.0.borrow_mut();
        let at = before
            .and_then(|s| nodes[parent].children.iter().position(|n| *n == s))
            .unwrap_or(nodes[parent].children.len());
        nodes[id].parent = Some(parent);
        nodes[parent].children.insert(at, id);
    }
}
impl TreeSink for Sink {
    type Handle = usize;
    type Output = Vec<Node>;
    type ElemName<'a> = Ref<'a, QualName>;
    fn finish(self) -> Vec<Node> {
        self.0.into_inner()
    }
    fn parse_error(&self, _: Cow<'static, str>) {}
    fn get_document(&self) -> usize {
        0
    }
    fn elem_name<'a>(&'a self, id: &usize) -> Self::ElemName<'a> {
        Ref::map(self.0.borrow(), |n| {
            n[*id].name.as_ref().expect("元素具有名称")
        })
    }
    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> usize {
        let mut n = Node::empty();
        if flags.template {
            n.template = Some(self.push(Node::empty()));
        }
        n.name = Some(name);
        n.attrs = attrs;
        self.push(n)
    }
    fn create_comment(&self, _: StrTendril) -> usize {
        self.push(Node::empty())
    }
    fn create_pi(&self, _: StrTendril, _: StrTendril) -> usize {
        self.push(Node::empty())
    }
    fn append(&self, parent: &usize, child: NodeOrText<usize>) {
        self.insert(*parent, None, child)
    }
    fn append_based_on_parent_node(
        &self,
        element: &usize,
        previous: &usize,
        child: NodeOrText<usize>,
    ) {
        let parent = self.0.borrow()[*element].parent;
        if let Some(parent) = parent {
            self.insert(parent, Some(*element), child)
        } else {
            self.insert(*previous, None, child)
        }
    }
    fn append_doctype_to_document(&self, _: StrTendril, _: StrTendril, _: StrTendril) {}
    fn get_template_contents(&self, id: &usize) -> usize {
        self.0.borrow()[*id].template.expect("模板具有独立内容")
    }
    fn same_node(&self, a: &usize, b: &usize) -> bool {
        a == b
    }
    fn set_quirks_mode(&self, _: QuirksMode) {}
    fn append_before_sibling(&self, id: &usize, child: NodeOrText<usize>) {
        let parent = self.0.borrow()[*id].parent;
        if let Some(parent) = parent {
            self.insert(parent, Some(*id), child)
        }
    }
    fn add_attrs_if_missing(&self, id: &usize, attrs: Vec<Attribute>) {
        let mut nodes = self.0.borrow_mut();
        for attr in attrs {
            if !nodes[*id].attrs.iter().any(|a| a.name == attr.name) {
                nodes[*id].attrs.push(attr);
            }
        }
    }
    fn remove_from_parent(&self, id: &usize) {
        self.detach(*id)
    }
    fn reparent_children(&self, id: &usize, new: &usize) {
        let children = std::mem::take(&mut self.0.borrow_mut()[*id].children);
        for child in children {
            self.insert(*new, None, NodeOrText::AppendNode(child));
        }
    }
}
pub(super) fn parse(html: &str) -> Vec<Node> {
    html5ever::parse_document(Sink(RefCell::new(vec![Node::empty()])), Default::default()).one(html)
}
