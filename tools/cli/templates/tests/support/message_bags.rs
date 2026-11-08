//! 生成应用通过公开 Provider 挂载业务路由，消费错误消息包契约。
use figment::Figment;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode},
};
use openrust::http::Json;
use openrust::routing::Router;
use openrust::support::{
    ArrayKey, Collection, MessageBag, MessageInput, MessageJsonOptions, MessageKeys, MessageLookup,
    MessageValue, ViewErrorBag,
};
use openrust::support::{BoxFuture, ServiceProvider};
use openrust::testing::TestApp;
use serde_json::{Value, json};
use std::time::Duration;

const PATH: &str = "/__framework_contract/message-bags";

struct MessageBagRoutes;
impl ServiceProvider for MessageBagRoutes {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes: Router| routes.get(PATH, inspect_message_bags))?;
            Ok(())
        })
    }
}

// 测试扩展保留生成应用原有 Provider 与模块清单。
struct MessageBagApplication<A>(std::marker::PhantomData<fn() -> A>);
impl<A: ApplicationDefinition> ApplicationDefinition for MessageBagApplication<A> {
    type Config = A::Config;

    fn config() -> openrust::config::ConfigManifest {
        A::config()
    }
    fn default_providers() -> openrust::support::DefaultProviders {
        A::default_providers()
    }
    fn package_providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        A::package_providers(config)
    }
    fn additional_providers() -> Vec<Box<dyn ServiceProvider>> {
        A::additional_providers()
    }
    fn with_bootstrap_providers() -> bool {
        A::with_bootstrap_providers()
    }

    fn config_namespace() -> &'static str {
        A::config_namespace()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = A::providers(config);
        providers.push(Box::new(MessageBagRoutes));
        providers
    }

    fn configure_providers(config: &Self::Config, builder: &mut openrust::foundation::ProviderBuilder) {
        A::configure_providers(config, builder);
    }
}

async fn inspect_message_bags() -> openrust::Result<Json<Value>> {
    let empty = MessageBag::new();
    let messages = Collection::try_from_entries([
        (ArrayKey::from(0_i64), "required".to_owned()),
        (ArrayKey::from(1_i64), "required".to_owned()),
        (ArrayKey::from(2_i64), "汉/a".to_owned()),
    ])?;
    let mut bag =
        MessageBag::try_from_messages([(ArrayKey::from("email"), MessageInput::Many(messages))])?;
    bag.add("email", "required")?;
    bag.add("email", "invalid")?;
    let raw = bag.messages().to_value()?;
    let formatted = match bag.get("email", Some("[:key] :message"))? {
        MessageLookup::Exact(values) => values.to_value()?,
        _ => {
            return Err(openrust::Error::custom(
                "generated_message_bag_shape",
                "消息形状不符",
            ));
        }
    };
    let first = bag.first(Some("email"), None)?;
    let has = bag.has(MessageKeys::Keys(vec![ArrayKey::from("email")]))?;
    let json = bag.to_json(
        MessageJsonOptions::UNESCAPED_UNICODE.union(MessageJsonOptions::UNESCAPED_SLASHES),
    )?;

    // 递归合并同名内层键会产生真实嵌套值；按 PHP 边界读取各方法结果。
    let mut nested = MessageBag::new();
    for message in ["x", "y"] {
        let incoming = Collection::try_from_entries([(
            ArrayKey::from("field"),
            Collection::try_from_entries([(
                ArrayKey::from("label"),
                MessageValue::Text(message.to_owned()),
            )])?,
        )])?;
        nested.merge(&incoming)?;
    }
    let nested_get = match nested.get("field", None)? {
        MessageLookup::Exact(values) => values.to_value()?,
        _ => {
            return Err(openrust::Error::custom(
                "generated_nested_message_shape",
                "嵌套消息形状不符",
            ));
        }
    };
    let nested_raw = nested.messages().to_value()?;
    let nested_json = nested.to_json(MessageJsonOptions::NONE)?;
    let nested_count = nested.count();
    let nested_first = nested.first(Some("field"), None)?;
    let nested_all_error = nested.all(None).is_err();

    let numeric = MessageBag::try_from_messages([(
        ArrayKey::from("numeric"),
        MessageInput::Many(Collection::try_from_entries([
            (ArrayKey::from(0_i64), "1e1000".to_owned()),
            (ArrayKey::from(1_i64), "1e-1000".to_owned()),
            (ArrayKey::from(2_i64), "1.0".to_owned()),
        ])?),
    )])?;
    let numeric_json = numeric.to_json(MessageJsonOptions::NUMERIC_CHECK)?;
    let mut numeric_add = MessageBag::new();
    for message in [
        "9223372036854775808",
        "9223372036854775809",
        "9223372036854775808",
    ] {
        numeric_add.add("reference", message)?;
    }

    let mut errors = ViewErrorBag::new();
    errors.put("sign.in", Box::new(bag))?;
    let named = errors
        .get_bag("sign.in")
        .as_bag()
        .first(Some("email"), None)?;
    let missing = errors.get_bag("missing").as_bag().is_empty();
    let count_without_default = errors.count();
    errors.put(
        "default",
        Box::new(MessageBag::try_from_messages([(
            ArrayKey::from("title"),
            MessageInput::One("missing".to_owned()),
        )])?),
    )?;

    Ok(Json(json!({
        "empty_json": empty.to_json(MessageJsonOptions::NONE)?,
        "raw": raw,
        "formatted": formatted,
        "first": first,
        "has": has,
        "json": json,
        "named": named,
        "nested_raw": nested_raw,
        "nested_get": nested_get,
        "nested_json": nested_json,
        "nested_count": nested_count,
        "nested_first": nested_first,
        "nested_all_error": nested_all_error,
        "numeric_json": numeric_json,
        "numeric_add": numeric_add.messages().to_value()?,
        "missing_is_empty": missing,
        "named_exists": errors.has_bag("sign.in"),
        "count_without_default": count_without_default,
        "default_count": errors.count(),
        "default_first": errors.default_first(Some("title"), None)?,
    })))
}

/// 经生成应用的真实 Provider 与 TestApp 请求入口验证消息包公开能力。
pub async fn assert_message_bags<A: ApplicationDefinition>(
    source: Figment,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = TestApp::<MessageBagApplication<A>>::builder(source)
        .build()
        .await?;
    let result: Result<(), Box<dyn std::error::Error>> = async {
        let response = app
            .request(
                Request::builder()
                    .uri(PATH)
                    .header("host", "example.test")
                    .body(Body::empty())?,
            )
            .await?;
        response.assert_status(StatusCode::OK)?.assert_json(&json!({
            "success": true,
            "code": "ok",
            "message": "ok",
            "data": {
                "empty_json": "[]",
                "raw": {"email": {"0": "required", "2": "汉/a", "3": "invalid"}},
                "formatted": {"0": "[email] required", "2": "[email] 汉/a", "3": "[email] invalid"},
                "first": "required",
                "has": true,
                "json": "{\"email\":{\"0\":\"required\",\"2\":\"汉/a\",\"3\":\"invalid\"}}",
                "named": "required",
                "nested_raw": {"field": {"label": ["x", "y"]}},
                "nested_get": {"label": ["x", "y"]},
                "nested_json": "{\"field\":{\"label\":[\"x\",\"y\"]}}",
                "nested_count": 3,
                "nested_first": "x",
                "nested_all_error": true,
                "numeric_json": "{\"numeric\":[\"1e1000\",0,1]}",
                "numeric_add": {"reference": ["9223372036854775808", "9223372036854775809"]},
                "missing_is_empty": true,
                "named_exists": true,
                "count_without_default": 0,
                "default_count": 1,
                "default_first": "missing"
            }
        }))?;
        Ok(())
    }
    .await;
    let shutdown = app
        .shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    result?;
    shutdown?;
    Ok(())
}
