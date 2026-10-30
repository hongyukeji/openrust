//! 通过公开 Views 服务渲染本模块模板。

use super::super::resources::about::AboutPageContext;
use openrust::prelude::*;

pub(crate) async fn about_page(
    Inject(views): Inject<openrust::view::Views>,
    info: openrust::http::RequestInfo,
) -> openrust::Result<openrust::http::axum::response::Html<String>> {
    views.render_for_request(
        "example::pages/about.html",
        &info,
        &AboutPageContext {
            app_name: env!("CARGO_PKG_NAME"),
            title: "关于示例应用",
        },
    )
}
