use std::{any::type_name, borrow::Cow, sync::LazyLock};

use openrust::{
    http::HttpFuture,
    json_schema::{JsonSchemaTypeFactory, types::ObjectType},
};
use serde_json::Value;

use crate::{Agent, AiError, AiResult, HasStructuredOutput, Tool, ToolRequest, localization};

/// 为子 Agent 声明工具名称和用途。
pub trait CanActAsTool: Agent + Sized {
    fn name(&self) -> &str;

    fn description(&self) -> &str;

    fn into_tool(self) -> AgentTool<Self> {
        let name = self.name().to_owned();
        let description = self.description().to_owned();
        AgentTool {
            agent: self,
            name,
            description: ToolDescription::Custom(description),
            prompt: prompt_text::<Self>,
        }
    }
}

type PromptAgent<A> = for<'a> fn(&'a A, &'a str) -> HttpFuture<'a, AiResult<String>>;

static TOOL_MESSAGES: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../lang/zh-Hans/tools.json"))
        .expect("内置 AI 工具词条必须为有效 JSON")
});

fn default_message(key: &str) -> &'static str {
    TOOL_MESSAGES["agent"][key]
        .as_str()
        .expect("内置子 Agent 工具词条必须为字符串")
}

enum ToolDescription {
    Default(String),
    Custom(String),
}

/// 将显式委托的任务交给独立 Agent，工具结果为子 Agent 的回答。
pub struct AgentTool<A> {
    agent: A,
    name: String,
    description: ToolDescription,
    prompt: PromptAgent<A>,
}

impl<A: Agent> AgentTool<A> {
    pub fn new(agent: A) -> Self {
        let kind = type_name::<A>().split('<').next().unwrap_or_default();
        let name = kind.rsplit("::").next().unwrap_or("agent").to_owned();
        let fallback = default_message("description").replace(":agent", &name);
        let description = agent_description(&name, &fallback);
        Self {
            agent,
            name,
            description: ToolDescription::Default(description),
            prompt: prompt_text::<A>,
        }
    }

    pub fn agent(&self) -> &A {
        &self.agent
    }
}

impl<A: HasStructuredOutput> AgentTool<A> {
    pub fn with_structured_output(mut self) -> Self {
        self.prompt = prompt_structured::<A>;
        self
    }
}

impl<A: Agent> Tool for AgentTool<A> {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        match &self.description {
            ToolDescription::Default(text) | ToolDescription::Custom(text) => text,
        }
    }

    fn localized_description(&self) -> Cow<'_, str> {
        match &self.description {
            ToolDescription::Default(fallback) => {
                Cow::Owned(agent_description(&self.name, fallback))
            }
            ToolDescription::Custom(text) => Cow::Borrowed(text),
        }
    }

    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType {
        schema.object([(
            "task",
            schema
                .string()
                .description(localization::text(
                    "ai::tools.agent.task",
                    &Default::default(),
                    default_message("task"),
                ))
                .required(true)
                .into(),
        )])
    }

    fn handle<'a>(&'a self, request: ToolRequest<'a>) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async move {
            let task = request
                .get("task")
                .and_then(Value::as_str)
                .filter(|task| !task.trim().is_empty())
                .ok_or(AiError::InvalidToolArguments)?;
            (self.prompt)(&self.agent, task).await
        })
    }
}

fn agent_description(name: &str, fallback: &str) -> String {
    localization::text(
        "ai::tools.agent.description",
        &[("agent".into(), name.to_owned())].into(),
        fallback,
    )
}

fn prompt_text<'a, A: Agent>(agent: &'a A, task: &'a str) -> HttpFuture<'a, AiResult<String>> {
    Box::pin(async move { Ok(agent.prompt(task).await?.text) })
}

fn prompt_structured<'a, A: HasStructuredOutput>(
    agent: &'a A,
    task: &'a str,
) -> HttpFuture<'a, AiResult<String>> {
    Box::pin(async move { Ok(agent.prompt_structured(task).await?.response.text) })
}
