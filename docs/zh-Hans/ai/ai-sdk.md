# AI SDK

## 支持状态 {#support-status}

可选 `ai` feature 提供 Laravel 风格的 `Ai`、`AiManager`、`Agent` 和文本 Fake。当前支持 OpenAI Responses 和 OpenAI 兼容 Chat Completions 的普通文本与文本 SSE。

## 可用入口 {#available-entry-points}

启用 `ai` feature 后，包发现会注册 SDK。选择一个支持的连接，填写对应的凭证和模型 ID。例如，使用兼容驱动连接 DeepSeek：

```dotenv
AI_PROVIDER=deepseek
DEEPSEEK_API_KEY=
DEEPSEEK_MODEL=deepseek-flash
```

API Key 填入应用环境。官方基础地址可通过 `DEEPSEEK_URL` 覆盖。也可在 `config/ai.rs` 自定义 `ai.providers` 的命名连接。模型 ID 可写入连接的 `models.text.default`，或在调用时显式指定。

```rust
use openrust_ai::{Agent, agent};

let response = agent("用简体中文简要回答。").prompt("如何申请退款？").await?;
let answer = response.text;
```

在启用 SDK 的应用发行版中，可运行 `artisan vendor:publish --tag=ai-config` 发布配置模板。在应用测试中，使用 `Ai::fake::<YourAgent>(responses)` 安装应用级替身，可校验提示内容和调用次数。

运行 `artisan ai:models deepseek` 查看模型，或加 `--json` 区分上游发现和本地配置。手填模型放在连接的 `models.text.ids`，列表会按原始 ID 合并并保留手填项。用 `artisan ai:prompt "请简单回答" --provider=deepseek --model=deepseek-flash` 试调用所选模型。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 返回 SDK 的默认 SSE 事件。也可以通过 `into_stream()` 消费事件。

## 范围 {#scope}

当前响应提供文本、来源连接、模型以及上游实际报告的输入和输出 Token 数。已包含预设地址的供应商中，使用 Anthropic 和 Gemini 原生协议的连接待后续支持。Vercel／AG-UI、SDK 工具、会话和 Embeddings 仍待实现。

## 模型网关 {#model-gateway}

启用 `ai-gateway` feature 和 `AI_GATEWAY_ENABLED=true`。运行 `artisan vendor:publish --tag=ai-gateway-config` 后，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填写发布关系及下游 Key 的权限，再重新构建应用：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在应用环境中为 `AI_GATEWAY_SUPPORT_KEY` 设置独立的随机密钥，`DEEPSEEK_API_KEY` 仍由上游连接单独使用。外部客户端将基础 URL 设为 `https://你的应用域名/v1`，用下游 Key 作为 Bearer 凭证，模型填写 `support-chat`。`GET /v1/models` 只显示当前 Key 有权调用的已发布模型。可选的 `requests_per_minute` 通过应用的限流缓存执行。

`POST /v1/chat/completions` 支持 OpenAI 与兼容连接。`POST /v1/responses` 目前要求 OpenAI 协议连接。两个入口均支持 JSON 和 SSE。Responses 当前采用无状态模式，令 `store` 为 `false`，并拒绝上游会话、旧 Response ID 和服务端工具。网关以结构化日志 `ai_gateway.call` 记录调用结果和上游报告的 Token 数。可用 `usage_channel` 指定日志通道，通道须允许 `info` 级别。未报告的计数记为 `null`。
