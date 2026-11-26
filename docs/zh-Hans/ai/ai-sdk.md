# AI SDK

## 支持状态 {#support-status}

可选 `ai` feature 提供 Laravel 风格的 `Ai`、`AiManager`、`Agent` 和文本 Fake。当前支持 OpenAI Responses 和 OpenAI 兼容 Chat Completions 的普通文本与文本 SSE。

SDK 和网关复用框架的 [HTTP 客户端](../digging-deeper/http-client.md)。

## 可用入口 {#available-entry-points}

在应用 `Cargo.toml` 的 `[features].default` 中加入 `ai` 后，包发现会注册 SDK。选择一个支持的连接，填写对应的凭证和模型 ID。例如，使用兼容驱动连接 DeepSeek：

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

在启用 SDK 的应用发行版中，可运行 `./artisan vendor:publish --tag=ai-config` 发布配置模板。在应用测试中，使用 `Ai::fake::<YourAgent>(responses)` 安装应用级替身，可校验提示内容和调用次数。

运行 `./artisan ai:models deepseek` 查看模型，或加 `--json` 区分上游发现和本地配置。手填模型放在连接的 `models.text.ids`，列表会按原始 ID 合并并保留手填项。用 `./artisan ai:prompt "请简单回答" --provider=deepseek --model=deepseek-flash` 试调用所选模型。

运行 `./artisan ai:select-model --save --default` 选择模型，并将该连接设为应用默认。Artisan 使用当前默认连接，或在只有一家配置了凭证时直接提示该连接的模型。也可指定连接名。手填模型用 `--model=your-model-id`，自动化场景可加 `--no-interaction`。保存写入应用实际加载的环境文件。使用配置缓存时，保存后重新生成缓存。

通过 `agent(instructions).with_messages(history)` 带上历史对话。按顺序使用 `Message::user(text)` 和 `Message::assistant(text)` 构建历史消息，`prompt` 和 `stream` 会先发送历史，再发送本次提示词。Artisan 的 `--messages` 接受由 `role` 和 `content` 组成的 JSON 消息数组。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 返回 SDK 的默认 SSE 事件。也可以通过 `into_stream()` 消费事件。

使用 `agent(instructions).with_schema(schema).prompt(text)` 请求 JSON 对象。Schema 用框架已有的 `JsonSchema` 构建。可读取 `response.structured`，或通过 `response.decode::<YourType>()` 转为 Rust 类型。所选模型须支持连接的原生结构化输出格式。结构化流将 JSON 作为文本增量发出。可用 `./artisan ai:prompt` 的 `--schema` 传入 JSON 对象 Schema 试调用。

## 范围 {#scope}

当前响应提供文本、来源连接、模型以及上游实际报告的输入和输出 Token 数。Anthropic 与 Gemini 原生连接也支持普通文本、SSE 和分页模型目录。Vercel／AG-UI、SDK 工具和会话仍待实现。

## 模型网关 {#model-gateway}

在应用 `Cargo.toml` 的 `[features].default` 中加入 `ai-gateway`，并设置 `AI_GATEWAY_ENABLED=true`。运行 `./artisan vendor:publish --tag=ai-gateway-config` 后，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填写发布关系及下游 Key 的权限，再重新构建应用：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在应用环境中为 `AI_GATEWAY_SUPPORT_KEY` 设置独立的随机密钥，`DEEPSEEK_API_KEY` 仍由上游连接单独使用。外部客户端将基础 URL 设为 `https://你的应用域名/v1`，用下游 Key 作为 Bearer 凭证，模型填写 `support-chat`。`GET /v1/models` 只显示当前 Key 有权调用的已发布模型。可选的 `requests_per_minute` 通过应用的限流缓存执行。

`POST /v1/chat/completions` 支持 OpenAI 与兼容连接。`POST /v1/responses` 可原生转发至 OpenAI 连接，也可将纯文本请求转换为 Anthropic Messages。两个入口均支持 JSON 和 SSE。Responses 当前采用无状态模式，令 `store` 为 `false`，并拒绝上游会话、旧 Response ID 和服务端工具。网关以结构化日志 `ai_gateway.call` 记录调用结果和上游报告的 Token 数。可用 `usage_channel` 指定日志通道，通道须允许 `info` 级别。未报告的计数记为 `null`。

使用 Anthropic 客户端时，基础 URL 指向应用域名，下游 Key 通过 `x-api-key` 提交。网关支持 `/v1/messages` 和 `/v1/messages/count_tokens`，模型可映射到 Anthropic 原生连接。纯文本 Messages 请求也可以转发到已发布的 OpenAI Responses 模型。带 `x-api-key` 的 `/v1/models` 和 `/v1/models/{model}` 使用 Anthropic 格式，Bearer 模型目录使用 OpenAI 格式。

两个方向的转换都支持普通 JSON 和 SSE，给客户端返回公开模型名。上游报告缓存读写用量时，网关会按目标协议返回相应计数。

同一个 Anthropic 发布模型同时可由 Messages 和 Responses 客户端调用。转换支持文本、系统指令、SSE 和基本采样参数。需要结构化输出、工具或附件时，按客户端与模型支持情况选择原生端点。

Gemini 客户端使用 `/v1beta` API，通过 `x-goog-api-key` 提交下游 Key。网关列出授权的 Gemini 模型，并支持 `/v1beta/models/{model}` 下的 `:generateContent`、`:streamGenerateContent?alt=sse` 和 `:countTokens`。在 SDK 或 Artisan 中选择 `gemini` 连接，模型 ID 可带 `models/` 前缀。

## Embeddings {#embeddings}

设置 `AI_EMBEDDINGS_PROVIDER=openai`，并将供应商的模型 ID 填入 `OPENAI_EMBEDDING_MODEL`。SDK 批量发送文本，按输入顺序返回向量。

```rust
use openrust_ai::Embeddings;

let response = Embeddings::r#for(["配送", "退货"])?
    .generate()
    .await?;
let vectors = response.embeddings;
```

所选模型支持指定维度时，可使用 `dimensions(n)`。使用 OpenAI 兼容连接生成向量前，先配置该连接的 `models.embeddings`。通过 `POST /v1/embeddings` 发布向量模型时，映射公开名并为该模型配置 `"endpoints": ["embeddings"]`，再将公开名授权给下游 Key。
