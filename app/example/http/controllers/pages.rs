//! 通过公开 Views 服务渲染本模块模板。

use super::super::resources::about::AboutPageContext;
use openrust::prelude::*;

pub(crate) async fn about_page(
    Inject(views): Inject<openrust::view::Views>,
) -> openrust::Result<openrust::http::axum::response::Html<String>> {
    views.render(
        "pages/about.html",
        &AboutPageContext {
            app_name: env!("CARGO_PKG_NAME"),
            title: "关于示例应用",
        },
    )
}
