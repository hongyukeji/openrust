// 第三方许可见 ../LICENSE.renderer。
use super::{
    StyleArgument,
    style::{self, Style},
};
use crate::console::ConsoleError;
use parking_lot::RwLock;
use std::{collections::HashMap, sync::Arc};

/// 自定义样式操作当前元素的实际设置，共用渲染器的布局和格式化管线。
pub struct TerminalStyles<'a> {
    state: &'a mut Style,
    repository: Option<&'a StyleRepository>,
    tag: &'a str,
    depth: usize,
}

impl<'a> TerminalStyles<'a> {
    pub(super) fn new(
        state: &'a mut Style,
        repository: Option<&'a StyleRepository>,
        tag: &'a str,
        depth: usize,
    ) -> Self {
        Self {
            state,
            repository,
            tag,
            depth,
        }
    }

    pub fn apply(&mut self, classes: &str) -> Result<&mut Self, ConsoleError> {
        style::apply(self.state, classes, self.tag, self.depth, self.repository)?;
        Ok(self)
    }

    pub fn uppercase(&mut self) -> &mut Self {
        style::transform(self.state, "upper");
        self
    }

    pub fn font_bold(&mut self) -> &mut Self {
        self.state.bold = Some(true);
        self
    }
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
        let mut styles = TerminalStyles::new(
            styles.state,
            styles.repository,
            styles.tag,
            styles.depth + 1,
        );
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
