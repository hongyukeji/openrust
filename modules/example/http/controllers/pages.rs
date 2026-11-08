//! 通过公开 Views 服务渲染本模块模板。

use super::super::resources::about::AboutPageContext;
use openrust::{
    database::eloquent::{
        AsHtmlString, AttributeValue, ModelAttributes, ModelHtmlString, ModelMetadata, ModelObject,
    },
    prelude::*,
    support::Htmlable,
};

pub(crate) async fn about_page(
    Inject(views): Inject<openrust::view::Views>,
    info: openrust::http::RequestInfo,
) -> openrust::Result<openrust::http::axum::response::Html<String>> {
    let description = about_description()?;
    views.render_for_request(
        "example::pages/about.html",
        &info,
        &AboutPageContext {
            app_name: env!("CARGO_PKG_NAME"),
            title: "关于示例应用",
            description: description.to_view_value()?,
        },
    )
}

fn about_description() -> openrust::Result<ModelHtmlString> {
    let mut metadata = ModelMetadata::new("pages", "AboutPage");
    metadata
        .casts
        .insert("description".into(), AsHtmlString::cast());
    let attributes = ModelAttributes::default();
    // 模板文案由应用提供，用户输入仍经模板自动转义。
    attributes.set_raw_attribute(
        "description",
        serde_json::json!(
            "此页面由业务路由注入 <code>Views</code>，并通过 <code>pages/about.html</code> 渲染。"
        ),
    )?;
    let value = attributes.get_attribute_value(&metadata, "description")?;
    let object = value.as_ref().and_then(AttributeValue::as_object);
    let snapshot = object.map(ModelObject::snapshot).transpose()?;
    snapshot
        .as_deref()
        .and_then(|value| value.as_any().downcast_ref::<ModelHtmlString>())
        .cloned()
        .ok_or_else(|| openrust::Error::custom("example_about_description", "缺少关于页 HTML 内容"))
}
