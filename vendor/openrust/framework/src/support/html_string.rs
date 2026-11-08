//! Laravel 的可信 HTML 内容契约。

use std::{any::Any, fmt};

/// 提供可直接用于 HTML 文本位置的可信标记。
pub trait Htmlable {
    /// 借用 HTML 标记。
    fn to_html(&self) -> &str;

    /// 保留 HTML 标记进入模板。调用方负责确保内容可信且用于 HTML 文本位置。
    #[cfg(feature = "view")]
    fn to_view_value(&self) -> minijinja::Value {
        minijinja::Value::from_safe_string(self.to_html().to_owned())
    }
}

/// 包装应用信任的 HTML。构造时保留原文。
///
/// ```
/// use openrust::support::{HtmlString, Htmlable};
///
/// let html = HtmlString::new("<em>示例</em>");
/// assert_eq!(html.to_html(), "<em>示例</em>");
/// assert!(html.is_not_empty());
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HtmlString(String);

impl HtmlString {
    /// 将可信标记保存为字符串。
    pub fn new(html: impl Into<String>) -> Self {
        Self(html.into())
    }

    /// 判断是否为空字符串。空白字符属于内容。
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 判断是否含有内容。
    pub fn is_not_empty(&self) -> bool {
        !self.is_empty()
    }
}

impl Htmlable for HtmlString {
    fn to_html(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HtmlString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.to_html())
    }
}

impl super::DataObject for HtmlString {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn string_projection(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

#[cfg(all(test, feature = "view"))]
mod tests {
    use super::{HtmlString, Htmlable};

    #[test]
    fn htmlable_preserves_only_explicitly_trusted_template_content() -> Result<(), minijinja::Error>
    {
        let mut views = minijinja::Environment::new();
        views.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);
        let html = "<strong>HTML &amp; 内容</strong>";
        let context = minijinja::context! {
            trusted => HtmlString::new(html).to_view_value(),
            plain => html,
        };
        let output = views.render_str("{{ trusted }}\n{{ plain }}", context)?;
        let mut lines = output.lines();
        assert_eq!(lines.next(), Some(html));
        let escaped = lines.next().unwrap_or_default();
        assert!(escaped.starts_with("&lt;strong&gt;"));
        assert!(escaped.contains("&amp;amp;"));
        assert!(!escaped.contains('<'));
        Ok(())
    }
}
