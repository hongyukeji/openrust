// 第三方许可见 ../LICENSE.renderer。
use super::{
    StyleRepository, TerminalStyles,
    style::{self, Style},
    tree::Prepared,
};
use crate::console::{AsOutput, ConsoleError, OutputOptions, Terminal};
use parking_lot::RwLock;
use std::sync::Arc;

/// 可直接构造的终端元素类型。构造时不解析 HTML。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TerminalElementKind {
    Raw,
    Span,
    Div,
    Paragraph,
    Anchor,
    BreakLine,
    Rule,
    ListItem,
    OrderedList,
    UnorderedList,
    DefinitionList,
    DefinitionTerm,
    DefinitionDescription,
}

impl TerminalElementKind {
    const fn tag(self) -> &'static str {
        match self {
            Self::Raw => "span",
            Self::Span => "span",
            Self::Div => "div",
            Self::Paragraph => "p",
            Self::Anchor => "a",
            Self::BreakLine => "br",
            Self::Rule => "hr",
            Self::ListItem => "li",
            Self::OrderedList => "ol",
            Self::UnorderedList => "ul",
            Self::DefinitionList => "dl",
            Self::DefinitionTerm => "dt",
            Self::DefinitionDescription => "dd",
        }
    }
}

/// 终端元素可包含文本或一组有序子项。
pub enum TerminalContent {
    Text(String),
    Children(Vec<TerminalChild>),
}

impl From<&str> for TerminalContent {
    fn from(text: &str) -> Self {
        Self::Text(text.into())
    }
}

impl From<String> for TerminalContent {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<Vec<TerminalChild>> for TerminalContent {
    fn from(children: Vec<TerminalChild>) -> Self {
        Self::Children(children)
    }
}

pub enum TerminalChild {
    Text(String),
    Element(TerminalElement),
}

impl From<&str> for TerminalChild {
    fn from(text: &str) -> Self {
        Self::Text(text.into())
    }
}

impl From<String> for TerminalChild {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<TerminalElement> for TerminalChild {
    fn from(element: TerminalElement) -> Self {
        Self::Element(element)
    }
}

/// 独立终端元素。样式组合、继承和格式化复用原生布局。
pub struct TerminalElement {
    kind: TerminalElementKind,
    content: TerminalContent,
    formatted_children: Option<String>,
    styles: Arc<RwLock<Style>>,
    repository: Option<StyleRepository>,
}

impl TerminalElement {
    pub fn new(kind: TerminalElementKind, content: impl Into<TerminalContent>) -> Self {
        Self {
            kind,
            content: content.into(),
            formatted_children: None,
            styles: Arc::new(RwLock::new(Style {
                element: kind.tag(),
                display: "inline",
                ..Style::default()
            })),
            repository: None,
        }
    }

    pub fn from_styles(
        kind: TerminalElementKind,
        content: impl Into<TerminalContent>,
        classes: &str,
    ) -> Result<Self, ConsoleError> {
        Self::from_styles_with(kind, content, classes, None)
    }

    pub fn from_styles_with(
        kind: TerminalElementKind,
        content: impl Into<TerminalContent>,
        classes: &str,
        styles: Option<StyleRepository>,
    ) -> Result<Self, ConsoleError> {
        let mut element = Self::new(kind, content);
        element.repository = styles;
        element.configure(|style| style.apply(classes).map(|_| ()))?;
        Ok(element)
    }

    pub fn default_styles(&self) -> impl Iterator<Item = &'static str> {
        style::default_classes(self.kind.tag()).split_ascii_whitespace()
    }

    /// 新元素保留样式身份，其子项缓存按新内容重新建立。
    pub fn set_content(&self, content: impl Into<TerminalContent>) -> Self {
        Self {
            kind: self.kind,
            content: content.into(),
            formatted_children: None,
            styles: Arc::clone(&self.styles),
            repository: self.repository.clone(),
        }
    }

    pub fn configure<T>(
        &mut self,
        callback: impl FnOnce(&mut TerminalStyles<'_>) -> Result<T, ConsoleError>,
    ) -> Result<T, ConsoleError> {
        let mut style = self.styles.try_write().ok_or_else(style_busy)?;
        callback(&mut TerminalStyles::borrow(
            &mut style,
            self.repository.as_ref(),
            self.kind.tag(),
            0,
        ))
    }

    /// 返回用于输出格式器的带标记文本。
    pub fn to_markup(&mut self) -> Result<String, ConsoleError> {
        let viewport = Terminal::new().get_width();
        if !(0..=1_000_000).contains(&viewport) {
            return Err(ConsoleError::new("组件终端宽度超出安全范围"));
        }
        let mut prepared = self.prepare(viewport, 0)?;
        let output = prepared.render(viewport)?;
        if matches!(self.content, TerminalContent::Children(_))
            && self.kind != TerminalElementKind::Raw
        {
            self.formatted_children = prepared.content;
        }
        Ok(output)
    }

    pub fn render(
        &mut self,
        output: &mut (impl AsOutput + ?Sized),
        options: impl Into<OutputOptions>,
    ) -> Result<(), ConsoleError> {
        output
            .as_output()
            .writeln(self.to_markup()?.as_str(), options)
    }

    fn prepare(&mut self, viewport: i64, depth: usize) -> Result<Prepared, ConsoleError> {
        if depth > 128 {
            return Err(ConsoleError::new("终端元素嵌套超过可渲染深度"));
        }
        if self.kind == TerminalElementKind::Raw {
            let mut value = String::new();
            match &mut self.content {
                TerminalContent::Text(text) => value.push_str(text),
                TerminalContent::Children(children) => {
                    for child in children {
                        value.push_str(&match child {
                            TerminalChild::Text(text) => text.clone(),
                            TerminalChild::Element(element) => {
                                element.prepare(viewport, depth + 1)?.render(viewport)?
                            }
                        });
                    }
                }
            }
            return Ok(raw(value));
        }
        let style = self.styles.try_read().ok_or_else(style_busy)?.clone();
        let mut children = vec![];
        let content = if let Some(prepared) = &self.formatted_children {
            Some(prepared.clone())
        } else {
            match &mut self.content {
                TerminalContent::Text(text) => Some(text.clone()),
                TerminalContent::Children(items) => {
                    children.reserve(items.len());
                    for child in items {
                        children.push(match child {
                            TerminalChild::Text(text) => raw(text.clone()),
                            TerminalChild::Element(child) => child.prepare(viewport, depth + 1)?,
                        });
                    }
                    None
                }
            }
        };
        Ok(Prepared {
            style: Some(style),
            raw_element: false,
            children,
            content,
            parent: None,
            spacing: [0; 2],
            first: false,
            br: self.kind == TerminalElementKind::BreakLine,
        })
    }
}

fn raw(content: String) -> Prepared {
    Prepared {
        style: None,
        raw_element: true,
        children: Vec::new(),
        content: Some(content),
        parent: None,
        spacing: [0; 2],
        first: false,
        br: false,
    }
}

fn style_busy() -> ConsoleError {
    ConsoleError::new("终端元素样式正被另一个调用修改")
}
