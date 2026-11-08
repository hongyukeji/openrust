//! 按请求写入一条固定的示例日志。

use openrust::container::attributes::Log;
use openrust::container::{ClassAttribute, ClassDescriptor, ConstructorParam};
use openrust::http::{Inject, axum::http::StatusCode};
use openrust::logging::{LogLevel, Logger};
use std::sync::Arc;

pub(crate) struct ExampleLogger(Arc<Logger>);

pub(crate) fn descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete::<ExampleLogger>(
        "Modules/Example/ExampleLogger",
        vec![ConstructorParam::class::<Logger>("logger").with_attribute(
            ClassAttribute::contextual(|| Ok(Log::new("single").named("example.request"))),
        )],
        |args| Ok(Arc::new(ExampleLogger(args.service::<Logger>(0)?))),
    )
}

pub(crate) async fn record(Inject(logger): Inject<ExampleLogger>) -> openrust::Result<StatusCode> {
    logger
        .0
        .log(LogLevel::Info, "示例日志请求已处理", serde_json::Map::new())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
