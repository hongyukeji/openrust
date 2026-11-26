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

執行 `./artisan ai:select-model --save --default` 選擇模型，並將該連線設為應用程式預設。Artisan 使用目前的預設連線，或在只有一家設定了憑證時直接提示該連線的模型。也可以指定連線名稱。手填模型使用 `--model=your-model-id`，自動化情境可加上 `--no-interaction`。儲存會寫入應用程式實際載入的環境檔。使用設定快取時，儲存後重新產生快取。

透過 `agent(instructions).with_messages(history)` 帶入歷史對話。依序使用 `Message::user(text)` 和 `Message::assistant(text)` 建立歷史訊息，`prompt` 和 `stream` 會先傳送歷史，再傳送本次提示詞。Artisan 的 `--messages` 接受由 `role` 和 `content` 組成的 JSON 訊息陣列。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 回傳 SDK 的預設 SSE 事件。也可透過 `into_stream()` 消費事件。

在 `to_response()` 之前呼叫 `using_vercel_data_protocol(message_id)` 可回傳 Vercel AI SDK UI 文字事件，呼叫 `using_agent_user_interaction_protocol(thread_id, run_id)` 可回傳 AG-UI 文字事件。傳入 `None` 時由 SDK 提供 ID。兩種格式都會附上結束狀態和上游回報的 Token 用量。應用程式可使用 `using_protocol` 自訂同一批 Agent 事件的輸出格式。

使用 `agent(instructions).with_schema(schema).prompt(text)` 請求 JSON 物件。Schema 使用框架既有的 `JsonSchema` 建立。可讀取 `response.structured`，或透過 `response.decode::<YourType>()` 轉成 Rust 型別。所選模型必須支援連線的原生結構化輸出格式。結構化串流會將 JSON 作為文字增量傳送。可透過 `./artisan ai:prompt` 的 `--schema` 傳入 JSON 物件 Schema 進行試呼叫。

目前的請求或命令需要暫時連線時，可使用 `Ai::build(config)`，欄位沿用具名連線設定。透過回傳的 `name()` 在提示詞呼叫中指定連線，或在同一作用域內透過 `Ai::text_provider` 查找。框架保留名稱及應用程式既有名稱無法用作暫時連線。`Ai::flush_state()` 會清除目前作用域的暫時連線。

## 範圍 {#scope}

目前回應提供文字、來源連線、模型，以及上游實際回報的輸入與輸出 Token 數。Anthropic 與 Gemini 原生連線也支援一般文字、SSE 和分頁模型目錄。持久化會話仍待實作。

## 模型閘道 {#model-gateway}

在應用程式 `Cargo.toml` 的 `[features].default` 中加入 `ai-gateway`，並設定 `AI_GATEWAY_ENABLED=true`。執行 `./artisan vendor:publish --tag=ai-gateway-config`，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填寫發佈關係及下游 Key 的權限，然後重新建置應用程式：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在應用程式環境中為 `AI_GATEWAY_SUPPORT_KEY` 設定獨立的隨機金鑰，`DEEPSEEK_API_KEY` 仍由上游連線單獨使用。外部用戶端的基礎 URL 設為 `https://你的應用程式網域/v1`，使用下游 Key 作為 Bearer 憑證，模型填寫 `support-chat`。`GET /v1/models` 只顯示目前 Key 有權呼叫的已發佈模型。選用的 `requests_per_minute` 由應用程式的速率限制快取執行。

`POST /v1/chat/completions` 支援 OpenAI 和相容連線。`POST /v1/responses` 可原生轉發至 OpenAI 連線，也可將請求轉換為 Anthropic Messages。兩個入口皆支援 JSON 和 SSE。Responses 目前採用無狀態模式，將 `store` 設為 `false`，並拒絕上游會話、舊 Response ID 和伺服器端工具。閘道使用結構化日誌 `ai_gateway.call` 記錄呼叫結果和上游回報的 Token 數。可用 `usage_channel` 指定日誌通道，通道必須允許 `info` 層級。未回報的計數記為 `null`。

使用 Anthropic 用戶端時，基礎 URL 指向應用程式網域，下游 Key 透過 `x-api-key` 提交。閘道支援 `/v1/messages` 和 `/v1/messages/count_tokens`，模型可對應到 Anthropic 原生連線。Messages 請求也可轉送至已發佈的 OpenAI Responses 模型。附帶 `x-api-key` 的 `/v1/models` 和 `/v1/models/{model}` 使用 Anthropic 格式，Bearer 模型目錄使用 OpenAI 格式。

兩個方向的轉換都支援一般 JSON 和 SSE，並向用戶端回傳公開模型名稱。上游回報快取讀寫用量時，閘道會依目標協定回傳相應的計數。

同一個 Anthropic 發佈模型可以由 Messages 和 Responses 用戶端呼叫。轉換支援文字、系統指示、基本取樣參數和用戶端函式工具，支援 JSON 和 SSE。工具呼叫 ID、串流參數增量與文字結果可雙向傳遞。結構化輸出和附件使用對應的原生端點。

Gemini 用戶端使用 `/v1beta` API，透過 `x-goog-api-key` 提交下游 Key。閘道列出已授權的 Gemini 模型，並支援 `/v1beta/models/{model}` 下的 `:generateContent`、`:streamGenerateContent?alt=sse` 和 `:countTokens`。在 SDK 或 Artisan 中選擇 `gemini` 連線，模型 ID 可以帶 `models/` 前綴。

公開模型對應可設定 `fallbacks`，依序嘗試備用連線。每項填寫 `provider` 和 `model`，例如 `[{"provider":"openai","model":"backup-model"}]`。無狀態請求遇到上游 429、5xx 等可重試錯誤時，閘道會嘗試備用連線。用戶端仍使用同一個公開名稱，每次嘗試使用同一個請求 ID 記錄。帶有工具、要求儲存輸出或已開始回傳的串流不會重送。

可在 `config/ai_gateway.rs` 中設定 `accounts.deepseek.cooldown_seconds = 30`，讓具名上游連線在回傳 HTTP 429、503 或 529 後進入冷卻。上游有效的 `Retry-After` 優先，最長 24 小時。使用同一連線的所有公開模型共用冷卻狀態。可用連線都在冷卻時，閘道回傳 HTTP 503 及 `Retry-After`。多程序部署可將 `account_cache_store` 指向共用快取，例如 `redis`。

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

## Agent 工具 {#agent-tools}

實作 `Tool` 的名稱、描述、JSON Schema 和非同步 `handle`。用 `ToolRequest::validate` 執行框架驗證規則，或透過 `decode` 讀取 Rust 型別的參數。工具可以讀取上游呼叫 ID 和本次執行的獨立 ID。

```rust
let response = agent("回答前先查目錄。")
    .with_tools(vec![std::sync::Arc::new(LookupInventory)])
    .prompt("還有多少庫存？")
    .await?;
let answer = response.text;
```

工具呼叫支援 Chat Completions、Responses、Anthropic Messages 和 Gemini GenerateContent 的非串流請求。Claude 和 Gemini 會收到原始模型內容及工具結果，讓供應商核對簽章。OpenAI 和 OpenAI 相容連線也支援在 `Agent::stream()` 中使用工具。SDK 即時送出文字，在下一次模型呼叫前送出完整的工具呼叫及結果。預設 SSE、Vercel UI 和 AG-UI 都提供對應的工具事件。Agent 用原呼叫 ID 把結果送回模型，並在回應中提供 `tool_calls` 和 `tool_results`。用 `with_max_steps(n)` 限制模型步驟數。最後一個步驟提出的工具呼叫會記錄為失敗結果，工具不會執行。

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
