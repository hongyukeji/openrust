use super::{View, ViewResult};

/// The rendering target of an application view component.
pub enum ComponentOutput {
    View(View),
    InlineTemplate(String),
}

/// A class-based component exposed to the template engine.
pub trait Component: Send + Sync {
    fn render(&self) -> ViewResult<ComponentOutput>;
}
