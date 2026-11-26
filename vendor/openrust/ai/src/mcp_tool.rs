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

/// 将应用已连接的 MCP 客户端工具注册为 Agent 工具。
pub struct McpTool {
    client: Peer<RoleClient>,
    definition: rmcp::model::Tool,
    name: String,
    schema: ObjectType,
    approval: Option<ApprovalPolicy>,
}

impl McpTool {
    pub fn new(client: Peer<RoleClient>, definition: rmcp::model::Tool) -> AiResult<Self> {
        let name = format!("mcp_tools_{}", definition.name);
        let schema = JsonSchema::from_array(Value::Object((*definition.input_schema).clone()))
            .and_then(ObjectType::try_from)
            .map_err(|_| AiError::InvalidSchema)?;
        Ok(Self {
            client,
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
            let result = self
                .client
                .call_tool(params)
                .await
                .map_err(|_| AiError::McpToolCallFailed)?;
            Ok(normalize_result(result))
        })
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
