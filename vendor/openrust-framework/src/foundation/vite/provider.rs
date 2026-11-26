use super::{Execution, Vite};
use crate::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
    view::{Factory, ViewError, ViewValue, compilers::DirectiveOutput},
};
use std::sync::Arc;

pub struct ViteServiceProvider;
impl ServiceProvider for ViteServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let owner = Arc::downgrade(&app.container_handle());
            let container = app.container();
            container.singleton::<Vite, _>(move |_| Ok(Arc::new(Vite::new(owner.clone()))))?;
            container.scoped::<Execution, _>(|container| {
                Ok(Arc::new(container.make::<Vite>()?.defaults.snapshot()))
            })?;
            container.after_resolving(Some("view".into()), |event, container| {
                let Some(factory) = event.value::<Factory>() else {
                    return Ok(());
                };
                let owner = Arc::downgrade(&container.make::<Application>()?.container_handle());
                let render_owner = owner.clone();
                factory
                    .compiler()
                    .builtin_directive("vite", move |arguments| {
                        let Some(first) = arguments.first() else {
                            return Err(invalid("资源指令需要入口参数"));
                        };
                        let entries = match first {
                            ViewValue::String(entry) => vec![entry.clone()],
                            ViewValue::Array(entries) => entries
                                .iter()
                                .map(|e| {
                                    e.as_str()
                                        .map(str::to_owned)
                                        .ok_or_else(|| invalid("资源入口必须是字符串"))
                                })
                                .collect::<Result<_, _>>()?,
                            _ => return Err(invalid("资源入口必须是字符串或字符串数组")),
                        };
                        let build = arguments
                            .get(1)
                            .filter(|v| !v.is_null())
                            .map(|v| v.as_str().ok_or_else(|| invalid("构建目录必须是字符串")))
                            .transpose()?;
                        let service = render_owner
                            .upgrade()
                            .ok_or_else(|| invalid("资源所属应用已结束"))?
                            .make::<Vite>()
                            .map_err(|e| ViewError::RenderFailed(Box::new(e)))?;
                        let html = service
                            .render(entries, build)
                            .map_err(|e| ViewError::RenderFailed(Box::new(e)))?;
                        Ok(DirectiveOutput::Html(html.to_html().to_owned()))
                    })
                    .map_err(binding)?;
                factory
                    .compiler()
                    .builtin_directive("viteReactRefresh", move |_| {
                        let service = owner
                            .upgrade()
                            .ok_or_else(|| invalid("资源所属应用已结束"))?
                            .make::<Vite>()
                            .map_err(|e| ViewError::RenderFailed(Box::new(e)))?;
                        let html = service
                            .react_refresh()
                            .map_err(|e| ViewError::RenderFailed(Box::new(e)))?;
                        Ok(DirectiveOutput::Html(
                            html.map_or_else(String::new, |html| html.to_html().to_owned()),
                        ))
                    })
                    .map_err(binding)?;
                Ok(())
            })?;
            Ok(())
        })
    }
}
fn invalid(message: &str) -> ViewError {
    ViewError::RenderFailed(Box::new(std::io::Error::other(message.to_owned())))
}
fn binding(source: ViewError) -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service: "Vite",
        source: Box::new(source),
    }
}
