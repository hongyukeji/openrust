# AI SDK

## 支援狀態 {#support-status}

選用的 `ai` feature 提供 Laravel 風格的 `Ai`、`AiManager`、`Agent` 與文字 Fake。目前支援 OpenAI Responses 和 OpenAI 相容 Chat Completions 的一般文字與文字 SSE。

## 可用入口 {#available-entry-points}

啟用 `ai` feature 後，套件探索會註冊 SDK。選擇支援的連線，填寫對應的憑證與模型 ID。例如，使用相容驅動程式連線至 DeepSeek：

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

在啟用 SDK 的應用程式發行版中，可執行 `artisan vendor:publish --tag=ai-config` 發布設定範本。在應用程式測試中，使用 `Ai::fake::<YourAgent>(responses)` 安裝應用程式層級的替身，可檢查提示文字與呼叫次數。

執行 `artisan ai:models deepseek` 查看模型，或加上 `--json` 區分上游探索與本地設定。手動填寫的模型放在連線的 `models.text.ids`，清單會依原始 ID 合併並保留手填項目。用 `artisan ai:prompt "請簡短回答" --provider=deepseek --model=deepseek-flash` 試呼叫所選模型。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 回傳 SDK 的預設 SSE 事件。也可透過 `into_stream()` 消費事件。

## 範圍 {#scope}

目前回應提供文字、來源連線、模型，以及上游實際回報的輸入與輸出 Token 數。已提供預設位址的供應商中，使用 Anthropic 和 Gemini 原生協定的連線仍待支援。Vercel／AG-UI、工具、會話、Embeddings 和對外相容閘道仍待實作。
