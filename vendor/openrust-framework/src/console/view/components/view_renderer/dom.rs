use html5ever::{
    Attribute, QualName, TokenizerResult,
    buffer_queue::BufferQueue,
    tokenizer::{Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, states::RawKind},
};
use std::{cell::RefCell, sync::LazyLock};

use crate::support::decode_html_entities;

#[derive(Clone, Default)]
pub(super) struct Node {
    pub name: Option<QualName>,
    pub attrs: Vec<Attribute>,
    pub text: String,
    pub comment: bool,
    pub explicit_empty: Vec<String>,
    pub children: Vec<usize>,
}
impl Node {
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

enum CommentKind {
    Comment,
    Instruction,
    InvalidEndTag,
}

struct Tree<'a> {
    nodes: Vec<Node>,
    open: Vec<usize>,
    markers: &'a Markers,
    next_comment: CommentKind,
    eof_tag_suffix: Option<&'static str>,
}

impl Tree<'_> {
    fn append(&mut self, node: Node) -> usize {
        let id = self.nodes.len();
        self.nodes.push(node);
        let parent = *self.open.last().expect("控制台片段始终拥有正文节点");
        self.nodes[parent].children.push(id);
        id
    }

    fn text(&mut self, text: &str) {
        let parent = *self.open.last().expect("控制台片段始终拥有正文节点");
        let previous = self.nodes[parent].children.last().copied();
        if let Some(previous) = previous.map(|id| &mut self.nodes[id])
            && previous.name.is_none()
            && !previous.comment
        {
            previous.text.push_str(text);
        } else {
            self.append(Node {
                text: text.to_owned(),
                ..Node::default()
            });
        }
    }

    fn start(&mut self, tag: Tag) -> TokenSinkResult<usize> {
        let name = tag.name.as_ref();
        if matches!(name, "html" | "body" | "head") {
            return TokenSinkResult::Continue;
        }
        while self.open.len() > 1
            && implicitly_closes(self.nodes[*self.open.last().unwrap()].tag(), name)
        {
            self.open.pop();
        }
        let raw = match name {
            "script" => Some(RawKind::ScriptData),
            "style" => Some(RawKind::Rawtext),
            _ => None,
        };
        let mut attrs = tag.attrs;
        let mut explicit_empty = Vec::new();
        for attr in &mut attrs {
            if attr.value.as_ref() == self.markers.empty {
                explicit_empty.push(attr.name.local.to_string());
            }
            attr.value = self.markers.restore(&attr.value, true).into();
        }
        let void = is_void(name);
        let id = self.append(Node {
            name: Some(QualName::new(None, html5ever::ns!(html), tag.name)),
            attrs,
            explicit_empty,
            ..Node::default()
        });
        if tag.self_closing || void {
            TokenSinkResult::Continue
        } else {
            self.open.push(id);
            raw.map_or(TokenSinkResult::Continue, TokenSinkResult::RawData)
        }
    }

    fn close(&mut self, name: &str) {
        let target_priority = end_priority(name);
        for (position, id) in self.open.iter().enumerate().skip(1).rev() {
            let actual = self.nodes[*id].tag();
            if actual == name {
                self.open.truncate(position);
                return;
            }
            if end_priority(actual) > target_priority {
                return;
            }
        }
    }
}

struct Fragment<'a>(RefCell<Tree<'a>>);
impl TokenSink for Fragment<'_> {
    type Handle = usize;

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<usize> {
        let mut tree = self.0.borrow_mut();
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => return tree.start(tag),
            Token::TagToken(tag) => tree.close(tag.name.as_ref()),
            Token::CharacterTokens(text) => tree.text(&text),
            Token::CommentToken(text) => {
                match std::mem::replace(&mut tree.next_comment, CommentKind::Comment) {
                    CommentKind::Comment => {
                        tree.append(Node {
                            text: text.to_string(),
                            comment: true,
                            ..Node::default()
                        });
                    }
                    CommentKind::Instruction => {
                        tree.append(Node {
                            name: Some(QualName::new(None, html5ever::ns!(html), "#pi".into())),
                            ..Node::default()
                        });
                    }
                    CommentKind::InvalidEndTag => {}
                }
            }
            Token::ParseError(error) => tree.handle_token_error(&error),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

impl Tree<'_> {
    fn handle_token_error(&mut self, error: &str) {
        // html5ever 0.38 默认错误保留状态。区分真实注释、处理指令与无效结束标签。
        if error == "Saw ? in state TagOpen" {
            self.next_comment = CommentKind::Instruction;
        } else if error.ends_with(" in state EndTagOpen") && error != "Saw EOF in state EndTagOpen"
        {
            self.next_comment = CommentKind::InvalidEndTag;
        }
        let Some(state) = error.strip_prefix("Saw EOF in state ") else {
            return;
        };
        self.eof_tag_suffix = match state {
            "AttributeValue(DoubleQuoted)" => Some("\">"),
            "AttributeValue(SingleQuoted)" => Some("'>"),
            "TagName"
            | "BeforeAttributeName"
            | "AttributeName"
            | "AfterAttributeName"
            | "AttributeValue(Unquoted)"
            | "AfterAttributeValueQuoted"
            | "SelfClosingStartTag" => Some(">"),
            _ => None,
        };
    }
}

struct Markers {
    carriage: String,
    empty: String,
    ampersand: String,
}
impl Markers {
    fn new(html: &str) -> Self {
        let mut marker = "\u{e000}".to_owned();
        while html.contains(&marker) {
            marker.push('\u{e001}');
        }
        Self {
            carriage: format!("{marker}R"),
            empty: format!("{marker}E"),
            ampersand: format!("{marker}A"),
        }
    }

    fn restore(&self, value: &str, decode: bool) -> String {
        let text = value
            .replace(&self.carriage, "\r")
            .replace(&self.empty, "")
            .replace(&self.ampersand, "&");
        if decode {
            decode_references(&text)
        } else {
            text
        }
    }
}

fn decode_references(text: &str) -> String {
    static REFERENCE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"&(?:#[xX][0-9A-Fa-f]*;?|#[0-9]*;?|[A-Za-z][A-Za-z0-9]*;)")
            .expect("合法 HTML4 引用表达式")
    });
    REFERENCE.replace_all(text, |capture: &regex::Captures<'_>| {
        let reference = &capture[0];
        if let Some(number) = reference.strip_prefix("&#") {
            let number = number.trim_end_matches(';');
            let code = if let Some(hex) = number.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                number.parse().ok()
            };
            return code
                .filter(|code| matches!(code, 9 | 10 | 13 | 32..=0xd7ff | 0xe000..=0xfffd | 0x10000..=0x10ffff))
                .and_then(char::from_u32)
                .map_or_else(String::new, |character| character.to_string());
        }
        if reference == "&apos;" {
            return "'".to_owned();
        }
        decode_html_entities(reference).into_owned()
    }).into_owned()
}

pub(super) fn parse(html: &str) -> Vec<Node> {
    static EMPTY: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r#"(?i)(\b[a-z_:][a-z0-9_:.-]*\s*=\s*)(""|'')"#)
            .expect("合法空属性表达式")
    });
    let markers = Markers::new(html);
    let html = html
        .replace('\r', &markers.carriage)
        .replace('&', &markers.ampersand);
    let html = EMPTY.replace_all(&html, |capture: &regex::Captures<'_>| {
        format!(
            "{}{}{}{}",
            &capture[1],
            &capture[2][..1],
            markers.empty,
            &capture[2][1..]
        )
    });
    let mut tree = tokenize(&html, &markers);
    if let Some(suffix) = tree.eof_tag_suffix {
        // 固定片段解析会接收结尾处已经读到的标签和属性。只在非完整标签的 EOF 重读一次。
        let completed = html.into_owned() + suffix;
        tree = tokenize(&completed, &markers);
    }
    let mut nodes = tree.nodes;
    for parent in 0..nodes.len() {
        let decode_text = !matches!(nodes[parent].tag(), "script" | "style");
        for position in 0..nodes[parent].children.len() {
            let child = nodes[parent].children[position];
            if nodes[child].name.is_none() {
                nodes[child].text =
                    markers.restore(&nodes[child].text, decode_text && !nodes[child].comment);
            }
        }
    }
    nodes
}

fn tokenize<'a>(html: &str, markers: &'a Markers) -> Tree<'a> {
    let tree = Fragment(RefCell::new(Tree {
        nodes: vec![Node {
            name: Some(QualName::new(None, html5ever::ns!(html), "body".into())),
            ..Node::default()
        }],
        open: vec![0],
        markers,
        next_comment: CommentKind::Comment,
        eof_tag_suffix: None,
    }));
    let tokenizer = Tokenizer::new(tree, Default::default());
    let input = BufferQueue::default();
    input.push_back(html.into());
    while !matches!(tokenizer.feed(&input), TokenizerResult::Done) {}
    tokenizer.end();
    tokenizer.sink.0.into_inner()
}

fn end_priority(name: &str) -> u8 {
    match name {
        "div" => 150,
        "td" | "th" => 160,
        "tr" => 170,
        "thead" | "tbody" | "tfoot" => 180,
        "table" => 190,
        "head" | "body" => 200,
        "html" => 220,
        _ => 100,
    }
}

// HTML4 恢复规则与第三方许可见 ../LICENSE.html4。
fn is_void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "basefont"
            | "br"
            | "col"
            | "frame"
            | "hr"
            | "img"
            | "input"
            | "isindex"
            | "link"
            | "meta"
            | "param"
    )
}

fn implicitly_closes(current: &str, next: &str) -> bool {
    match current {
        "a" => matches!(next, "a" | "fieldset" | "table" | "td" | "th"),
        "address" => matches!(next, "dd" | "dl" | "dt" | "form" | "li" | "ul"),
        "b" => matches!(next, "center" | "p" | "td" | "th"),
        "big" => matches!(next, "p"),
        "caption" => matches!(
            next,
            "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr"
        ),
        "col" => matches!(
            next,
            "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr"
        ),
        "colgroup" => matches!(next, "colgroup" | "tbody" | "tfoot" | "thead" | "tr"),
        "dd" => matches!(next, "dt"),
        "dir" => matches!(next, "dd" | "dl" | "dt" | "form" | "ul"),
        "dl" => matches!(next, "form" | "li"),
        "dt" => matches!(next, "dd" | "dl"),
        "font" => matches!(next, "center" | "td" | "th"),
        "form" => matches!(next, "form"),
        "h1" => matches!(next, "fieldset" | "form" | "li" | "p" | "table"),
        "h2" => matches!(next, "fieldset" | "form" | "li" | "p" | "table"),
        "h3" => matches!(next, "fieldset" | "form" | "li" | "p" | "table"),
        "h4" => matches!(next, "fieldset" | "form" | "li" | "p" | "table"),
        "h5" => matches!(next, "fieldset" | "form" | "li" | "p" | "table"),
        "h6" => matches!(next, "fieldset" | "form" | "li" | "p" | "table"),
        "head" => matches!(
            next,
            "a" | "abbr"
                | "acronym"
                | "address"
                | "b"
                | "bdo"
                | "big"
                | "blockquote"
                | "body"
                | "br"
                | "center"
                | "cite"
                | "code"
                | "dd"
                | "dfn"
                | "dir"
                | "div"
                | "dl"
                | "dt"
                | "em"
                | "fieldset"
                | "font"
                | "form"
                | "frameset"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "hr"
                | "i"
                | "iframe"
                | "img"
                | "kbd"
                | "li"
                | "listing"
                | "map"
                | "menu"
                | "ol"
                | "p"
                | "pre"
                | "q"
                | "s"
                | "samp"
                | "small"
                | "span"
                | "strike"
                | "strong"
                | "sub"
                | "sup"
                | "table"
                | "tt"
                | "u"
                | "ul"
                | "var"
                | "xmp"
        ),
        "hr" => matches!(next, "form"),
        "i" => matches!(next, "center" | "p" | "td" | "th"),
        "legend" => matches!(next, "fieldset"),
        "li" => matches!(next, "li"),
        "link" => matches!(next, "body" | "frameset"),
        "listing" => matches!(
            next,
            "dd" | "dl" | "dt" | "fieldset" | "form" | "li" | "table" | "ul"
        ),
        "menu" => matches!(next, "dd" | "dl" | "dt" | "form" | "ul"),
        "ol" => matches!(next, "form"),
        "option" => matches!(next, "optgroup" | "option"),
        "p" => matches!(
            next,
            "address"
                | "blockquote"
                | "body"
                | "caption"
                | "center"
                | "col"
                | "colgroup"
                | "dd"
                | "dir"
                | "div"
                | "dl"
                | "dt"
                | "fieldset"
                | "form"
                | "frameset"
                | "h1"
                | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "head"
                | "hr"
                | "li"
                | "listing"
                | "menu"
                | "ol"
                | "p"
                | "pre"
                | "table"
                | "tbody"
                | "td"
                | "tfoot"
                | "th"
                | "title"
                | "tr"
                | "ul"
                | "xmp"
        ),
        "pre" => matches!(
            next,
            "dd" | "dl" | "dt" | "fieldset" | "form" | "li" | "table" | "ul"
        ),
        "s" => matches!(next, "p"),
        "script" => matches!(next, "noscript"),
        "small" => matches!(next, "p"),
        "span" => matches!(next, "td" | "th"),
        "strike" => matches!(next, "p"),
        "style" => matches!(next, "body" | "frameset"),
        "tbody" => matches!(next, "tbody" | "tfoot"),
        "td" => matches!(next, "tbody" | "td" | "tfoot" | "th" | "tr"),
        "tfoot" => matches!(next, "tbody"),
        "th" => matches!(next, "tbody" | "td" | "tfoot" | "th" | "tr"),
        "thead" => matches!(next, "tbody" | "tfoot"),
        "title" => matches!(next, "body" | "frameset"),
        "tr" => matches!(next, "tbody" | "tfoot" | "tr"),
        "tt" => matches!(next, "p"),
        "u" => matches!(next, "p" | "td" | "th"),
        "ul" => matches!(next, "address" | "form" | "menu" | "pre"),
        "xmp" => matches!(
            next,
            "dd" | "dl" | "dt" | "fieldset" | "form" | "li" | "table" | "ul"
        ),
        _ => false,
    }
}
