//! Contracts for views, factories, and render engines.

use std::{path::Path, sync::Arc};

use crate::view::{ViewData, ViewResult, ViewValue};

/// A view and its rendering data.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub trait View: Send + Sync {
    fn name(&self) -> &str;
    fn with(&mut self, key: &str, value: ViewValue) -> &mut dyn View;
    fn get_data(&self) -> &ViewData;
    fn render(&mut self) -> ViewResult<String>;
}

/// Configures a view when it is created or rendered.
pub type ViewCallback = dyn Fn(&mut dyn View) -> ViewResult<()> + Send + Sync;

/// Creates views with application-specific configuration.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub trait Factory: Send + Sync {
    fn exists(&self, view: &str) -> bool;
    fn file(&self, path: &Path, data: ViewData, merge_data: ViewData) -> ViewResult<Box<dyn View>>;
    fn make(&self, view: &str, data: ViewData, merge_data: ViewData) -> ViewResult<Box<dyn View>>;
    fn share(&self, key: &str, value: ViewValue) -> ViewValue;
    fn composer(&self, views: &[&str], callback: Arc<ViewCallback>);
    fn creator(&self, views: &[&str], callback: Arc<ViewCallback>);
    fn addNamespace(&self, namespace: &str, hints: &[&Path]) -> ViewResult<()>;
    fn replaceNamespace(&self, namespace: &str, hints: &[&Path]) -> ViewResult<()>;
}

/// Renders a file within the supplied view root.
pub trait Engine: Send + Sync {
    fn get(&self, root: &Path, path: &Path, data: &ViewData) -> ViewResult<String>;
}
