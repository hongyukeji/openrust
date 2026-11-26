# AI SDK

## 支援狀態 {#support-status}

選用的 `ai` feature 提供 Laravel 風格的 `Ai`、`AiManager`、`Agent` 與文字 Fake。一般文字與串流呼叫支援 OpenAI Responses、OpenAI 相容 Chat Completions、Anthropic Messages 和 Gemini GenerateContent。Agent 可以將文字對話儲存在應用程式資料庫。

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

執行 `./artisan ai:select-model --save --default` 選擇模型，並將該連線設為應用程式預設。Artisan 使用目前的預設連線，或在只有一家設定了憑證時直接提示該連線的模型。也可以指定連線名稱。手填模型使用 `--model=your-model-id`，自動化情境可加上 `--no-interaction`。儲存會寫入應用程式實際載入的環境檔。使用設定快取時，儲存後重新產生快取。

透過 `agent(instructions).with_messages(history)` 帶入歷史對話。依序使用 `Message::user(text)` 和 `Message::assistant(text)` 建立歷史訊息，`prompt` 和 `stream` 會先傳送歷史，再傳送本次提示詞。Artisan 的 `--messages` 接受由 `role` 和 `content` 組成的 JSON 訊息陣列。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 回傳 SDK 的預設 SSE 事件。也可透過 `into_stream()` 消費事件。

在 `to_response()` 之前呼叫 `using_vercel_data_protocol(message_id)` 可回傳 Vercel AI SDK UI 文字事件，呼叫 `using_agent_user_interaction_protocol(thread_id, run_id)` 可回傳 AG-UI 文字事件。傳入 `None` 時由 SDK 提供 ID。兩種格式都會附上結束狀態和上游回報的 Token 用量。應用程式可使用 `using_protocol` 自訂同一批 Agent 事件的輸出格式。

使用 `agent(instructions).with_schema(schema).prompt(text)` 請求 JSON 物件。Schema 使用框架既有的 `JsonSchema` 建立。可讀取 `response.structured`，或透過 `response.decode::<YourType>()` 轉成 Rust 型別。所選模型必須支援連線的原生結構化輸出格式。結構化串流會將 JSON 作為文字增量傳送。可透過 `./artisan ai:prompt` 的 `--schema` 傳入 JSON 物件 Schema 進行試呼叫。

目前的請求或命令需要暫時連線時，可使用 `Ai::build(config)`，欄位沿用具名連線設定。透過回傳的 `name()` 在提示詞呼叫中指定連線，或在同一作用域內透過 `Ai::text_provider` 查找。框架保留名稱及應用程式既有名稱無法用作暫時連線。`Ai::flush_state()` 會清除目前作用域的暫時連線。

## Azure OpenAI {#azure-openai}

使用 `azure` 預設時，將 `AZURE_OPENAI_URL` 設為 Azure 資源位址，再填寫 `AZURE_OPENAI_API_KEY`。預設會視需要附加 `/openai/v1`。文字呼叫使用 `AZURE_OPENAI_DEPLOYMENT` 指定部署名稱。新增其他具名連線時，使用 `driver: "openai"`、完整的 v1 URL 和 `auth: "api-key"`。

## 範圍 {#scope}

目前回應提供文字、來源連線、模型，以及上游實際回報的輸入與輸出 Token 數。Anthropic 與 Gemini 原生連線也支援一般文字、SSE 和分頁模型目錄。

## 模型閘道 {#model-gateway}

在應用程式 `Cargo.toml` 的 `[features].default` 中加入 `ai-gateway`，並設定 `AI_GATEWAY_ENABLED=true`。執行 `./artisan vendor:publish --tag=ai-gateway-config`，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填寫發佈關係及下游 Key 的權限，然後重新建置應用程式：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在應用程式環境中為 `AI_GATEWAY_SUPPORT_KEY` 設定獨立的隨機金鑰，`DEEPSEEK_API_KEY` 仍由上游連線單獨使用。外部用戶端的基礎 URL 設為 `https://你的應用程式網域/v1`，使用下游 Key 作為 Bearer 憑證，模型填寫 `support-chat`。`GET /v1/models` 只顯示目前 Key 有權呼叫的已發佈模型。選用的 `requests_per_minute` 由應用程式的速率限制快取執行。

`POST /v1/chat/completions` 支援 OpenAI 和相容連線。`POST /v1/responses` 可原生轉發至 OpenAI 連線，也可將請求轉換為 Anthropic Messages。兩個入口皆支援 JSON 和 SSE。Responses 也可路由至只提供 Chat Completions 的連線。這條轉換支援無狀態文字、系統指令、基本生成參數、JSON 物件模式和用戶端函式工具。工具呼叫 ID 原樣回傳，參數增量會串流傳送。附件與 JSON Schema 需要支援對應功能的連線或轉換。Responses 目前採用無狀態模式，將 `store` 設為 `false`。上游會話和舊 Response ID 目前無法使用。伺服器端網頁搜尋需要在公開模型上明確啟用。閘道使用結構化日誌 `ai_gateway.call` 記錄呼叫結果和上游回報的 Token 數。可用 `usage_channel` 指定日誌通道，通道必須允許 `info` 層級。未回報的計數記為 `null`。

使用 Anthropic 用戶端時，基礎 URL 指向應用程式網域，下游 Key 透過 `x-api-key` 提交。閘道支援 `/v1/messages` 和 `/v1/messages/count_tokens`，模型可對應到 Anthropic 原生連線。Messages 請求也可路由至已發佈的 OpenAI Responses 或 Chat Completions 模型。閘道保留用戶端工具呼叫 ID，並依序傳送每個工具的內容區塊。附帶 `x-api-key` 的 `/v1/models` 和 `/v1/models/{model}` 使用 Anthropic 格式，Bearer 模型目錄使用 OpenAI 格式。

透過 `POST /v1/responses/input_tokens` 統計已發佈的 OpenAI 或 Anthropic 模型輸入。傳送包含模型與無狀態輸入的 JSON，閘道呼叫上游計數並回傳 `input_tokens`。Messages 計數入口也可使用 OpenAI Responses 連線。計數請求納入下游 Key 的請求次數，計數結果不計入生成用量。

用戶端需要不含 `/v1` 的路徑時，在 `config/ai_gateway.rs` 將 `unversioned_paths` 設為 `true`。這會新增 `/models`、`/models/{model}`、`/chat/completions`、`/responses`、`/responses/input_tokens`、`/embeddings` 和 `/messages/count_tokens`，並共用原有授權與限流。

兩個方向的轉換都支援一般 JSON 和 SSE，並向用戶端回傳公開模型名稱。上游回報快取讀寫用量時，閘道會依目標協定回傳相應的計數。

同一個 Anthropic 發佈模型可供 Messages、Responses 和 Chat Completions 用戶端呼叫。轉換支援文字、系統指示、基本取樣參數和用戶端函式工具，支援 JSON 和 SSE。工具呼叫 ID、串流參數增量與文字結果可雙向傳遞。結構化輸出和附件使用對應的原生端點。

Gemini 用戶端使用 `/v1beta` API，透過 `x-goog-api-key` 提交下游 Key。閘道列出已授權的 Gemini 模型，並支援 `/v1beta/models/{model}` 下的 `:generateContent`、`:streamGenerateContent?alt=sse` 和 `:countTokens`。在 SDK 或 Artisan 中選擇 `gemini` 連線，模型 ID 可以帶 `models/` 前綴。

公開模型對應可設定 `fallbacks`，依序嘗試備用連線。每項填寫 `provider` 和 `model`，例如 `[{"provider":"openai","model":"backup-model"}]`。無狀態請求遇到上游 429、5xx 等可重試錯誤時，閘道會嘗試備用連線。用戶端仍使用同一個公開名稱，每次嘗試使用同一個請求 ID 記錄。帶有工具、要求儲存輸出或已開始回傳的串流不會重送。

可在 `config/ai_gateway.rs` 中設定 `accounts.deepseek.cooldown_seconds = 30`，讓具名上游連線在回傳 HTTP 429、503 或 529 後進入冷卻。上游有效的 `Retry-After` 優先，最長 24 小時。使用同一連線的所有公開模型共用冷卻狀態。可用連線都在冷卻時，閘道回傳 HTTP 503 及 `Retry-After`。多程序部署可將 `account_cache_store` 指向共用快取，例如 `redis`。

需要讓同一用戶端會話優先使用上一次成功的上游時，將 `account_session_ttl` 設為 1 至 86400 秒，並在每次請求傳送 `X-Session-ID`。會話依下游 Key 與公開模型隔離。首選連線仍須通過授權、冷卻與並行檢查。成功回應會更新有效期間。快取暫時無法使用時，閘道依設定順序選路。多副本可共用 `account_cache_store` 以維持會話黏性。

設定 `accounts.deepseek.max_concurrent_requests` 可限制該連線的並行請求數。使用同一快取的閘道程序共用槽位。串流請求會佔用槽位，直到結束或用戶端中斷連線。首選連線已滿時，閘道會在送出請求前檢查下一個可用連線。若所有可用連線的槽位都已滿，回傳 HTTP 503 和 `upstream_busy`。

多個 Key 共用權限時，可設定群組，並在 Key 中填寫 `group`。群組的 `models` 列出公開模型別名，選填的 `providers` 限制首選與備援時可以呼叫的上游連線。Key 也可再填寫 `models`，縮小群組授予的模型範圍。

```rust
"groups": {
    "support-team": {"models": ["support-chat"], "providers": ["deepseek", "openai"]},
},
"keys": {
    "support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "group": "support-team"},
},
```

將 `managed_keys.enabled` 設為 `true` 可啟用資料庫管理的 Key。選填的 `managed_keys.connection` 指定資料庫連線。先執行套件遷移，再為既有群組簽發 Key。

```shell
./artisan migrate
./artisan ai:gateway:key:create support-client support-team --expires-in=86400 --requests-per-minute=60
./artisan ai:gateway:key:list
./artisan ai:gateway:key:rotate KEY_ID
./artisan ai:gateway:key:revoke KEY_ID
```

簽發和輪替只回傳一次可用的 Bearer Token。資料庫儲存 SHA-256 摘要。輪替沿用 Key ID、群組、有效期限與請求額度。舊版、撤銷或過期的 Token 後續請求回傳 HTTP 401。需要自訂遷移時，執行 `./artisan vendor:publish --tag=ai-gateway-migrations`，重新編譯後執行遷移。

### Responses WebSocket {#responses-websocket}

使用 HTTP 端點相同的 Bearer Key 連線到 `wss://your-app.example/v1/responses`。啟用 `unversioned_paths` 時也可使用 `/responses`。傳送 `response.create` JSON 文字訊框，讀取 Responses 事件直到 `response.completed` 或 `response.incomplete`。

```json
{
  "type": "response.create",
  "model": "support-chat",
  "input": "How do I return an item?"
}
```

為平行通道指定不同的 `stream_id`。每條連線最多四個活動通道，每個通道同時處理一個回應。事件攜帶所屬通道 ID。每輪沿用 HTTP 的身分驗證、路由、限額和用量記錄。關閉連線會取消所有正在生成的回應。

延續文字或用戶端函式對話時，將最近一次已完成回應的 ID 放入 `previous_response_id`，並在 `input` 中只提交新訊息或函式結果。沿用同一通道可續接，換一個 `stream_id` 可從仍在記憶體中的回應分叉。省略 `model` 時沿用原別名。`instructions` 僅用於當前輪次。每條連線保留最多四份歷史，每份上限為 512 KiB、256 項。重新連線、更換模型或收到 `previous_response_not_found` 時，重新傳送完整上下文。

文字訊框上限為 2 MiB，連線滿一小時後須重新連線。

### Codex 模型目錄 {#codex-model-catalog}

使用 Bearer 憑證請求 `/v1/models?client_version=0.170.0` 可取得 Codex 模型目錄。Codex 也可將基礎 URL 設為 `https://your-app.example/backend-api/codex`。此前綴提供 `/models`、HTTP 與 WebSocket 的 `/responses`，以及 `/responses/input_tokens`。目錄僅列出目前 Key 能透過 Responses 呼叫的公開模型。一般 OpenAI 和 Anthropic 模型端點沿用各自格式。

在公開模型對應中可選填 `codex` 物件，用來設定顯示名稱、說明、上下文視窗、輸入類型和推理選項。預設使用公開模型名稱作為顯示名稱，並宣告文字輸入。用戶端可帶上 `If-None-Match` 重複使用已授權的目錄。

```json
{
  "models": {
    "support-chat": {
      "provider": "openai",
      "model": "upstream-model",
      "codex": {
        "display_name": "Support Assistant",
        "context_window": 40000,
        "input_modalities": ["text", "image"]
      }
    }
  }
}
```

在 Codex 中設定 Responses 連線，填入閘道基礎 URL（例如 `https://your-app.example/v1`）及下游 Key，並以公開別名作為模型。原生 Responses 上游支援用戶端工具命名空間。若要使用上游網頁搜尋，在公開模型設定 `"server_tools": ["web_search"]`。模型未啟用網頁搜尋時，在 Codex 用戶端設定使用 `web_search = "disabled"`。

### 閘道圖片生成與編輯 {#gateway-images}

在 `ai_gateway.models` 發布圖片模型時，明確列出 `images/generations` 與 `images/edits`。沿用 Key 或群組授權公開模型名稱。上游可用 OpenAI 連線，或在 `models.images` 設定圖片能力的 OpenAI 相容連線。

```json
{
  "catalog-image": {
    "provider": "openai",
    "model": "gpt-image-1",
    "endpoints": ["images/generations", "images/edits"]
  }
}
```

`POST /v1/images/generations` 接收 JSON，`POST /v1/images/edits` 接收 JSON 或 multipart。表單可傳單個 `image` 或有序 `image[]`，以及選用的 `mask`。JSON 參照可用連結或內嵌資料，共享閘道憑證下無法參照供應商檔案 ID。`stream: true` 透過 SSE 回傳局部及最終圖片。閘道記錄上游回報的用量，每個圖片呼叫只向上游傳送一次。設定了文字輸出 Token 配額的群組須透過另一個群組使用圖片模型。

### 接入 DeepSeek Harness {#dsh-model-connection}

在 DeepSeek Harness 中開啟 **設定 → 模型 → 新增模型供應商**，選擇 **自訂模型 API**。填入連線 ID、閘道下游 Key 以及對應協定的基礎 URL。

| DSH 協定 | 基礎 URL |
| --- | --- |
| OpenAI Chat Completions | `https://your-app.example/v1` |
| OpenAI Responses | `https://your-app.example/v1` |
| Anthropic Messages | `https://your-app.example` |
| Gemini API | `https://your-app.example/v1beta` |

使用 **取得可用模型** 選取模型，或透過 **新增模型** 手動輸入閘道發布的別名。儲存後，在對話輸入區選取該連線的模型。

### 持久化閘道用量 {#gateway-usage}

在 `config/ai_gateway.rs` 設定 `usage.enabled = true` 後執行遷移。多個執行個體共用用量資料庫時，可透過 `usage.connection` 指定連線。

```shell
./artisan migrate
./artisan ai:gateway:usage:list --limit=20
./artisan ai:gateway:usage:list --request-id=REQUEST_ID
```

每次上游嘗試各有一筆記錄。備援嘗試共用 `request_id`。記錄包含下游 Key 或群組、公開模型、連線、上游模型、回應狀態和供應商實際回報的 Token 數。缺少的用量保留為 `null`。帳本省略 Key 金鑰、請求本文和產生的文字。

閘道先登記嘗試，再向上游送出請求。正常結束、出錯和取消都會結算。處理程序在結算前結束時，帳本可能留下 `pending` 記錄供核對。

使用發起模型呼叫的 Key，以 Bearer 或 `x-api-key` 驗證身分並請求 `GET /v1/usage?days=7`。回應包含該 Key 當日、累計與依 UTC 日期彙整的用量。`days` 可設為 1 至 90。遞補只算一次請求，並按實際向上游送出的次數統計嘗試。缺少的 Token 數維持 `null`。

`model_stats` 傳回目前 Key 最近 30 天依公開模型名稱彙整的用量。可用 `start_date` 與 `end_date` 指定 `YYYY-MM-DD` 格式的 UTC 日期，包含開始及結束當天。省略或無法解析的日期使用對應的預設值，此範圍僅影響 `model_stats`。

託管 Key 輪替後保留歷史。替換環境 Key 後，新憑證開始累積自己的歷史。維運人員可透過 Artisan 帳本查詢先前記錄及上游明細。

### 群組輸出 Token 配額 {#gateway-output-quota}

啟用用量帳本並執行遷移後，可在群組設定輸出 Token 配額。同群組所有 Key 在各閘道執行個體共用每日額度，於 UTC 午夜重設。

```json
{
  "groups": {
    "support-team": {
      "models": ["support-chat"],
      "quota": {
        "output_tokens_per_day": 100000,
        "max_output_tokens": 4096
      }
    }
  }
}
```

閘道依 `max_output_tokens` 限制每次嘗試，送出請求前先預留額度。請求指定較小的輸出上限時，會減少預留。成功回應依供應商回報的輸出 Token 結算。失敗、取消或缺少最終用量時保留預留直到時段重設。每次備援都需要個別預留。

額度不足時傳回 HTTP 429 和 `Retry-After`。若供應商回報的輸出超過預留，該群組停止接收新的生成請求，直到下一個時段。配額支援 Chat Completions、Responses 和 Anthropic Messages 生成。Token 計數和 Embeddings 沿用請求次數限制。

## 持久對話 {#conversations}

讓 Agent 實作 `RemembersConversations` 即可把對話存到應用程式資料庫。先發佈遷移，重新編譯應用程式，再執行遷移。

```bash
cargo run --features ai -- artisan vendor:publish --tag=ai-migrations
cargo run --features ai -- artisan migrate
```

```rust
use openrust_ai::{ConversationParticipant, RemembersConversations};

impl RemembersConversations for SupportAgent {}

let participant = ConversationParticipant::new("customer", authenticated_id);
let first = agent.for_participant(participant.clone()).prompt("Hello").await?;
let next = agent.r#continue(first.conversation_id, participant).prompt("Continue").await?;
```

繼續對話時傳入已驗證參與者的類型和 ID。`for_user(&user)` 可直接為已驗證的使用者開啟對話。`continue_last_conversation(participant)` 查詢該參與者與同一 Agent 類型的上一段對話，沒有歷史時則開啟新對話。

SDK 將已完成的使用者訊息和助理訊息一起儲存。下一次提示依時間順序讀取最近 100 則訊息，可在 `config/ai.rs` 中調整 `conversations.max_messages`。資料庫預設沿用應用程式連線，也可用 `AI_CONVERSATIONS_CONNECTION` 指定具名連線。歷史頁面使用 `Ai::conversation_store()?.paginate_conversation_messages(id, &participant, 15, cursor)` 從最新訊息開始游標分頁。串流對話使用 `stream()`，將回傳的 `response` 交給 SSE 轉譯器。呼叫開始後即可讀取對話 ID。SDK 在傳送最終完成事件前儲存該輪訊息，串流中斷時不會新增對話訊息。宣告了回應結構的 Agent 可使用 `prompt_structured()` 或 `stream_structured()`。

## 文字摘要 {#text-summarization}

匯入擴充 trait 後，`Str` 與 `Stringable` 共用同一個摘要 Agent，預設要求不超過三句。`summarize_with` 可傳入句數，並用一般的 `PromptOptions` 指定連線、模型與逾時。

```rust
use openrust::support::Str;
use openrust_ai::{PromptOptions, Summarizes, SummarizesText};

let short = Str::summarize(article).await?;
let detailed = Str::of(article)
    .summarize_with(5, PromptOptions::default())
    .await?;
```

摘要使用所選連線的 `models.text.cheapest`。Agent 也可選擇 `TextModelTier::Smartest`，使用 `models.text.smartest`。OpenAI 預設可用 `OPENAI_MODEL_CHEAPEST` 與 `OPENAI_MODEL_SMARTEST` 設定，其他預設可在其模型變數上加相同後綴。未設定的檔位會改用連線的預設模型，明確指定的模型優先。

## 圖片附件 {#prompt-images}

```rust
use openrust_ai::{Agent, ImageAttachment, agent};

let photo = ImageAttachment::from_storage("photos/sample.png", None).await?;
let response = agent("Describe the scene.")
    .with_attachments([photo])
    .prompt("What can you see?")
    .await?;
```

上傳圖片用 `ImageAttachment::from_upload(&file)` 讀取，本機檔案用 `ImageAttachment::from_path(path).await`，位元組用 `ImageAttachment::from_bytes(bytes)`。具名 Agent 透過 `Agent::attachments` 提供附件，匿名 Agent 可用 `with_attachments`。

所選模型須支援圖片輸入。OpenAI Responses、Chat Completions、Anthropic Messages 與 Gemini 分別使用原生圖片格式。SDK 接收 PNG、JPEG 與 WebP，單次最多 16 張，原始圖片總量須小於 50 MB，同時受供應商限制。可只傳送圖片，也可附上文字。附件會跟隨本輪呼叫與工具續步，文字對話儲存只記錄文字。

### 文件附件 {#document-attachments}

使用 `DocumentAttachment` 讀取 PDF、UTF-8 文字文件或 Markdown。支援 `from_bytes(filename, bytes)`、`from_upload(&file)`、`from_path(path).await` 與 `from_storage(path, disk).await`。混合傳入圖片和文件時，用 `Attachment` 統一型別，保留傳入順序。

```rust
use openrust_ai::{Agent, Attachment, DocumentAttachment, ImageAttachment, agent};

let report = DocumentAttachment::from_storage("reports/quarterly.pdf", None).await?;
let chart = ImageAttachment::from_storage("reports/chart.png", None).await?;
let response = agent("Review the quarterly report.")
    .with_attachments([Attachment::from(report), Attachment::from(chart)])
    .prompt("Summarize the results.")
    .await?;
```

請選擇支援文件的模型。Responses、Anthropic Messages 與 Gemini 接收這三種格式。Chat Completions 僅接收 PDF。圖片與文件共用每次提示詞的附件數量及大小上限。

## Agent 工具 {#agent-tools}

實作 `Tool` 的名稱、描述、JSON Schema 和非同步 `handle`。用 `ToolRequest::validate` 執行框架驗證規則，或透過 `decode` 讀取 Rust 型別的參數。工具可以讀取上游呼叫 ID 和本次執行的獨立 ID。

```rust
let response = agent("回答前先查目錄。")
    .with_tools(vec![std::sync::Arc::new(LookupInventory)])
    .prompt("還有多少庫存？")
    .await?;
let answer = response.text;
```

工具呼叫支援 Chat Completions、Responses、Anthropic Messages 和 Gemini GenerateContent 的一般與串流請求。Claude 和 Gemini 會收到原始模型內容及工具結果，讓供應商核對簽章。SDK 即時送出文字，在下一次模型呼叫前送出完整的工具呼叫及結果。預設 SSE、Vercel UI 和 AG-UI 都提供對應的工具事件。Agent 用原呼叫 ID 把結果送回模型，並在回應中提供 `tool_calls` 和 `tool_results`。用 `with_max_steps(n)` 限制模型步驟數。最後一個步驟提出的工具呼叫會記錄為失敗結果，工具不會執行。

## Agent 排隊 {#queued-agents}

讓具名 Agent 派生 `Serialize` 和 `Deserialize`，實作 `QueueableAgent` 並提供穩定的 `QUEUE_JOB` 名稱。在應用程式服務提供者的 `boot` 方法裡，從應用程式取得 `QueueManager`，呼叫 `TicketAgent::register_queue(queue.as_ref())` 註冊。

```rust
use openrust_ai::Agent;

let delivery = agent.queue("Summarize the latest conversation")
    .on_connection("database")
    .on_queue("ai")
    .then("persist")
    .catch("record_failure")
    .dispatch()
    .await?;
```

用 `queued_then` 和 `queued_catch` 將回呼名稱解析為具型別的函式。成功回呼接收還原後的 Agent 和 `AgentResponse`，失敗回呼接收 Agent 和工作終止錯誤。重試和退避沿用框架佇列。工作可能重複投遞，因此回呼中的寫入需要具備冪等性。上游金鑰由 Worker 的具名連線設定提供。

### 廣播串流事件 {#broadcast-agents}

`broadcast_now` 立即發布各事件，`broadcast` 透過框架佇列投遞事件。兩者都傳回串流，消費串流或傳回 `to_response()` 後才會開始傳送。使用者資料應透過私人頻道發布，並由應用程式的頻道規則授權訂閱。

```rust
use openrust::broadcasting::PrivateChannel;
use openrust_ai::Agent;

let channel = PrivateChannel { name: format!("tickets.{ticket_id}") };
let stream = agent.broadcast_now("Summarize this ticket", [channel.into()]).await?;
let response = stream.to_response();
```

`broadcast_on_queue` 在 Worker 內產生並發布串流。派發前可透過 `invocation_id()` 取得呼叫識別碼，用於關聯用戶端事件。成功回呼接收彙整回應，終止失敗會傳送同一呼叫識別碼的 `stream_failed`。廣播工作預設只嘗試一次，因為訂閱者可能已收到部分事件。

Agent 可覆寫 `without_broadcasting` 排除部分廣播事件，同時在 HTTP 串流中保留這些事件。即時投遞失敗會寫入日誌。廣播服務本身的訊息大小限制仍有效。

## 產生 Agent 或工具 {#generators}

```shell
./artisan make:agent CatalogAgent
./artisan make:agent Products/StockAgent --structured
./artisan make:tool LookupInventory
./artisan make:agent-middleware CheckInventory
./artisan vendor:publish --tag=ai-stubs
```

Agent 和工具分別產生在 `app/ai/agents` 和 `app/ai/tools`。在父模組中宣告產生的 Rust 模組，再重新編譯。加 `--force` 可覆寫既有檔案。發布後可編輯 `stubs/` 下的檔案，自訂產生範本。

在 Agent 上透過 `Agent::middleware` 設定中介層，匿名 Agent 則使用 `with_middleware`。非串流產生的每一步都會收到 `PendingStep`，其中包含目前模型、步驟編號、已完成步驟和用量。呼叫 `next.run(step)` 繼續執行。用 `with_model`、`with_instructions` 和 `only_tools` 修改目前步驟。

## Embeddings {#embeddings}

設定 `AI_EMBEDDINGS_PROVIDER=openai`，並將供應商的模型 ID 填入 `OPENAI_EMBEDDING_MODEL`。SDK 批次傳送文字，並依輸入順序回傳向量。

```rust
use openrust_ai::Embeddings;

let response = Embeddings::r#for(["配送", "退貨"])?
    .generate()
    .await?;
let vectors = response.embeddings;
```

所選模型支援指定維度時，可使用 `dimensions(n)`。使用 OpenAI 相容連線產生向量前，先設定該連線的 `models.embeddings`。透過 `POST /v1/embeddings` 發佈向量模型時，對應公開名稱並為該模型設定 `"endpoints": ["embeddings"]`，再把公開名稱授權給下游 Key。

呼叫 `cache(Some(600), None)` 可將向量快取 600 秒。在 `ai.caching.embeddings` 設定預設有效期、快取存放位置，以及是否為所有 SDK 呼叫啟用快取。逐筆快取可跨批次重用向量，只將未命中的輸入送給供應商。第二個參數傳入 `Some(false)` 會依原順序快取整批。單次呼叫可使用 `cache(Some(0), None)` 略過快取。部分命中時，用量只記錄實際送往上游的輸入。

### 測試 Embeddings {#embeddings-fake}

用 `Embeddings::fake` 依預期呼叫順序提供向量批次，或用 `fake_with` 根據記錄的請求產生向量。空序列在指定維度時產生單位向量。啟用 `prevent_stray_embeddings(true)` 可要求每次呼叫都使用預設回應。

```rust
let fake = Embeddings::fake([vec![vec![0.5, 0.2]]])?;
fake.prevent_stray_embeddings(true);
let result = Embeddings::r#for(["配送"])?.dimensions(2).generate().await?;
Embeddings::assert_generated(|call| call.inputs == ["配送"]);
```

SDK 會記錄 Fake 呼叫。Fake 不聯絡模型供應商，也不讀寫設定的向量快取。

## 文件重新排序 {#reranking}

用 `AI_RERANKING_PROVIDER` 選擇預設連線，再填寫該連線的 API Key 和重新排序模型 ID。

| 連線 | Base URL | URL 覆寫 | API Key | Model ID |
| --- | --- | --- | --- | --- |
| `cohere` | `https://api.cohere.com/v2` | `COHERE_URL` | `COHERE_API_KEY` | `COHERE_RERANK_MODEL` |
| `jina` | `https://api.jina.ai/v1` | `JINA_URL` | `JINA_API_KEY` | `JINA_RERANK_MODEL` |
| `voyageai` | `https://api.voyageai.com/v1` | `VOYAGE_URL` | `VOYAGE_API_KEY` | `VOYAGE_RERANK_MODEL` |
| `openrouter` | `https://openrouter.ai/api/v1` | `OPENROUTER_URL` | `OPENROUTER_API_KEY` | `OPENROUTER_RERANK_MODEL` |

```rust
use openrust_ai::Reranking;

let result = Reranking::of(["配送", "換貨", "退貨"])?
    .limit(2)
    .rerank("如何退貨？")
    .await?;
```

結果保留供應商回傳的排序順序。各項的 `index` 都指向原始文件位置，可用來取得輸入集合中的原始紀錄。

匯入 `Reranks` 後，可呼叫 `collection.rerank(|item| item.title.clone(), query).await?`，依相關性回傳集合中的原始項目。需要覆寫回傳筆數、逾時、連線或模型時，使用 `rerank_with` 和 `RerankingOptions`。

用 `Reranking::fake([])?` 產生測試排名，呼叫不會連線到供應商。也可預設結果，或用 `Reranking::fake_with` 撰寫回呼。透過 `assert_reranked`、`assert_not_reranked` 和 `assert_nothing_reranked` 驗證呼叫。替身的 `prevent_stray_rerankings(true)` 會拒絕未設定回應的呼叫。

## 圖像生成 {#images}

選擇圖像連線及模型。OpenAI 沿用 `OPENAI_API_KEY`，可用 `OPENAI_URL` 覆寫服務位址。

```dotenv
AI_IMAGE_PROVIDER=openai
OPENAI_IMAGE_MODEL=gpt-image-1.5
```

```rust
use openrust_ai::Image;

let response = Image::of("A watercolor pear")?.square().quality("high").generate().await?;
let path = response.store_as("generated", "pear.png", None).await?;
```

透過 `square()`、`portrait()` 或 `landscape()` 選擇標準尺寸，也可用 `size()`、`quality()` 或 `timeout()` 設定請求參數。編輯圖像時，用 `ImageAttachment::from_bytes` 或 `ImageAttachment::from_storage` 建立參考圖，呼叫 `attachments()` 後再生成。

支援 Images API 的 OpenAI 及 OpenAI 相容連線可回傳內嵌的 PNG、JPEG 或 WebP。相容連線透過 `models.images` 啟用圖像功能。用 `first_image()` 讀取 MIME 類型與位元組，或用 `store` 與 `store_as` 寫入磁碟。需要公開可見時，呼叫 `store_publicly` 或 `store_publicly_as`。

## 語音生成與轉錄 {#audio}

為兩項能力分別選擇連線與模型。OpenAI 沿用 `OPENAI_API_KEY`，也可用 `OPENAI_URL` 覆寫服務位址。

```dotenv
AI_AUDIO_PROVIDER=openai
OPENAI_AUDIO_MODEL=gpt-4o-mini-tts
AI_TRANSCRIPTION_PROVIDER=openai
OPENAI_TRANSCRIPTION_MODEL=gpt-4o-transcribe
```

```rust
use openrust_ai::{Audio, AudioFormat, Transcription};

let speech = Audio::of("Read the notice")?.format(AudioFormat::Wav).generate().await?;
let path = speech.store_as("audio", "notice.wav", None).await?;
let transcript = Transcription::from_storage(&path, None).await?.language("en").generate().await?;
```

使用 `Audio::of(text)`，或匯入 `ToAudio` 在字串上呼叫 `to_audio()`。透過 `voice()`、`male()` 或 `female()` 選擇音色，支援 MP3、WAV、Opus、AAC、FLAC 和 PCM 輸出。用 `store` 或 `store_as` 將位元組寫入應用程式的 Storage 磁碟。

已儲存於磁碟的錄音用 `Transcription::from_storage` 載入，也可用 `from_path`、`from_upload` 或 `from_bytes` 讀取本機輸入。已知語言時設定 `language()`。需要說話者和分段時間時，使用 `diarize(true)` 並選擇支援此功能的模型。回應包含文字與上游實際回報的用量。OpenAI 相容連線可透過 `models.audio` 或 `models.transcription` 啟用相應能力。

## 分類 {#classification}

將 `AI_CLASSIFICATION_PROVIDER` 設為連線名稱，並將該連線的預設模型設為支援對應 Decisions API 的模型 ID。

| 連線 | 預設模型環境變數 |
| --- | --- |
| `openai` | `OPENAI_CLASSIFICATION_MODEL` |
| `typesafe` | `TYPESAFE_CLASSIFICATION_MODEL` |
| `openrouter` | `OPENROUTER_CLASSIFICATION_MODEL` |
| `gemini` | `GEMINI_CLASSIFICATION_MODEL` |

各預設沿用自己的金鑰和 URL 設定。TypeSafe 使用 `TYPESAFE_API_KEY` 和 `TYPESAFE_URL`。自訂連線可用 `system-one` 驅動程式接入 TypeSafe 協定，或在 OpenAI 相容連線上設定 `models.classification.protocol = "openrouter-decisions"`，讓分類請求使用 OpenRouter 的 `/api/alpha/decisions`。支援 OpenAI Decisions 的相容連線可透過 `models.classification` 設定預設模型並啟用該協定。

```rust
use openrust_ai::{Classification, classification::{Boolean, Choice}};

let result = Classification::of("The item arrived damaged")?
    .question("eligible", Boolean::new("Is this a return request?"))
    .question("category", Choice::new("Choose a category", ["return", "other"]))
    .classify()
    .await?;

let category = result.answer("category");
```

分類輸入可為文字、JSON 物件或 JSON 陣列。為問題指定鍵，使用 `Boolean`、`Choice` 或 `Score` 宣告問題類型，也可用 `attachments()` 加入內嵌圖片。回應包含型別化答案、明確的拒答狀態，以及上游實際回報的 Token 數。

TypeSafe 支援 PNG 和 JPEG。狀態為 JSON 物件時，`images` 用於儲存附件參照。OpenRouter 和 Gemini 連線也接受 WebP。實際可用性取決於所選模型和帳號。

在字串或 `Stringable` 上匯入 `Decides` 即可做是非判斷，也可匯入 `DecidesText` 使用 `Str::decide`。匯入 `DecidesBetween` 可從集合選出原始項目。`decide_by` 接受標籤和描述產生函式，`decide_key` 可從鍵到描述的集合選出鍵。`DecideOptions` 可指定機率閾值、連線、模型和逾時。選項機率低於閾值時傳回 `None`。

```rust
use openrust::support::{Collection, Str};
use openrust_ai::{Decides, DecidesBetween};

let eligible = Str::of("The item arrived damaged")
    .decide("Is this a return request?")
    .await?;
let department = Collection::make(["sales", "support"]);
let assigned = department
    .decide("Which department should reply?", "My blender arrived damaged")
    .await?;
```

測試時可用 `Classification::fake([])` 自動產生具型別的答案，也可提供答案對應或完整回應的序列，或使用 `fake_with` 根據請求產生答案。呼叫記錄支援 `assert_classified`、`assert_not_classified` 和 `assert_nothing_classified`。開啟 `prevent_stray_classifications(true)` 後，回應序列用完即會報錯。

## 供應商檔案 {#provider-files}

用 `AI_FILE_PROVIDER` 指定支援 Files API 的具名連線，也可以沿用 AI 預設連線。OpenAI、Azure OpenAI、Anthropic、Gemini 和 OpenRouter 全球端點原生支援。自訂 OpenAI 相容連線提供同一 Files API 時可設定 `files: true`。

```rust
use openrust_ai::{FileUpload, Files};

let document = FileUpload::from_path("policy.txt").await?
    .with_mime_type("text/plain")?;
let file = Files::put(document, None).await?;
let id = file.id();
let private_path = file.store_as("ai", "policy.txt", Some("local")).await?;
```

HTTP 上傳使用 `FileUpload::from_upload`，記憶體位元組使用 `from_bytes`，已設定磁碟上的檔案可使用 `Files::put_from_storage`。在 `FileUpload` 上可覆寫名稱、MIME 類型及上傳用途，預設用途是 `user_data`。Azure 預設使用 `assistants`。Anthropic 和 OpenRouter 在伺服器管理用途，上傳時使用預設設定。

`Files::get` 讀取中繼資料，`Files::list` 依游標分頁。傳回的檔案物件沿用原連線下載、重新整理和刪除。`store_as` 將內容存入私有磁碟路徑。使用者提供檔案 ID 時，先在應用程式中查驗檔案歸屬。 供應商傳回 `downloadable: false` 時，讀取或存檔會傳回 `AiError::FileNotDownloadable`。Anthropic 和 OpenRouter 對使用者上傳的檔案採用此限制。

Gemini 傳回類似 `files/policy` 的資源名稱。`Files::get` 和 `Files::delete` 也接受短 ID。上傳後用 `refresh()` 檢查狀態是否為 `ACTIVE`，分頁時原樣傳回權杖。Gemini 驅動支援上傳與中繼資料操作，應用程式後續需要讀取上傳原檔時，將原檔保存在 Storage。

透過共享閘道呼叫時，使用對應協定的原生格式提交內嵌附件。供應商帳號下的檔案 ID 可在 SDK 中透過建立檔案的連線使用。

測試中用 `Files::fake([])?` 在記憶體中上傳、查詢與刪除檔案。可傳入位元組回應序列，也可用 `fake_with` 依請求 ID 產生內容。在替身上啟用 `prevent_stray_operations(true)`，拒絕未設定的查詢。可用 `assert_stored`、`assert_not_stored`、`assert_deleted` 及 `assert_not_deleted` 驗證操作。`Files::fake_id("policy.txt")` 傳回固定的測試 ID。

## 向量儲存庫 {#vector-stores}

用 `AI_STORE_PROVIDER` 選擇向量儲存庫的具名連線，也可以沿用 AI 預設連線。OpenAI、Azure OpenAI 與 Gemini File Search 原生支援。自訂 OpenAI 相容端點可設定 `stores: true`，同一連線支援檔案上傳時再啟用 `files: true`。

```rust
use openrust_ai::{Files, Stores};

let store = Stores::create("Support policies", None).await?;
let file = Files::get("file-policy", Some(store.provider())).await?;
let member = store.add(&file, &Default::default()).await?;
let ready = store.refresh().await?.is_ready();
```

`StoreOptions` 可指定已查驗的初始檔案 ID。OpenAI 也接受描述和閒置到期天數。`refresh()` 可讀取索引進度及是否就緒。`Stores::list` 與 `Store::files` 支援游標分頁。

儲存庫與檔案須來自同一連線。`Store::remove(&member, false)` 移除成員並保留原檔案。傳入 `true` 會繼續刪除原檔案。移除後刪除失敗時，`DetachedFileDeletionFailed` 會傳回原檔案 ID 供個別處理。使用 `add_file_id` 前先在應用程式中查驗檔案 ID 的歸屬。

使用 Gemini 時，原檔 ID 可使用 `files/policy` 這類資源名稱。匯入會等待索引操作完成，並傳回獨立的文件 ID。已知原檔 ID 時，`StoreFile::file_id()` 傳回 `Some`。Gemini 文件清單省略原檔 ID，因此透過 `remove(&member, false)` 移除清單中的文件。若已建立儲存庫，但有初始檔案匯入失敗，`StorePopulationFailed` 會攜帶已建立的儲存庫 ID。

使用 `Stores::fake([])?` 在記憶體中測試儲存庫建立、成員管理與刪除。它也啟用檔案 Fake，便於上傳檔案、加入儲存庫並驗證是否刪除原檔。若要保留目前的檔案連線，可用 `Stores::fake_with_files([], false)?`。可傳入查詢名稱序列，或用 `fake_with` 設定回呼。可用 `assert_created`、`assert_not_created`、`assert_deleted` 和 `assert_not_deleted` 驗證儲存庫操作。
