//! 按请求写入一条固定的示例日志。

use openrust::container::attributes::{Context, Log};
use openrust::container::{ClassAttribute, ClassDescriptor, ConstructorParam};
use openrust::foundation::ApplicationState;
use openrust::http::axum::{extract::State, http::StatusCode};
use openrust::logging::{LogLevel, Logger, context::Repository};
use std::sync::Arc;

const OPERATION_KEY: &str = "example_operation";
const OPERATION: &str = "logging-demo";
const INTERNAL_KEY: &str = "example_internal_marker";
const INTERNAL_MARKER: &str = "example-private-marker";

pub(crate) struct ExampleLogger {
    logger: Arc<Logger>,
    operation: Arc<String>,
    internal_marker: Arc<String>,
}

pub(crate) fn descriptor() -> ClassDescriptor {
    ClassDescriptor::concrete::<ExampleLogger>(
        "Modules/Example/ExampleLogger",
        vec![
            ConstructorParam::class::<Logger>("logger").with_attribute(ClassAttribute::contextual(
                || Ok(Log::new("single").named("example.request")),
            )),
            ConstructorParam::scalar::<String>("operation").with_attribute(
                ClassAttribute::contextual(|| {
                    Ok(Context::<String>::new(OPERATION_KEY).with_default(String::new()))
                }),
            ),
            ConstructorParam::scalar::<String>("internal_marker").with_attribute(
                ClassAttribute::contextual(|| {
                    Ok(Context::<String>::new(INTERNAL_KEY)
                        .hidden()
                        .with_default(String::new()))
                }),
            ),
        ],
        |args| {
            Ok(Arc::new(ExampleLogger {
                logger: args.service::<Logger>(0)?,
                operation: args.value(1)?.downcast()?,
                internal_marker: args.value(2)?.downcast()?,
            }))
        },
    )
}

pub(crate) async fn record(State(app): State<ApplicationState>) -> openrust::Result<StatusCode> {
    let context = app.resolve::<Repository>().await?;
    context.add(OPERATION_KEY, serde_json::json!(OPERATION));
    context.add_hidden(INTERNAL_KEY, serde_json::json!(INTERNAL_MARKER));
    let service = app.resolve::<ExampleLogger>().await?;
    if service.operation.as_str() != OPERATION
        || service.internal_marker.as_str() != INTERNAL_MARKER
    {
        return Err(openrust::Error::custom(
            "example_log_context_mismatch",
            "示例日志缺少请求上下文",
        ));
    }
    service
        .logger
        .log(LogLevel::Info, "示例日志请求已处理", serde_json::Map::new())
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
