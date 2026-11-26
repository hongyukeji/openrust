# AI SDK

## 支持状态 {#support-status}

可选 `ai` feature 提供 Laravel 风格的 `Ai`、`AiManager`、`Agent` 和文本 Fake。当前支持 OpenAI Responses 和 OpenAI 兼容 Chat Completions 的非流式文本调用。

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

## 范围 {#scope}

当前响应提供文本、来源连接、模型以及上游实际报告的输入和输出 Token 数。已包含预设地址的供应商中，使用 Anthropic 和 Gemini 原生协议的连接待后续支持。流式输出、工具、会话、Embeddings 和对外兼容网关也待实现。
