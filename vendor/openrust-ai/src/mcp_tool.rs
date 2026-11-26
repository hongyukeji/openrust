use std::future::Future;

use openrust::{
    http::HttpFuture,
    json_schema::{JsonSchema, JsonSchemaTypeFactory, types::ObjectType},
};
use rmcp::{
    Peer,
    model::{CallToolRequestParams, CallToolResult, ToolAnnotations},
    service::RoleClient,
};
use serde_json::Value;

use crate::{AiError, AiResult, Approval, Tool, ToolRequest, localization};

type ApprovalPolicy = Box<dyn Fn(&ToolRequest<'_>) -> Option<Approval> + Send + Sync>;
type Call = Box<
    dyn Fn(CallToolRequestParams) -> HttpFuture<'static, AiResult<CallToolResult>> + Send + Sync,
>;

/// 将应用已连接的 MCP 客户端工具注册为 Agent 工具。
pub struct McpTool {
    call: Call,
    definition: rmcp::model::Tool,
    name: String,
    schema: ObjectType,
    approval: Option<ApprovalPolicy>,
}

impl McpTool {
    pub fn new(client: Peer<RoleClient>, definition: rmcp::model::Tool) -> AiResult<Self> {
        let name = format!("mcp_tools_{}", definition.name);
        Self::from_definition(name, definition, move |params| {
            let client = client.clone();
            async move {
                client
                    .call_tool(params)
                    .await
                    .map_err(|_| AiError::McpToolCallFailed)
            }
        })
    }

    fn from_definition<F, Fut>(
        name: String,
        definition: rmcp::model::Tool,
        call: F,
    ) -> AiResult<Self>
    where
        F: Fn(CallToolRequestParams) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AiResult<CallToolResult>> + Send + 'static,
    {
        let schema = JsonSchema::from_array(Value::Object((*definition.input_schema).clone()))
            .and_then(ObjectType::try_from)
            .map_err(|_| AiError::InvalidSchema)?;
        Ok(Self {
            call: Box::new(move |request| Box::pin(call(request))),
            definition,
            name,
            schema,
            approval: None,
        })
    }

    pub fn annotations(&self) -> Option<&ToolAnnotations> {
        self.definition.annotations.as_ref()
    }

    pub fn with_approval(
        mut self,
        policy: impl Fn(&ToolRequest<'_>) -> Option<Approval> + Send + Sync + 'static,
    ) -> Self {
        self.approval = Some(Box::new(policy));
        self
    }
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        self.definition
            .description
            .as_deref()
            .or(self.definition.title.as_deref())
            .unwrap_or(&self.definition.name)
    }

    fn schema(&self, _: JsonSchemaTypeFactory) -> ObjectType {
        self.schema.clone()
    }

    fn should_request_approval(&self, request: &ToolRequest<'_>) -> Option<Approval> {
        self.approval.as_ref().and_then(|policy| policy(request))
    }

    fn handle<'a>(&'a self, request: ToolRequest<'a>) -> HttpFuture<'a, AiResult<String>> {
        Box::pin(async move {
            let params = CallToolRequestParams::new(self.definition.name.clone())
                .with_arguments(request.arguments().clone());
            let result = (self.call)(params).await?;
            Ok(normalize_result(result))
        })
    }
}

/// 直接调用应用声明的 MCP 服务工具处理函数。
pub struct McpServerTool(McpTool);

impl McpServerTool {
    pub fn new<F, Fut>(definition: rmcp::model::Tool, call: F) -> AiResult<Self>
    where
        F: Fn(CallToolRequestParams) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = AiResult<CallToolResult>> + Send + 'static,
    {
        McpTool::from_definition(definition.name.to_string(), definition, call).map(Self)
    }

    pub fn annotations(&self) -> Option<&ToolAnnotations> {
        self.0.annotations()
    }

    pub fn with_approval(
        self,
        policy: impl Fn(&ToolRequest<'_>) -> Option<Approval> + Send + Sync + 'static,
    ) -> Self {
        Self(self.0.with_approval(policy))
    }
}

impl Tool for McpServerTool {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn schema(&self, schema: JsonSchemaTypeFactory) -> ObjectType {
        self.0.schema(schema)
    }

    fn should_request_approval(&self, request: &ToolRequest<'_>) -> Option<Approval> {
        self.0.should_request_approval(request)
    }

    fn handle<'a>(&'a self, request: ToolRequest<'a>) -> HttpFuture<'a, AiResult<String>> {
        self.0.handle(request)
    }
}

fn normalize_result(result: CallToolResult) -> String {
    let structured = result
        .structured_content
        .as_ref()
        .filter(|value| !value.is_null());
    let text = || {
        result
            .content
            .iter()
            .filter_map(|part| part.as_text().map(|part| part.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if result.is_error != Some(true) {
        return structured.map_or_else(text, Value::to_string);
    }
    let mut detail = text();
    if detail.is_empty() {
        if let Some(value) = structured {
            detail = value.to_string();
        } else {
            return localization::text(
                "ai::tools.mcp.error",
                &Default::default(),
                "MCP 工具执行失败。",
            );
        }
    }
    let fallback = format!("MCP 工具执行失败：{detail}");
    localization::text(
        "ai::tools.mcp.error_with_detail",
        &[("detail".into(), detail)].into(),
        &fallback,
    )
}
