// 第三方许可见 ../LICENSE.renderer。
use super::{
    StyleRepository, TerminalStyles, properties,
    style::{self, Style},
    tree::Prepared,
};
use crate::console::{AsOutput, ConsoleError, OutputOptions, Terminal};
use parking_lot::RwLock;
use serde_json::{Map, Value};
use std::sync::{Arc, Weak};

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

struct ElementContent {
    value: TerminalContent,
    formatted_children: Option<String>,
}

struct ElementStyles {
    style: RwLock<Style>,
    latest: RwLock<Weak<RwLock<ElementContent>>>,
}

/// 独立终端元素。样式组合、继承和格式化复用原生布局。
pub struct TerminalElement {
    kind: TerminalElementKind,
    content: Arc<RwLock<ElementContent>>,
    styles: Arc<ElementStyles>,
    repository: Option<StyleRepository>,
    defaults: Option<Arc<[String]>>,
}

impl TerminalElement {
    pub fn new(kind: TerminalElementKind, content: impl Into<TerminalContent>) -> Self {
        Self::with_styles(kind, content, TerminalStyles::new())
    }

    /// 将独立样式关联到所构造的元素，保留其属性、默认类和修饰器。
    pub fn with_styles(
        kind: TerminalElementKind,
        content: impl Into<TerminalContent>,
        styles: TerminalStyles<'static>,
    ) -> Self {
        let (mut style, repository, defaults) = styles.into_parts();
        style.element = kind.tag();
        let content = Arc::new(RwLock::new(ElementContent {
            value: content.into(),
            formatted_children: None,
        }));
        let styles = Arc::new(ElementStyles {
            style: RwLock::new(style),
            latest: RwLock::new(Arc::downgrade(&content)),
        });
        Self {
            kind,
            content,
            styles,
            repository,
            defaults: defaults.map(Arc::from),
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

    pub fn default_styles(&self) -> impl Iterator<Item = &str> {
        let implicit = if self.defaults.is_none() {
            style::default_classes(self.kind.tag())
        } else {
            ""
        };
        implicit.split_ascii_whitespace().chain(
            self.defaults
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(String::as_str),
        )
    }

    /// 新元素保留样式身份，其子项缓存按新内容重新建立。
    pub fn set_content(&self, content: impl Into<TerminalContent>) -> Self {
        let content = Arc::new(RwLock::new(ElementContent {
            value: content.into(),
            formatted_children: None,
        }));
        *self.styles.latest.write() = Arc::downgrade(&content);
        Self {
            kind: self.kind,
            content,
            styles: Arc::clone(&self.styles),
            repository: self.repository.clone(),
            defaults: self.defaults.clone(),
        }
    }

    pub fn configure<T>(
        &mut self,
        callback: impl FnOnce(&mut TerminalStyles<'_>) -> Result<T, ConsoleError>,
    ) -> Result<T, ConsoleError> {
        let mut style = self.styles.style.try_write().ok_or_else(style_busy)?;
        let mut scope =
            TerminalStyles::borrow(&mut style, self.repository.as_ref(), self.kind.tag(), 0);
        if let Some(defaults) = &self.defaults {
            scope.set_default_classes(defaults);
        }
        callback(&mut scope)
    }

    pub fn get_properties(&self) -> Result<Map<String, Value>, ConsoleError> {
        let style = self.styles.style.try_read().ok_or_else(style_busy)?;
        Ok(style
            .properties
            .as_deref()
            .expect("终端元素始终拥有属性")
            .clone())
    }

    pub fn set_properties(&mut self, value: Map<String, Value>) -> Result<&mut Self, ConsoleError> {
        let mut style = self.styles.style.try_write().ok_or_else(style_busy)?;
        properties::set(&mut style, value)?;
        drop(style);
        Ok(self)
    }

    pub fn with(&mut self, value: Map<String, Value>) -> Result<&mut Self, ConsoleError> {
        let mut style = self.styles.style.try_write().ok_or_else(style_busy)?;
        let mut current = style
            .properties
            .as_deref()
            .expect("终端元素始终拥有属性")
            .clone();
        properties::merge(&mut current, value)?;
        properties::set(&mut style, current)?;
        drop(style);
        Ok(self)
    }

    /// 对指定文本或最近关联的存活元素测量显示列数。
    pub fn get_length(&self, text: Option<&str>) -> Result<i64, ConsoleError> {
        if let Some(text) = text {
            return Ok(length(text));
        }
        let latest = self.styles.latest.read().upgrade();
        let content = latest.as_deref().unwrap_or(&self.content);
        self.render_markup(content).map(|markup| length(&markup))
    }

    pub fn get_inner_width(&self) -> Result<i64, ConsoleError> {
        let content_length = self.get_length(None)?;
        let style = self.styles.style.try_read().ok_or_else(style_busy)?;
        let [_, right, _, left] = style.margins(false, [0; 2]);
        Ok(content_length - (right + left) as i64)
    }

    /// 返回用于输出格式器的带标记文本。
    pub fn to_markup(&mut self) -> Result<String, ConsoleError> {
        self.render_markup(&self.content)
    }

    fn render_markup(&self, content: &RwLock<ElementContent>) -> Result<String, ConsoleError> {
        let viewport = Terminal::new().get_width();
        if !(0..=1_000_000).contains(&viewport) {
            return Err(ConsoleError::new("组件终端宽度超出安全范围"));
        }
        let mut prepared = self.prepare(content, viewport, 0)?;
        let output = prepared.render(viewport)?;
        if self.kind != TerminalElementKind::Raw {
            let mut content = content.write();
            if matches!(content.value, TerminalContent::Children(_)) {
                content.formatted_children = prepared.content;
            }
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

    fn prepare(
        &self,
        content: &RwLock<ElementContent>,
        viewport: i64,
        depth: usize,
    ) -> Result<Prepared, ConsoleError> {
        if depth > 128 {
            return Err(ConsoleError::new("终端元素嵌套超过可渲染深度"));
        }
        let content = content.read();
        if self.kind == TerminalElementKind::Raw {
            let mut value = String::new();
            match &content.value {
                TerminalContent::Text(text) => value.push_str(text),
                TerminalContent::Children(children) => {
                    for child in children {
                        match child {
                            TerminalChild::Text(text) => value.push_str(text),
                            TerminalChild::Element(element) => value.push_str(
                                &element
                                    .prepare(&element.content, viewport, depth + 1)?
                                    .render(viewport)?,
                            ),
                        }
                    }
                }
            }
            return Ok(raw(value));
        }
        let style = self.styles.style.try_read().ok_or_else(style_busy)?.clone();
        let first =
            properties::format_first(style.properties.as_deref().expect("终端元素始终拥有属性"))?;
        let mut children = vec![];
        let markup = if let Some(prepared) = &content.formatted_children {
            Some(prepared.clone())
        } else {
            match &content.value {
                TerminalContent::Text(text) => Some(text.clone()),
                TerminalContent::Children(items) => {
                    children.reserve(items.len());
                    for child in items {
                        children.push(match child {
                            TerminalChild::Text(text) => raw(text.clone()),
                            TerminalChild::Element(child) => {
                                child.prepare(&child.content, viewport, depth + 1)?
                            }
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
            content: markup,
            parent: None,
            spacing: [0; 2],
            first,
            br: self.kind == TerminalElementKind::BreakLine,
        })
    }
}

fn length(text: &str) -> i64 {
    super::super::display::visible_width(text)
        .try_into()
        .unwrap_or(i64::MAX)
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
