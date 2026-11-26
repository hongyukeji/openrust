use html5ever::{
    Attribute, QualName,
    tendril::StrTendril,
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
    pub comment: bool,
    pub explicit_empty: Vec<String>,
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
            comment: false,
            explicit_empty: vec![],
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
                    if nodes[previous].name.is_none() && !nodes[previous].comment {
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
    fn create_comment(&self, text: StrTendril) -> usize {
        let mut node = Node::empty();
        node.comment = true;
        node.text = text.to_string();
        self.push(node)
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
struct PreservingTree(html5ever::tree_builder::TreeBuilder<usize, Sink>);
impl html5ever::tokenizer::TokenSink for PreservingTree {
    type Handle = usize;
    fn process_token(
        &self,
        token: html5ever::tokenizer::Token,
        line: u64,
    ) -> html5ever::tokenizer::TokenSinkResult<usize> {
        use html5ever::tokenizer::{TagKind, Token};
        let pre = matches!(&token,Token::TagToken(tag) if tag.name.as_ref()=="pre" && tag.kind==TagKind::StartTag);
        let result = self.0.process_token(token, line);
        // 清除 HTML5 对 pre 首换行的抑制；固定预格式管线自行移除一个空边界行。
        if pre {
            let cleared = self
                .0
                .process_token(Token::CharacterTokens("\n".into()), line);
            debug_assert!(matches!(
                cleared,
                html5ever::tokenizer::TokenSinkResult::Continue
            ));
        }
        result
    }
    fn end(&self) {
        self.0.end();
    }
    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.0
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}
pub(super) fn parse(html: &str) -> Vec<Node> {
    use html5ever::{
        TokenizerResult, buffer_queue::BufferQueue, tokenizer::Tokenizer, tree_builder::TreeBuilder,
    };
    use std::sync::LazyLock;
    // 临时标记不与输入冲突，解析后即恢复，避免 HTML5 预处理吞并 CR 或丢失空属性来源。
    let mut marker = "\u{e000}".to_owned();
    while html.contains(&marker) {
        marker.push('\u{e001}');
    }
    let carriage = format!("{marker}R");
    let empty = format!("{marker}E");
    static EMPTY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)(\b[a-z_:][a-z0-9_:.-]*\s*=\s*)(""|'')"#)
            .expect("合法空属性表达式")
    });
    let html = html.replace('\r', &carriage);
    let html = EMPTY.replace_all(&html, |capture: &regex::Captures<'_>| {
        format!(
            "{}{}{}{}",
            &capture[1],
            &capture[2][..1],
            empty,
            &capture[2][1..]
        )
    });
    let tree = PreservingTree(TreeBuilder::new(
        Sink(RefCell::new(vec![Node::empty()])),
        Default::default(),
    ));
    let tokenizer = Tokenizer::new(tree, Default::default());
    let input = BufferQueue::default();
    input.push_back(html.as_ref().into());
    while !matches!(tokenizer.feed(&input), TokenizerResult::Done) {}
    tokenizer.end();
    let mut nodes = tokenizer.sink.0.sink.finish();
    let restore = |s: &str| s.replace(&carriage, "\r").replace(&empty, "");
    for node in &mut nodes {
        node.text = restore(&node.text);
        for attr in &mut node.attrs {
            if attr.value.as_ref() == empty {
                node.explicit_empty.push(attr.name.local.to_string());
            }
            attr.value = restore(&attr.value).into();
        }
    }
    nodes
}

/// 列表范围内按固定 HTML4 自动结束规则提升后续节点；其余恢复差异单列。
pub(super) fn recover_lists(nodes: &mut [Node]) {
    for id in 0..nodes.len() {
        let closes: &[&str] = match nodes[id].tag() {
            "dl" => &["li", "form"],
            "dt" => &["dl"],
            "ol" => &["form"],
            "ul" => &["address", "form", "menu", "pre"],
            "pre" => &["dd", "dl", "dt", "fieldset", "form", "li", "table", "ul"],
            _ => continue,
        };
        let Some(position) = nodes[id]
            .children
            .iter()
            .position(|child| closes.contains(&nodes[*child].tag()))
        else {
            continue;
        };
        let Some(parent) = nodes[id].parent else {
            continue;
        };
        let Some(after) = nodes[parent]
            .children
            .iter()
            .position(|child| *child == id)
            .map(|n| n + 1)
        else {
            continue;
        };
        let moved = nodes[id].children.split_off(position);
        for child in &moved {
            nodes[*child].parent = Some(parent);
        }
        nodes[parent].children.splice(after..after, moved);
    }
}
