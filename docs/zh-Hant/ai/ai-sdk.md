# AI SDK

## 支援狀態 {#support-status}

選用的 `ai` feature 提供 Laravel 風格的 `Ai`、`AiManager`、`Agent` 與文字 Fake。目前支援 OpenAI Responses 和 OpenAI 相容 Chat Completions 的一般文字與文字 SSE。

SDK 和閘道共用框架的 [HTTP 用戶端](../digging-deeper/http-client.md)。

## 可用入口 {#available-entry-points}

在應用程式 `Cargo.toml` 的 `[features].default` 中加入 `ai` 後，套件探索會註冊 SDK。選擇支援的連線，填寫對應的憑證與模型 ID。例如，使用相容驅動程式連線至 DeepSeek：

```dotenv
AI_PROVIDER=deepseek
DEEPSEEK_API_KEY=
DEEPSEEK_MODEL=deepseek-flash
```

將 API Key 填入應用程式環境。官方基礎位址可透過 `DEEPSEEK_URL` 覆寫。也可在 `config/ai.rs` 裡自訂 `ai.providers` 的具名連線。模型 ID 可寫入連線的 `models.text.default`，或於呼叫時明確指定。

```rust
use openrust_ai::{Agent, agent};

let response = agent("用繁體中文簡要回答。").prompt("如何申請退款？").await?;
let answer = response.text;
```

在啟用 SDK 的應用程式發行版中，可執行 `./artisan vendor:publish --tag=ai-config` 發布設定範本。在應用程式測試中，使用 `Ai::fake::<YourAgent>(responses)` 安裝應用程式層級的替身，可檢查提示文字與呼叫次數。

執行 `./artisan ai:models deepseek` 查看模型，或加上 `--json` 區分上游探索與本地設定。手動填寫的模型放在連線的 `models.text.ids`，清單會依原始 ID 合併並保留手填項目。用 `./artisan ai:prompt "請簡短回答" --provider=deepseek --model=deepseek-flash` 試呼叫所選模型。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 回傳 SDK 的預設 SSE 事件。也可透過 `into_stream()` 消費事件。

## 範圍 {#scope}

目前回應提供文字、來源連線、模型，以及上游實際回報的輸入與輸出 Token 數。Anthropic 原生連線也支援一般文字、SSE 與分頁模型目錄。Gemini 原生連線仍待實作。Vercel／AG-UI、SDK 工具、會話和 Embeddings 仍待實作。

## 模型閘道 {#model-gateway}

在應用程式 `Cargo.toml` 的 `[features].default` 中加入 `ai-gateway`，並設定 `AI_GATEWAY_ENABLED=true`。執行 `./artisan vendor:publish --tag=ai-gateway-config`，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填寫發佈關係及下游 Key 的權限，然後重新建置應用程式：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在應用程式環境中為 `AI_GATEWAY_SUPPORT_KEY` 設定獨立的隨機金鑰，`DEEPSEEK_API_KEY` 仍由上游連線單獨使用。外部用戶端的基礎 URL 設為 `https://你的應用程式網域/v1`，使用下游 Key 作為 Bearer 憑證，模型填寫 `support-chat`。`GET /v1/models` 只顯示目前 Key 有權呼叫的已發佈模型。選用的 `requests_per_minute` 由應用程式的速率限制快取執行。

`POST /v1/chat/completions` 支援 OpenAI 和相容連線。`POST /v1/responses` 可原生轉發至 OpenAI 連線，也可將純文字請求轉換為 Anthropic Messages。兩個入口皆支援 JSON 和 SSE。Responses 目前採用無狀態模式，將 `store` 設為 `false`，並拒絕上游會話、舊 Response ID 和伺服器端工具。閘道使用結構化日誌 `ai_gateway.call` 記錄呼叫結果和上游回報的 Token 數。可用 `usage_channel` 指定日誌通道，通道必須允許 `info` 層級。未回報的計數記為 `null`。

使用 Anthropic 用戶端時，基礎 URL 指向應用程式網域，下游 Key 透過 `x-api-key` 提交。閘道支援 `/v1/messages` 和 `/v1/messages/count_tokens`，模型須對應到 Anthropic 原生連線。附帶 `x-api-key` 的 `/v1/models` 和 `/v1/models/{model}` 使用 Anthropic 格式，Bearer 模型目錄使用 OpenAI 格式。

同一個 Anthropic 發佈模型可以由 Messages 和 Responses 用戶端呼叫。轉換支援文字、系統指示、SSE 和基本取樣參數。需要結構化輸出、工具或附件時，依用戶端與模型支援情況選擇原生端點。
