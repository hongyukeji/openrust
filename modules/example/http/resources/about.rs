//! 关于页面的类型化上下文。

#[derive(serde::Serialize)]
pub(crate) struct AboutPageContext {
    pub(crate) app_name: &'static str,
    pub(crate) title: &'static str,
    pub(crate) description: minijinja::Value,
}
