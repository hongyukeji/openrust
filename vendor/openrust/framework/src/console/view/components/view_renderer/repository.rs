// 第三方许可见 ../LICENSE.renderer。
use super::style_methods::MethodCall;
use super::{
    StyleArgument,
    style::{self, Style},
};
use crate::console::{ConsoleError, Terminal};
use parking_lot::RwLock;
use std::{borrow::Cow, collections::HashMap, sync::Arc};

enum StyleStorage<'a> {
    Borrowed(&'a mut Style),
    Owned(Box<Style>),
}

impl StyleStorage<'_> {
    fn get(&self) -> &Style {
        match self {
            Self::Borrowed(style) => style,
            Self::Owned(style) => style,
        }
    }

    fn get_mut(&mut self) -> &mut Style {
        match self {
            Self::Borrowed(style) => style,
            Self::Owned(style) => style,
        }
    }
}

/// 自定义样式操作当前元素的实际设置，共用渲染器的布局和格式化管线。
pub struct TerminalStyles<'a> {
    state: StyleStorage<'a>,
    repository: Option<Cow<'a, StyleRepository>>,
    defaults: Option<Cow<'a, [String]>>,
    tag: &'a str,
    depth: usize,
}

impl<'a> TerminalStyles<'a> {
    pub(super) fn borrow(
        state: &'a mut Style,
        repository: Option<&'a StyleRepository>,
        tag: &'a str,
        depth: usize,
    ) -> Self {
        Self {
            state: StyleStorage::Borrowed(state),
            repository: repository.map(Cow::Borrowed),
            defaults: None,
            tag,
            depth,
        }
    }

    fn reborrow(&mut self) -> TerminalStyles<'_> {
        TerminalStyles {
            state: StyleStorage::Borrowed(self.state.get_mut()),
            repository: self.repository.as_deref().map(Cow::Borrowed),
            defaults: self.defaults.as_deref().map(Cow::Borrowed),
            tag: self.tag,
            depth: self.depth + 1,
        }
    }

    pub(super) fn set_default_classes(&mut self, defaults: &'a [String]) {
        self.defaults = Some(Cow::Borrowed(defaults));
    }

    pub fn default_styles(&self) -> impl Iterator<Item = &str> {
        let implicit = if self.defaults.is_none() {
            style::default_classes(self.tag)
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

    pub fn apply(&mut self, classes: &str) -> Result<&mut Self, ConsoleError> {
        style::apply_with_defaults(
            self.state.get_mut(),
            classes,
            self.tag,
            self.depth,
            self.repository.as_deref(),
            self.defaults.as_deref(),
        )?;
        Ok(self)
    }

    pub fn set_style(&mut self, name: &str) -> &mut Self {
        style::mark(self.state.get_mut(), name);
        self
    }

    pub fn has_style(&self, name: &str) -> bool {
        self.state.get().markers.iter().any(|style| style == name)
    }

    fn change(
        &mut self,
        name: &str,
        args: &[StyleArgument<'_>],
    ) -> Result<&mut Self, ConsoleError> {
        style::apply_method(
            self.state.get_mut(),
            MethodCall { name, args },
            self.repository.as_deref(),
            name,
        )?;
        Ok(self)
    }

    pub fn text(&mut self, value: &str) -> Result<&mut Self, ConsoleError> {
        self.text_with(value, 0)
    }

    pub fn text_with(&mut self, value: &str, variant: i64) -> Result<&mut Self, ConsoleError> {
        self.change(
            "text",
            &[StyleArgument::Text(value), StyleArgument::Integer(variant)],
        )
    }

    pub fn bg(&mut self, color: &str) -> Result<&mut Self, ConsoleError> {
        self.bg_with(color, 0)
    }

    pub fn bg_with(&mut self, color: &str, variant: i64) -> Result<&mut Self, ConsoleError> {
        self.change(
            "bg",
            &[StyleArgument::Text(color), StyleArgument::Integer(variant)],
        )
    }

    pub fn truncate(&mut self) -> Result<&mut Self, ConsoleError> {
        self.truncate_with(0, "…")
    }

    pub fn truncate_with(&mut self, limit: i64, suffix: &str) -> Result<&mut Self, ConsoleError> {
        self.change(
            "truncate",
            &[StyleArgument::Integer(limit), StyleArgument::Text(suffix)],
        )
    }

    pub fn list(&mut self, kind: &str) -> Result<&mut Self, ConsoleError> {
        self.list_with(kind, 0)
    }

    pub fn list_with(&mut self, kind: &str, index: i64) -> Result<&mut Self, ConsoleError> {
        self.change(
            "list",
            &[StyleArgument::Text(kind), StyleArgument::Integer(index)],
        )
    }

    pub fn border_t(&mut self) -> Result<&mut Self, ConsoleError> {
        self.border_t_with(1)
    }

    pub fn border_t_with(&mut self, width: i64) -> Result<&mut Self, ConsoleError> {
        self.change("bordert", &[StyleArgument::Integer(width)])
    }

    pub fn w(&mut self, width: StyleArgument<'_>) -> Result<&mut Self, ConsoleError> {
        self.change("w", &[width])
    }

    pub fn min_w(&mut self, width: StyleArgument<'_>) -> Result<&mut Self, ConsoleError> {
        self.change("minw", &[width])
    }

    pub fn max_w(&mut self, width: StyleArgument<'_>) -> Result<&mut Self, ConsoleError> {
        self.change("maxw", &[width])
    }

    pub fn add_style(&mut self, classes: &str) -> Result<&mut Self, ConsoleError> {
        self.apply(classes)
    }

    pub fn uppercase(&mut self) -> &mut Self {
        style::transform(self.state.get_mut(), "upper");
        self
    }

    pub fn font_bold(&mut self) -> &mut Self {
        self.state.get_mut().bold = Some(true);
        self
    }

    pub fn ml(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("ml", &[StyleArgument::Integer(columns)])
    }

    pub fn mr(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("mr", &[StyleArgument::Integer(columns)])
    }

    pub fn mb(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("mb", &[StyleArgument::Integer(columns)])
    }

    pub fn mt(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("mt", &[StyleArgument::Integer(columns)])
    }

    pub fn mx(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("mx", &[StyleArgument::Integer(columns)])
    }

    pub fn my(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("my", &[StyleArgument::Integer(columns)])
    }

    pub fn m(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("m", &[StyleArgument::Integer(columns)])
    }

    pub fn pl(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("pl", &[StyleArgument::Integer(columns)])
    }

    pub fn pr(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("pr", &[StyleArgument::Integer(columns)])
    }

    pub fn px(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("px", &[StyleArgument::Integer(columns)])
    }

    pub fn pt(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("pt", &[StyleArgument::Integer(columns)])
    }

    pub fn pb(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("pb", &[StyleArgument::Integer(columns)])
    }

    pub fn py(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("py", &[StyleArgument::Integer(columns)])
    }

    pub fn p(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("p", &[StyleArgument::Integer(columns)])
    }

    pub fn space_x(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("spacex", &[StyleArgument::Integer(columns)])
    }

    pub fn space_y(&mut self, columns: i64) -> Result<&mut Self, ConsoleError> {
        self.change("spacey", &[StyleArgument::Integer(columns)])
    }

    pub fn italic(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("italic", &[])
    }

    pub fn strong(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("strong", &[])
    }

    pub fn underline(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("underline", &[])
    }

    pub fn line_through(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("linethrough", &[])
    }

    pub fn invisible(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("invisible", &[])
    }

    pub fn font_normal(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("fontnormal", &[])
    }

    pub fn lowercase(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("lowercase", &[])
    }

    pub fn capitalize(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("capitalize", &[])
    }

    pub fn snakecase(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("snakecase", &[])
    }

    pub fn block(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("block", &[])
    }

    pub fn hidden(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("hidden", &[])
    }

    pub fn flex(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("flex", &[])
    }

    pub fn w_full(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("wfull", &[])
    }

    pub fn w_auto(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("wauto", &[])
    }

    pub fn justify_between(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("justifybetween", &[])
    }

    pub fn justify_around(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("justifyaround", &[])
    }

    pub fn justify_evenly(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("justifyevenly", &[])
    }

    pub fn justify_center(&mut self) -> Result<&mut Self, ConsoleError> {
        self.change("justifycenter", &[])
    }

    pub fn content_repeat(&mut self, value: &str) -> Result<&mut Self, ConsoleError> {
        self.change("contentrepeat", &[StyleArgument::Text(value)])
    }

    pub fn prepend(&mut self, value: &str) -> Result<&mut Self, ConsoleError> {
        self.change("prepend", &[StyleArgument::Text(value)])
    }

    pub fn append(&mut self, value: &str) -> Result<&mut Self, ConsoleError> {
        self.change("append", &[StyleArgument::Text(value)])
    }

    pub fn href(&mut self, value: &str) -> Result<&mut Self, ConsoleError> {
        self.change("href", &[StyleArgument::Text(value)])
    }
}

impl Default for TerminalStyles<'static> {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalStyles<'static> {
    /// 不绑定任何元素的独立样式，可直接格式化文本。
    pub fn new() -> Self {
        Self {
            state: StyleStorage::Owned(Box::new(Style {
                element: "generic",
                display: "inline",
                ..Style::default()
            })),
            repository: None,
            defaults: None,
            tag: "span",
            depth: 0,
        }
    }

    pub fn with_default_styles(
        mut self,
        defaults: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.defaults = Some(Cow::Owned(defaults.into_iter().map(Into::into).collect()));
        self
    }

    pub fn with_repository(mut self, repository: StyleRepository) -> Self {
        self.repository = Some(Cow::Owned(repository));
        self
    }

    pub fn format(&self, content: &str) -> Result<String, ConsoleError> {
        style::format(
            content.to_owned(),
            self.state.get(),
            &None,
            Terminal::new().get_width(),
            false,
            [0; 2],
        )
    }

    pub fn get_length(&self, content: Option<&str>) -> i64 {
        length(content.unwrap_or_default())
    }

    pub fn get_parent_width(
        ancestors: &[super::TerminalDimensions<'_>],
    ) -> Result<i64, ConsoleError> {
        super::dimensions::width_from_ancestors(ancestors, Terminal::new().get_width())
    }

    pub fn get_inner_width(&self) -> i64 {
        let style = self.state.get();
        let [_, right, _, left] = style.margins(false, [0; 2]);
        self.get_length(None) - (left + right) as i64
    }
}

fn length(value: &str) -> i64 {
    super::super::display::visible_width(value)
        .try_into()
        .unwrap_or(i64::MAX)
}

type Callback =
    dyn Fn(&mut TerminalStyles<'_>, &[StyleArgument<'_>]) -> Result<(), ConsoleError> + Send + Sync;

#[derive(Default)]
struct Definition {
    color: String,
    layers: Vec<Arc<str>>,
}

/// 一个可复用的样式定义。克隆句柄保留颜色、组合规则和回调的身份。
#[derive(Clone, Default)]
pub struct TerminalStyle {
    callback: Option<Arc<Callback>>,
    definition: Arc<RwLock<Definition>>,
}

impl TerminalStyle {
    pub fn new(
        callback: impl Fn(&mut TerminalStyles<'_>, &[StyleArgument<'_>]) -> Result<(), ConsoleError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::new_with_color(callback, "")
    }

    pub fn new_with_color(
        callback: impl Fn(&mut TerminalStyles<'_>, &[StyleArgument<'_>]) -> Result<(), ConsoleError>
        + Send
        + Sync
        + 'static,
        color: impl Into<String>,
    ) -> Self {
        Self {
            callback: Some(Arc::new(callback)),
            definition: Arc::new(RwLock::new(Definition {
                color: color.into(),
                layers: Vec::new(),
            })),
        }
    }

    /// 在回调之后依次应用附加的类，可以多次追加。
    pub fn apply(&self, classes: impl Into<Arc<str>>) {
        self.definition.write().layers.push(classes.into());
    }

    pub fn color(&self, value: &str) -> Result<(), ConsoleError> {
        let candidate = value.strip_suffix('\n').unwrap_or(value);
        let valid = candidate.strip_prefix('#').is_some_and(|hex| {
            matches!(hex.len(), 3 | 6) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        if !valid {
            return Err(ConsoleError::new(format!("无效的终端样式颜色：{value}")));
        }
        self.definition.write().color = value.to_owned();
        Ok(())
    }

    pub fn get_color(&self) -> String {
        self.definition.read().color.clone()
    }

    pub fn invoke(
        &self,
        styles: &mut TerminalStyles<'_>,
        arguments: &[StyleArgument<'_>],
    ) -> Result<(), ConsoleError> {
        // 组合类会重新进入解析器，在工作线程的有限栈上提前停止。
        const MAX_STYLE_DEPTH: usize = 32;
        if styles.depth >= MAX_STYLE_DEPTH {
            return Err(ConsoleError::new("组件样式递归超出范围"));
        }
        let mut styles = styles.reborrow();
        let layers = self.definition.read().layers.clone();
        if let Some(callback) = &self.callback {
            callback(&mut styles, arguments)?;
        }
        for layer in layers {
            styles.apply(&layer)?;
        }
        Ok(())
    }
}

/// 由应用或渲染器持有的注册表。不同实例可以使用相同名称配置独立样式。
#[derive(Clone, Default)]
pub struct StyleRepository(Arc<RwLock<HashMap<String, TerminalStyle>>>);

impl StyleRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self, name: impl Into<String>) -> TerminalStyle {
        self.insert(name.into(), TerminalStyle::default())
    }

    pub fn create_with(
        &self,
        name: impl Into<String>,
        callback: impl Fn(&mut TerminalStyles<'_>, &[StyleArgument<'_>]) -> Result<(), ConsoleError>
        + Send
        + Sync
        + 'static,
    ) -> TerminalStyle {
        self.insert(name.into(), TerminalStyle::new(callback))
    }

    fn insert(&self, name: String, style: TerminalStyle) -> TerminalStyle {
        let previous = self.0.write().insert(name, style.clone());
        drop(previous);
        style
    }

    pub fn flush(&self) {
        let removed = std::mem::take(&mut *self.0.write());
        drop(removed);
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.read().contains_key(name)
    }

    pub fn get(&self, name: &str) -> Result<TerminalStyle, ConsoleError> {
        self.lookup(name)
            .ok_or_else(|| ConsoleError::new(format!("终端样式未定义：{name}")))
    }

    pub(super) fn lookup(&self, name: &str) -> Option<TerminalStyle> {
        self.0.read().get(name).cloned()
    }
}
