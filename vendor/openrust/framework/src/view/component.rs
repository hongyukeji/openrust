use super::{View, ViewData, ViewResult};

/// The rendering target of an application view component.
pub enum ComponentOutput {
    View(View),
    InlineTemplate(String),
}

/// A class-based component exposed to the template engine.
pub trait Component: Send + Sync {
    /// Controls whether the component and its slots render.
    #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
    fn shouldRender(&self) -> bool {
        true
    }

    /// Data exposed to the component template and its scoped slots.
    fn data(&self) -> ViewData {
        ViewData::new()
    }

    fn render(&self) -> ViewResult<ComponentOutput>;
}

pub(super) enum ResolvedComponent {
    Hidden,
    Visible {
        output: Box<ComponentOutput>,
        data: ViewData,
    },
}
