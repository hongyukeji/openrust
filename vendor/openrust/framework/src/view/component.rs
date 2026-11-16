use super::{View, ViewData, ViewResult};

/// The rendering target of an application view component.
pub enum ComponentOutput {
    View(View),
    InlineTemplate(String),
}

/// A class-based component exposed to the template engine.
pub trait Component: Send + Sync {
    /// Data exposed to the component template and its scoped slots.
    fn data(&self) -> ViewData {
        ViewData::new()
    }

    fn render(&self) -> ViewResult<ComponentOutput>;
}

pub(super) struct ResolvedComponent {
    pub output: ComponentOutput,
    pub data: ViewData,
}
