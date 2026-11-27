use openrust::{
    foundation::configuration::Routing,
    http::{HttpError, Response, StatusCode},
    json_schema::{JsonSchemaTypeFactory, Type},
    support::facades::{Lang, Route},
    testing::TestApp,
};
use openrust_ai::{Agent, AgentTool, CanActAsTool, Tool};
use serde_json::Value;

struct ChildAgent;

impl Agent for ChildAgent {
    fn instructions(&self) -> &str {
        ""
    }
}

impl CanActAsTool for ChildAgent {
    fn name(&self) -> &str {
        "custom_agent"
    }

    fn description(&self) -> &str {
        "Custom tool description"
    }
}

#[tokio::test]
async fn child_agent_metadata_uses_the_request_locale_and_keeps_tool_contracts()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let default = AgentTool::new(ChildAgent);
    assert!(default.description().contains("ChildAgent"));
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(openrust_ai::openrust_providers())
            .with_routing(Routing::default().web(|| {
                Route::get("/tools", || async {
                    let default = AgentTool::new(ChildAgent);
                    let custom = ChildAgent.into_tool();
                    assert_eq!(default.name(), "ChildAgent");
                    assert_eq!(custom.name(), "custom_agent");
                    for locale in [
                        "ar", "de", "en", "es", "fr", "hi", "id", "it", "ja", "ko", "nl", "pt",
                        "ru", "th", "tr", "vi", "zh-Hans", "zh-Hant",
                    ] {
                        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("lang")
                            .join(locale)
                            .join("tools.json");
                        let catalog: Value =
                            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                        Lang::with_locale(locale, async {
                            let text = &catalog["agent"];
                            let expected = text["description"]
                                .as_str()
                                .unwrap()
                                .replace(":agent", "ChildAgent");
                            assert_eq!(default.localized_description(), expected, "{locale}");
                            assert_eq!(AgentTool::new(ChildAgent).description(), expected);
                            assert_eq!(custom.localized_description(), "Custom tool description");
                            for tool in [&default, &custom] {
                                let schema = Type::from(tool.schema(JsonSchemaTypeFactory))
                                    .to_array()
                                    .unwrap();
                                assert_eq!(
                                    schema["properties"]["task"]["description"],
                                    text["task"]
                                );
                                assert_eq!(schema["required"], serde_json::json!(["task"]));
                            }
                        })
                        .await
                        .unwrap();
                    }
                    Ok::<_, HttpError>(Response::empty(StatusCode::OK))
                })
                .map(drop)
            }))
    })?;
    assert_eq!(app.get("/tools").await?.status(), StatusCode::OK);
    Ok(())
}
