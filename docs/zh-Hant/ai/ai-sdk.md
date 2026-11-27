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

使用 DeepSeek Chat Completions 的思考模式時，採用自動工具選擇。OpenRust 將上游推理欄位保留在工具步驟中，續接同一次呼叫時回傳。需要 DeepSeek 嚴格 JSON Schema 輸出時，可新增一個具名連線，使用 `driver: "openai"` 和 `url: "https://api.deepseek.com/v1"` 接入 Responses API，沿用 `DEEPSEEK_API_KEY` 環境變數。 [DeepSeek 思考模式](https://api-docs.deepseek.com/zh-cn/guides/thinking_mode/) · [DeepSeek Responses API](https://api-docs.deepseek.com/zh-cn/api/create-response/)

AI 命令說明使用應用程式目前的語言。執行 `./artisan help ai:select-model` 或 `./artisan ai:prompt --help` 可查看本地化說明。可在 `lang/vendor/ai/<locale>/console.json` 中透過 `commands.<命令>.description` 覆寫命令說明，透過 `commands.<命令>.arguments.<參數>` 覆寫個別命令的參數說明。

`make:agent`、`make:tool` 和 `make:agent-middleware` 的互動、產生結果與名稱驗證文字使用應用程式目前的語言。可在 `lang/vendor/ai/<locale>/console.json` 的 `make` 下覆寫文字，`created` 中的 `:path` 會替換為產生的檔案路徑。

互動式連線與模型選擇及驗證提示使用應用程式目前的語言。各語言共用相同的數字選項和最終輸出的模型 ID。可在 `lang/vendor/ai/<locale>/console.json` 的 `select_model` 下覆寫互動及驗證提示。

Artisan 使用應用程式目前的語言顯示連線和供應商診斷。可在 `lang/vendor/ai/<locale>/errors.json` 的 `console` 下覆寫文字。`ai:models --json` 繼續輸出原有的模型目錄資料。

透過 `agent(instructions).with_messages(history)` 帶入歷史對話。依序使用 `Message::user(text)` 和 `Message::assistant(text)` 建立歷史訊息，`prompt` 和 `stream` 會先傳送歷史，再傳送本次提示詞。Artisan 的 `--messages` 接受由 `role` 和 `content` 組成的 JSON 訊息陣列。

在 HTTP 路由中，可使用 `agent(instructions).stream(prompt).await?.to_response()` 回傳 SDK 的預設 SSE 事件。也可透過 `into_stream()` 消費事件。

在 `to_response()` 之前呼叫 `using_vercel_data_protocol(message_id)` 可回傳 Vercel AI SDK UI 文字事件，呼叫 `using_agent_user_interaction_protocol(thread_id, run_id)` 可回傳 AG-UI 文字事件。傳入 `None` 時由 SDK 提供 ID。兩種格式都會附上結束狀態和上游回報的 Token 用量。應用程式可使用 `using_protocol` 自訂同一批 Agent 事件的輸出格式。

消費 `StreamableAgentResponse` 前可用 `.catch(|error| { /* 記錄錯誤 */ })` 登記回呼。串流回報錯誤時，各回呼依註冊順序執行一次，消費端仍會收到串流錯誤。連線與請求啟動失敗透過 `stream(...).await` 的結果處理。

可用 `stream.each(|event| { inspect(event); true }).await?` 逐一處理事件並保留回應。回呼傳回 `false` 時暫停本次走訪。之後透過 `each`、`into_stream` 或 `to_response` 消費回應時，會依原本的 ID 重播已走訪的事件，並從同一次上游請求讀取剩餘事件。

可用 `stream.then(callback)` 在串流消費完成時取得 `StreamedAgentResponse`。其中包含合併後的文字與用量、工具呼叫及結果和原始事件。串流暫停等候審批時，也會提供已儲存的審批批次。在 `each` 完整走訪後註冊回呼會立即取得結果。

在回呼中讀取 `result.response.steps`，可取得每個步驟的文字、公開推理摘要、引文、工具呼叫和結果。每個步驟提供供應商回報的模型、用量和結束原因。供應商未回報模型時，沿用該步驟串流開始事件中的模型。暫停等候審批的步驟帶有工具呼叫和已回報的用量。回應使用最後一步的模型，並將暫停步驟中已回報的用量計入彙總。

OpenAI Responses 文字串流的網頁引文在原生 SSE 中使用 `citation` 事件，在 Vercel 中使用 `source-url`，在 AG-UI 中使用名為 `citation` 的 `CUSTOM` 事件。原生引文會附上對應的文字訊息 ID。可在完成回呼中透過 `StreamedAgentResponse::citations()` 依序讀取引文和上游回報的文字位置。 工具續輪會依事件順序保留各次模型呼叫的引文。

Anthropic Messages 串流透過同一套原生 SSE、Vercel 和 AG-UI 格式傳送網頁搜尋與擷取引文。每個模型步驟傳送每個 URL 的首次出現，並保留公開標題。 Gemini GenerateContent 串流也透過這些格式傳送選中候選的 Grounding 網頁來源。同一模型步驟內，跨資料框重複的來源只傳送一次。

OpenAI Responses 的一般回應可透過 `response.meta()` 讀取來源連線、模型與網頁引文。引文沿用上游順序與各文字分段回報的位置。結構化回應也提供該方法。自訂文字驅動可使用 `AgentResponse::new` 建立回應，再填入引文。 非串流工具呼叫的回應中繼資料取自最後一個模型步驟。

內建文字驅動的非串流回應透過 `response.steps` 提供各個模型步驟。每步包含自己的模型、用量、結束原因、公開摘要與引文，以及本步的工具呼叫和結果。頂層用量彙總整個呼叫。

非串流 Anthropic Messages 提供網頁搜尋和擷取引文。非串流 Gemini GenerateContent 提供選中候選的 Grounding 網頁來源。兩者保留每個 URL 首次出現的順序和公開標題。

Responses 模型支援公開推理摘要時，可透過 `with_provider_options` 將 `reasoning.summary` 設為 `auto`。摘要片段在原生 SSE、Vercel 和 AG-UI 中使用獨立事件。可在完成回呼中用 `StreamedAgentResponse::reasoning()` 讀取合併後的摘要。 非串流 Responses 在 `response.reasoning` 中提供公開摘要，結構化回應同樣可讀。非串流工具呼叫提供最後一個模型步驟的公開摘要。

Vercel 和 AG-UI 串流配接器使用目前的應用程式語言顯示產生失敗提示。可在 `lang/vendor/ai/<locale>/errors.json` 下覆寫 `stream` 文字。

使用 `agent(instructions).with_schema(schema).prompt(text)` 請求 JSON 物件。Schema 使用框架既有的 `JsonSchema` 建立。可讀取 `response.structured`，或透過 `response.decode::<YourType>()` 轉成 Rust 型別。所選模型必須支援連線的原生結構化輸出格式。結構化串流會將 JSON 作為文字增量傳送。可透過 `./artisan ai:prompt` 的 `--schema` 傳入 JSON 物件 Schema 進行試呼叫。 SDK 會在送出的 Schema 中停用物件的額外屬性，包括巢狀物件。如果模型執行 OpenAI 嚴格 Schema 驗證，請將所有屬性標示為必填。

目前的請求或命令需要暫時連線時，可使用 `Ai::build(config)`，欄位沿用具名連線設定。透過回傳的 `name()` 在提示詞呼叫中指定連線，或在同一作用域內透過 `Ai::text_provider` 查找。框架保留名稱及應用程式既有名稱無法用作暫時連線。`Ai::flush_state()` 會清除目前作用域的暫時連線。

### 供應商容錯移轉 {#provider-failover}

一般文字、結構化輸出和文字串流可以依序嘗試多個具名連線。匿名 Agent 使用 `with_providers`，自訂 Agent 可實作 `Agent::providers`。每個連線可指定模型，省略時採用該連線的模型級別。

```rust
use openrust_ai::{Agent, ProviderTarget, agent};

let responder = agent("Answer briefly.").with_providers([
    ProviderTarget::new("openai").with_model("first-model-id"),
    ProviderTarget::new("deepseek"),
]);
let answer = responder.prompt("What can I do next?").await?;
```

確定連線未成功建立、流量限制、費用不足或指定的上游過載狀態可觸發切換。請求逾時、送出後斷線，以及收到成功回應後首個資料框中斷時，SDK 傳回錯誤，避免自動重送上游可能已處理的請求。應用程式可監聽 `openrust_ai::events::AgentFailedOver`，用呼叫 ID 關聯已嘗試的連線。使用 `PromptOptions.provider` 明確指定連線時只會呼叫該連線。串流在交付首個事件後固定使用目前連線。具有工具的串流可在啟動時切換，工具結果與後續模型步驟留在選中的連線。非串流工具呼叫可在第一個模型步驟失敗時切換。第一個成功的模型步驟確定連線，後續錯誤會傳回呼叫端。使用提示詞中介軟體的呼叫目前只使用首選連線。

內建文字協定遇到流量限制或指定的過載狀態時，如果上游的 `Retry-After` 是 0 到 10 秒的整數，SDK 會等待後重試目前請求一次。上游回報額度或餘額耗盡時直接依容錯移轉規則處理。切換事件只記錄連線切換，同一連線重試沿用原呼叫 ID。

Anthropic Messages 的 HTTP 529 可觸發重試或切換。Messages 傳回符合固定 Laravel 基線的額度錯誤時也可切換。發生這兩類錯誤時，應用程式可分別比對 `AiError::ProviderOverloaded` 和 `AiError::ProviderInsufficientCredits`，取得 HTTP 狀態。原始錯誤本文不會進入應用程式錯誤。

### Agent 提示事件 {#agent-prompt-events}

非串流提示在每次供應商嘗試前分派 `PromptingAgent`，隨後分派帶有回應的 `AgentPrompted` 或帶有型別化錯誤的 `AgentFailed`。透過框架的 `Event` 門面依各事件的 `NAME` 註冊監聽器。每次嘗試及 `AgentFailedOver` 共用呼叫 ID。`event.prompt.as_prompt()` 可讀取目前連線在步驟中介層執行前的請求參數。

串流呼叫在每次供應商嘗試啟動前分派 `StreamingAgent`。完整消費的串流分派帶有結果的 `AgentStreamed`，啟動或讀取出錯時分派 `AgentFailed`。暫停等候審批時會帶上已儲存的審批批次。持久會話先寫入再分派成功事件，事件帶有會話 ID。請在發起請求前註冊監聽器。用戶端提前停止消費時，可能收不到成功事件。

非串流與串流生成皆可監聽 `StartingStep`、`StepCompleted` 和 `StepFailed`，查看實際發起的模型步驟。步驟編號從零開始。起始事件帶有已解析的請求及先前的工具結果。請求快照包含步驟中介層的調整。完成事件在本步驟的工具執行前分派，帶有供應商回報的模型、用量和結束原因。中介層直接回傳回應時不會產生模型步驟事件。

### 接入 Responses 閘道 {#responses-gateway}

接入提供 OpenAI Responses 的 Sub2API 執行個體時，使用 `driver: "openai"` 和該執行個體的 v1 基礎位址。SDK 使用 `/models` 探索模型，透過 `/responses` 發出生成請求。設定 `store: false` 可請求上游關閉回應儲存。

```rust
config["providers"]["sub2api"] = serde_json::json!({
    "driver": "openai",
    "url": env.get_utf8("SUB2API_URL")?.unwrap_or("https://ai.example.com/v1"),
    "key_env": "SUB2API_API_KEY",
    "store": false,
    "models": {"text": {"default": "gpt-6.1-sol"}},
    "headers": {"x-openai-actor-authorization": "local-image-extension"}
});
```

在 `SUB2API_API_KEY` 設定 Bearer 憑證。執行個體需要額外標頭時，可在連線中設定 `headers`。名稱和值均須符合 HTTP 規範，驗證和通訊協定所需的標頭由連線及 SDK 管理。可用 `./artisan ai:models sub2api --json` 確認設定的模型也在探索結果中。

## Azure OpenAI {#azure-openai}

使用 `azure` 預設時，將 `AZURE_OPENAI_URL` 設為 Azure 資源位址，再填寫 `AZURE_OPENAI_API_KEY`。預設會視需要附加 `/openai/v1`。文字呼叫使用 `AZURE_OPENAI_DEPLOYMENT` 指定部署名稱。新增其他具名連線時，使用 `driver: "openai"`、完整的 v1 URL 和 `auth: "api-key"`。

## 範圍 {#scope}

目前回應提供文字、來源連線、模型，以及上游實際回報的輸入與輸出 Token 數。Anthropic 與 Gemini 原生連線也支援一般文字、SSE 和分頁模型目錄。

文字 `Usage` 提供提示與輸出 Token 總量，並保留上游回報的快取讀取、快取寫入和推理計數。提示總量包含快取計數，輸出總量包含推理計數。可用 `total_tokens()` 和 `uncached_input_tokens()` 根據已知計數取得總量與未快取輸入量，串流回應同樣帶上實際回報的計數。

## 模型閘道 {#model-gateway}

在應用程式 `Cargo.toml` 的 `[features].default` 中加入 `ai-gateway`，並設定 `AI_GATEWAY_ENABLED=true`。執行 `./artisan vendor:publish --tag=ai-gateway-config`，在 `config/ai_gateway.rs` 的 `models` 和 `keys` 中填寫發佈關係及下游 Key 的權限，然後重新建置應用程式：

```json
{
  "models": {"support-chat": {"provider": "deepseek", "model": "deepseek-flash"}},
  "keys": {"support": {"key_env": "AI_GATEWAY_SUPPORT_KEY", "models": ["support-chat"], "requests_per_minute": 60}}
}
```

在應用程式環境中為 `AI_GATEWAY_SUPPORT_KEY` 設定獨立的隨機金鑰，`DEEPSEEK_API_KEY` 仍由上游連線單獨使用。外部用戶端的基礎 URL 設為 `https://你的應用程式網域/v1`，使用下游 Key 作為 Bearer 憑證，模型填寫 `support-chat`。`GET /v1/models` 只顯示目前 Key 有權呼叫的已發佈模型。選用的 `requests_per_minute` 由應用程式的速率限制快取執行。

閘道驗證失敗與 SSE 終止錯誤文字使用應用程式選定的語言，可透過應用程式設定的語言請求標頭選擇。可在 `lang/vendor/ai-gateway/<locale>/errors.json` 中覆寫 `invalid_api_key` 和 `stream`。

`POST /v1/chat/completions` 支援 OpenAI 和相容連線。`POST /v1/responses` 可原生轉發至 OpenAI 連線，也可將請求轉換為 Anthropic Messages。兩個入口皆支援 JSON 和 SSE。Responses 也可路由至只提供 Chat Completions 的連線。這條轉換支援無狀態文字、系統指令、基本生成參數、JSON 物件模式和用戶端函式工具。工具呼叫 ID 原樣回傳，參數增量會串流傳送。附件與 JSON Schema 需要支援對應功能的連線或轉換。Responses 目前採用無狀態模式，將 `store` 設為 `false`。上游會話和舊 Response ID 目前無法使用。伺服器端網頁搜尋需要在公開模型上明確啟用。閘道使用結構化日誌 `ai_gateway.call` 記錄呼叫結果和上游回報的 Token 數。可用 `usage_channel` 指定日誌通道，通道必須允許 `info` 層級。未回報的計數記為 `null`。

使用 Anthropic 用戶端時，基礎 URL 指向應用程式網域，下游 Key 透過 `x-api-key` 提交。閘道支援 `/v1/messages` 和 `/v1/messages/count_tokens`，模型可對應到 Anthropic 原生連線。Messages 請求也可路由至已發佈的 OpenAI Responses 或 Chat Completions 模型。閘道保留用戶端工具呼叫 ID，並依序傳送每個工具的內容區塊。附帶 `x-api-key` 的 `/v1/models` 和 `/v1/models/{model}` 使用 Anthropic 格式，Bearer 模型目錄使用 OpenAI 格式。

透過 `POST /v1/responses/input_tokens` 統計已發佈的 OpenAI 或 Anthropic 模型輸入。傳送包含模型與無狀態輸入的 JSON，閘道呼叫上游計數並回傳 `input_tokens`。Messages 計數入口也可使用 OpenAI Responses 連線。計數請求納入下游 Key 的請求次數，計數結果不計入生成用量。

兩種計數入口也可使用 Bedrock 連線，前提是所選模型和區域支援執行階段的 `CountTokens` 操作。若要同時發佈兩條路徑，在閘道模型的 `endpoints` 中加入 `responses/input_tokens` 和 `messages/count_tokens`。兩種入口都接受文字、系統指令和用戶端函式歷史。Messages 計數亦接受 Bedrock Converse 支援的內嵌 Base64 附件，含文件的訊息必須同時包含文字區塊。閘道記錄對上游的請求，生成用量、計費金額和貨幣預算餘額維持原值。發佈前可確認 AWS 的模型支援情況。[Amazon Bedrock Token 計數](https://docs.aws.amazon.com/bedrock/latest/userguide/count-tokens.html)

用戶端需要不含 `/v1` 的路徑時，在 `config/ai_gateway.rs` 將 `unversioned_paths` 設為 `true`。這會新增 `/models`、`/models/{model}`、`/chat/completions`、`/responses`、`/responses/input_tokens`、`/embeddings` 和 `/messages/count_tokens`，並共用原有授權與限流。

兩個方向的轉換都支援一般 JSON 和 SSE，並向用戶端回傳公開模型名稱。上游回報快取讀寫用量時，閘道會依目標協定回傳相應的計數。

同一個 Anthropic 發佈模型可供 Messages、Responses 和 Chat Completions 用戶端呼叫。轉換支援文字、系統指示、基本取樣參數和用戶端函式工具，支援 JSON 和 SSE。工具呼叫 ID、串流參數增量與文字結果可雙向傳遞。結構化輸出和附件使用對應的原生端點。

Gemini 用戶端使用 `/v1beta` API，透過 `x-goog-api-key` 提交下游 Key。閘道列出已授權的 Gemini 模型，並支援 `/v1beta/models/{model}` 下的 `:generateContent`、`:streamGenerateContent` 和 `:countTokens`。在 SDK 或 Artisan 中選擇 `gemini` 連線，模型 ID 可以帶 `models/` 前綴。

`:streamGenerateContent` 預設以 JSON 陣列串流回傳。可用 `?alt=json` 明確選擇 JSON，或用 `?alt=sse` 選擇 SSE。陣列中的每一項是一個 Gemini 回應區塊。串流開始後若發生錯誤，陣列末尾會提供 Gemini 錯誤物件。

`:countTokens` 可以直接接收 `contents`，也可以在 `generateContentRequest` 中接收完整的提示設定，包括系統指令、用戶端函式、生成設定和選填的閘道 `cachedContent`。內層指定 `model` 時須與路徑中的公開模型一致。每次請求使用其中一種形式。回傳的 `totalTokens` 和選填的 `cachedContentTokenCount` 保留上游計數，並獨立於生成用量。

發佈 Gemini 上下文快取時，啟用閘道用量儲存，執行閘道遷移，並在 Gemini 生成模型上設定 `"gemini_cached_contents": true`。向 `POST /v1beta/cachedContents` 提交快取內容，`model` 填公開模型名稱，例如 `models/support-chat`，在 `x-goog-api-key` 中傳入閘道 Key。用 `GET /v1beta/cachedContents` 列出目前 Key 的快取，支援 `pageSize` 和 `pageToken`。透過 `/v1beta/{name}` 的 GET、PATCH 和 DELETE 管理單個快取，PATCH 可更新 `ttl` 或 `expireTime`。

在 `:generateContent` 或 `:streamGenerateContent` 中，將閘道回傳的 `name` 作為 `cachedContent`，沿用同一個 Key 和公開模型。閘道會將請求送到建立快取的連線。建立時可使用內嵌內容和用戶端函式定義。生成用量依設定的快取輸入費率結算，快取建立和儲存費用仍需供應商帳單證據。群組或帳號預算要求拒絕上限未知的費用時，閘道會在送出請求前拒絕建立和修改有效期。

閘道將建立、修改有效期限和刪除分別記為快取生命週期操作。`ai:gateway:usage:list --billing` 透過 `resource_id` 關聯生成紀錄，並顯示上游請求 ID 與上游回報的資源用量。建立和有效期限更新的上游費用在取得帳單證據前維持待確認，生命週期操作不產生下游應收。帳單包含請求 ID 時，可用 `ai:gateway:billing:import` 匯入對應證據。只有 SKU 彙總的儲存費用應單獨填寫 `category`。閘道保留彙總金額，不將其分攤給單次操作或加到生成 Token 的彙總中。

匯入 Gemini 儲存 SKU 彙總時，將該項的 `category` 設為 `gemini_context_cache_storage`。在模型的上游費率中設定 `cache_storage_per_million_token_hours`，單位為每百萬快取 Token 每小時的金額。對於已結束的帳期，閘道將已確認的快取有效區間裁切至報告期間，依建立和修改有效期限時的費率快照計算，彙總後只捨入一次。`storage.estimated` 是估算金額，`storage.estimated_difference` 是帳單金額減去估算金額。缺少時間、費率或生命週期證據時，受影響的資源維持待確認。估算僅供 SKU 彙總對照。

要發佈 Gemini 非同步批次工作，請啟用閘道用量儲存、發佈並執行 `ai-gateway-migrations`，再將 `batchGenerateContent` 或 `asyncBatchEmbedContent` 加入公開模型的 `endpoints`。將批次工作提交到 `POST /v1beta/models/{alias}:batchGenerateContent` 或 `POST /v1beta/models/{alias}:asyncBatchEmbedContent`。使用閘道回傳的 `batches/…` 名稱查詢、取消或刪除工作。`GET /v1beta/batches` 列出目前 Key 的工作，支援 `pageSize` 和 `pageToken`。各入口均使用 `x-goog-api-key`。

請依批次工作的價格設定具名連線與公開模型費率。閘道只登記一次提交，取得最終項目用量後結算。啟用硬預算時，為上游模型設定可信的單項輸入上限 `input_token_limit`，向量批次也依此上限預留。啟用硬預算或輸出配額的生成批次支援文字輸出。閘道採用最嚴格的設定上限，依每項的候選數換算 `maxOutputTokens`，保留呼叫端較小的上限和原有推理設定。候選輸出與推理 Token 均計入配額。用量無法確認時保留配額預留。Gemini 回傳結果檔案時，閘道在 `response.responsesFile` 中回傳目前 Key 有權存取的檔名。使用 `GeminiBatches::download`，或持同一 Key 請求 `GET /download/v1beta/{responsesFile}:download?alt=media`。結果以 JSONL 串流回傳，單行上限為 64 MiB。下載中斷後從頭重試。

不超過 8 MiB 的 JSONL 可用 `Files::put`，也可透過 `POST /upload/v1beta/files` 啟動工作階段後一次送出 `upload, finalize`。較大的檔案使用回傳的上傳 URL 逐片傳送 `upload`，以 `X-Goog-Upload-Offset` 指定位元組偏移。中斷後傳送 `query`，從 `X-Goog-Upload-Size-Received` 指示的位置繼續。最後一片可用 `upload, finalize`，也可在全部上傳後以檔案總位元組數為偏移傳送空的 `finalize`。閘道每個檔案最多接收 32 MiB，每次請求最多 8 MiB，每個檔案最多 256 片。分片保存在私有 `local` 磁碟，多程序部署時共用該磁碟。持同一 Key 將回傳的 `files/input-…` 填入 `batch.inputConfig.fileName`。透過 `/v1beta/{fileName}` 可讀取中繼資料或刪除暫存檔案。閘道驗證檔案並套用設定的限額，再將 JSONL 上傳至選定連線。批次工作受理後即可刪除暫存檔案。

上傳工作階段與暫存檔案各保留 48 小時，暫存檔案從上傳完成起計時。用 `./artisan ai:gateway:batch-files:prune --pretend` 預覽清理，移除 `--pretend` 即可分批清理到期檔案和中繼資料。在 `routes/console.rs` 登記同一命令即可定時清理。

```rust
Schedule::configure(|schedule| {
    schedule
        .command("ai:gateway:batch-files:prune", [])
        .cron("0 * * * *")?
        .without_overlapping();
    Ok(())
})?;
```

執行[工作排程器](../digging-deeper/scheduling.md)的程序應與閘道共用私有磁碟和用量資料庫。

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

使用 xAI 原生 Responses 時，在公開模型啟用所需工具。`code_interpreter` 使用 xAI 沙箱。`file_search` 只能搜尋 `file_search_collections` 授權的集合，請求中的每個集合 ID 必須完全相符。下游 Key 還須有權存取支援所有請求工具的原生連線。工具費用上限未知時沿用預算設定。啟用硬預算時預設拒絕這類請求，設定 `unknown: "monitor"` 可放行並保留計費觀察。

```rust
"grok-tools": {
    "provider": "xai",
    "model": "grok-4.7",
    "endpoints": ["responses"],
    "server_tools": ["web_search", "code_interpreter", "file_search"],
    "file_search_collections": ["collection-id"],
},
```


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

發佈 Bedrock 文字生圖模型時，只列出 `images/generations`。回應透過 `data[].b64_json` 傳回 Base64 PNG。Titan Image Generator 單次 AWS 呼叫最多生成五張圖，直式和橫式分別對應 Bedrock 的 `768x1152` 和 `1152x768`。Stability Stable Image 使用長寬比和模型預設品質，每次生成一張。Bedrock 生成使用一般 JSON 回應。

啟用 Bedrock 圖片計費前，發佈並執行最新的 `ai-gateway-migrations`。在上游和下游費率中設定 `image`，用十進位字串表示對應幣別的每圖單價，例如 `"image": "0.04"`。依尺寸和品質分級時，設定 `image_tiers`，例如 `{"768x1152/premium": "0.08"}`。閘道依實際送往 Bedrock 的參數選擇費率，Stability 使用 `3:2/default` 這類鍵。未命中時使用 `image` 固定費率。兩項都缺少時，該方向金額保留待核對，硬預算也無法依該方向預留。帳本會儲存選中的 `image_tier` 和單價。只依圖片計費時將 `input` 和 `output` 設為 `"0"`。閘道依請求的 `n` 預留，再依實際傳回且通過驗證的圖片數結算。已知數量會顯示於用量帳本及 `/v1/usage` 的 `output_images`。未知供應商費用保留待核對，可用 AWS 帳單對帳。

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

啟用持久化用量後，閘道會先儲存上游回報的最終用量，再向用戶端傳送完成訊框。若串流在上游回報最終計數前結束，嘗試記錄會保留已收到的計數。

Artisan 嘗試清單的 `usage_state` 區分 `not_dispatched`（未送出）、`unknown`（未知）、`partial`（部分）、`final`（最終）與 `legacy`（歷史）。明確零值會保留。清單也會回傳可取得的送出時間、上游請求 ID 與回報模型。已記錄送出時間表示閘道已進入傳送步驟，中斷的嘗試仍可能需要對帳。

### Token 計費 {#text-billing}

將 `billing.enabled` 與 `usage.enabled` 設為 true。發布並執行閘道遷移。指定價格版本，按連線與上游模型設定成本費率，再按公開模型設定下游應收費率。

```json
{
  "billing": {
    "enabled": true,
    "version": "prices-2026-11",
    "upstream": {
      "openai": {"upstream-model": {"currency": "USD", "input": "1", "output": "4"}}
    },
    "downstream": {
      "published-model": {"currency": "USD", "input": "1.25", "output": "5"}
    }
  }
}
```

費率以十進位字串表示，單位為指定幣別每百萬 Token。分項定價時設定 `cache_read`、`cache_write` 或 `reasoning`。每筆費用使用請求時儲存的費率。缺少價格或用量證據時，金額可保留待核對。執行 `./artisan ai:gateway:usage:list --billing` 可查看成本、應收、價格版本與精確小數金額。每頁在同一條資料庫查詢中讀取用量、費用和帳單證據。

`/v1/embeddings` 和 Gemini 的 `embedContent`、`batchEmbedContents` 只依輸入 Token 計算成本和應收。向量模型的費率可將 `output` 設為 `"0"`。上游明確回報零 Token 時金額為零，缺少輸入用量時金額保留待核對。

在 `groups.<群組>.rate_multiplier` 設定非負十進位字串，例如 `"0.5"`，可調整該群組的公開模型應收。倍率參與預留和結算，結算金額最後統一捨入。費率快照保留請求時使用的倍率。

每次上游嘗試各有一筆記錄。備援嘗試共用 `request_id`。記錄包含下游 Key 或群組、公開模型、連線、上游模型、回應狀態和供應商實際回報的 Token 數。缺少的用量保留為 `null`。帳本省略 Key 金鑰、請求本文和產生的文字。

在 `ai.providers` 中，為連線至自有閘道的 SDK 連線設定 `"forward_usage_context": true`。閘道為每次實際文字請求儲存 `consumer_invocation` 和從零開始的 `model_turn`。執行 `./artisan ai:gateway:usage:list --billing --invocation-id=<id>` 可查看相關嘗試，並用 `--invocation-scope=<scope>` 限定發起呼叫的 Key 範圍。各模型回合有獨立的閘道請求 ID。關聯識別碼不影響計費或請求去重。

SDK Agent 透過自有閘道使用 xAI 原生工具時，在 `ai.providers` 新增獨立連線並設定 `driver: "xai"`。URL 填寫以 `/v1` 結尾的閘道位址，憑證使用下游閘道 Key，`models.text.default` 填寫公開模型別名。設定 `store: false`，啟用 `forward_usage_context` 以關聯各模型回合。將 `ProviderTool::options` 傳給該 SDK 連線名稱。需要在回合間重播供應商工具結果或加密推理時，把公開模型固定到能讀取這些狀態的上游連線。

閘道先登記嘗試，再向上游送出請求。正常結束、出錯和取消都會結算。處理程序在結算前結束時，帳本可能留下 `pending` 記錄供核對。

使用發起模型呼叫的 Key，以 Bearer 或 `x-api-key` 驗證身分並請求 `GET /v1/usage?days=7`。回應包含該 Key 當日、累計與依 UTC 日期彙整的用量。`days` 可設為 1 至 90。遞補只算一次請求，並按實際向上游送出的次數統計嘗試。缺少的 Token 數維持 `null`。

`model_stats` 傳回目前 Key 最近 30 天依公開模型名稱彙整的用量。可用 `start_date` 與 `end_date` 指定 `YYYY-MM-DD` 格式的 UTC 日期，包含開始及結束當天。省略或無法解析的日期使用對應的預設值，此範圍僅影響 `model_stats`。

託管 Key 輪替後保留歷史。替換環境 Key 後，新憑證開始累積自己的歷史。維運人員可透過 Artisan 帳本查詢先前記錄及上游明細。

#### 匯入供應商成本證據 {#import-cost-evidence}

用 `./artisan ai:gateway:billing:import invoice.json --actor=operator` 匯入包含逐請求金額的供應商帳單。指令依 UTC 時間範圍、具名連線與上游請求 ID 比對。時間範圍包含起始時間，排除結束時間。

```json
{
  "source": "provider-invoice",
  "statement_id": "daily-2026-11-26",
  "revision": 1,
  "complete": true,
  "starts_at": 1795651200,
  "ends_at": 1795737600,
  "entries": [
    {
      "id": "invoice-line-1",
      "provider": "openai",
      "upstream_request_id": "req_example",
      "currency": "USD",
      "amount": "0.03"
    }
  ]
}
```

部分帳單使用 `complete: false`。更正時沿用來源、帳單 ID 和時間範圍並增加版本號。同版本同內容可重複匯入。用量清單的 `amount` 保留費率計算值，`actual` 顯示已有證據的供應商成本。幣別不符或報表衝突會保留供維運核對。

### 供應商彙總帳單 {#provider-cost-totals}

供應商依連線與幣別提供彙總時，透過同一匯入指令提交 `totals`。將供應商原始帳期換算為 UTC 時間戳記。Token 費用使用 `category: "tokens"`，其他費用分類保留原金額。

```json
{
  "source": "daily-provider-costs",
  "statement_id": "2026-11-26",
  "revision": 1,
  "complete": true,
  "starts_at": 1795651200,
  "ends_at": 1795737600,
  "totals": [
    {
      "id": "tokens-usd",
      "provider": "openai",
      "currency": "USD",
      "category": "tokens",
      "amount": "0.16"
    }
  ]
}
```

匯入時，閘道會將每個彙總項與對應連線、幣別和帳期的帳本比較。已確認的逐請求帳單證據優先於該筆的費率核算值。結果記錄供應商金額、本地記帳金額、帶正負號的差額，以及已定價和待確認的嘗試數。部分帳單、待確定成本與未對應的費用分類保留供核對。彙總差額保留在帳單層級。

使用匯入指令傳回的 `report_id` 執行 `./artisan ai:gateway:billing:report <report-id>`，可查看已儲存的明細、彙總差額與匯入記錄。

帳單分成多個檔案時，加入 `"page": {"number": 1, "total": 3}`，各檔案的來源、帳單 ID、版本、UTC 帳期與 `complete` 一致。用同一個指令依任意順序匯入各檔案。可透過 `billing:report` 的 `pagination.received_pages` 找出缺頁。閘道在收到全部頁面後合併帳單並記錄證據。所有分頁提交完畢且聲明 `complete: false` 時，仍按部分帳單處理。

### 借項與貸項調整 {#billing-adjustments}

執行 `./artisan ai:gateway:billing:adjust adjustment.json --actor=operator`，為已結算帳目追加借項或貸項。`direction: "cost"` 用於上游成本，`"receivable"` 用於下游應收。使用原帳目幣別，`reference` 填寫支持憑據或行號。若依據是已完整匯入的帳單，可附上該帳單的 `report_id`。

```json
{
  "attempt_id": "01J9GVMJMGV80VGEX9TEXA8ETB",
  "direction": "receivable",
  "kind": "credit",
  "currency": "USD",
  "amount": "0.05",
  "reference": "credit-memo-42",
  "reason": "Approved correction"
}
```

原始費率、金額與證據均保留。`usage:list --billing` 中的 `adjustments` 顯示借項、貸項、淨額及稽核明細。以同一次嘗試、相同方向與憑據編號提交相同內容可安全重試。原金額仍待確認時，淨額維持待核對。彙總帳單使用匯入時可確認的淨額。

### 分組每日貨幣預算 {#billing-budget}

在 `config/ai_gateway.rs` 啟用用量與計費，並執行閘道遷移。在 `groups.<群組>.budget` 設定預算。在 `billing.upstream.<連線>.<模型>` 中提供 `input_token_limit`，使用供應商確認的單次請求可計費輸入 Token 上限，包含快取 Token。下游費率使用與預算相同的幣別。同組所有 Key 及共用同一資料庫的閘道執行個體共享預算。

向量請求依整次上游呼叫的可計費輸入 Token 上限預留，不占用輸出額度。發佈批次向量入口時，需核對整批輸入的上限。

```json
{
  "groups": {
    "team": {
      "models": ["published"],
      "budget": {
        "currency": "USD",
        "daily_limit": "20.00",
        "max_output_tokens": 4096
      }
    }
  }
}
```

閘道限制對外生成的輸出 Token，並按應收上限預留。成功後按儲存的費率結算並釋放差額。未形成下游應收的失敗嘗試會釋放應收預留。缺少最終用量或請求送出後程序停止時，預留留在原 UTC 帳期。供應商實際回報導致應收超過聲明上限時，該群組在本帳期停止受理，實際費用仍可從帳本查看。

執行 `./artisan ai:gateway:budget:show team` 可查看目前 UTC 帳期的限額、預留、已在限額內入帳的金額及可用餘額。餘額不足時回傳 HTTP 429 和 `budget_exceeded`。無法確認上限的路由會回傳 `budget_unbounded`，也可透過 `unknown: "monitor"` 允許通過，並保留通常的用量與計費。

程序重新啟動後，可執行 `./artisan ai:gateway:budget:recover team` 釋放已逾時且未寫入對外送出標記的預留。預設等待 300 秒，可用 `--after=秒數` 和 `--limit=筆數` 調整掃描。復原會在同一交易中關閉嘗試及費用記錄並釋放預留。一般已對外送出的嘗試會繼續保留供對帳。

若 Titan 批次在最終結算前中斷，復原指令會在同一交易處理整批，即使筆數超過 `--limit`。復原會釋放下游應收預留，依已儲存的費率結算已確認的輸入用量，並保留已送出但用量未知的帳號預留。逾時檢查包含最近的送出時間。請在確認原請求已停止後執行復原。

### 上游帳號每日成本預算 {#upstream-cost-budget}

在 `accounts.<連線>.budget` 使用與分組預算相同的欄位，幣別須與上游費率一致。該帳號的預算涵蓋經過此連線的所有公開模型和群組。兩種預算同時適用時，閘道會在對外送出前用同一交易預留上游成本與下游應收。

```json
{
  "accounts": {
    "openai": {
      "budget": {
        "currency": "USD",
        "daily_limit": "100.00",
        "max_output_tokens": 4096
      }
    }
  }
}
```

帳號餘額不足時，閘道可選擇另一個有權限的上游。所有可用帳號的預算都不足時，回傳 HTTP 429 和 `upstream_budget_exceeded`。失敗嘗試的上游成本無法確認時，仍會保留成本預留。這次嘗試的下游應收可獨立記為零。

執行 `./artisan ai:gateway:budget:show openai --account` 查詢帳號餘額，用 `./artisan ai:gateway:budget:recover openai --account` 依連線復原逾時且未對外送出的嘗試。一筆嘗試同時預留兩種預算時，復原會一併釋放。

供應商帳單或借貸項確認費用後，執行 `./artisan ai:gateway:budget:reconcile <attempt-id> --direction=cost --reference=<憑據編號> --actor=operator`。核對下游應收時改用 `--direction=receivable`。指令在原預算帳期內結算已確認金額。重複提交相同憑據編號會傳回原記錄，更正金額時使用新的憑據編號。費用超出原預留上限時，本帳期停止新增呼叫，實際金額仍可核對。

執行 `./artisan help ai:gateway:budget:reconcile` 可依應用程式目前的語言查看閘道命令說明。應用程式可在 `lang/vendor/ai-gateway/<locale>/console.json` 中覆寫 `commands.<命令>.description` 或 `commands.<命令>.arguments.<參數>`。各語言共用相同的命令名稱、參數名稱與選項名稱。

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

SDK 將已完成的使用者訊息和助理訊息一起儲存。下一次提示依時間順序讀取最近 100 則訊息，可在 `config/ai.rs` 中調整 `conversations.max_messages`。資料庫預設沿用應用程式連線，也可用 `AI_CONVERSATIONS_CONNECTION` 指定具名連線。歷史頁面使用 `Ai::conversation_store()?.paginate_conversation_messages(id, &participant, 15, cursor)` 從最新訊息開始游標分頁。串流對話使用 `stream()`，將回傳的 `response` 交給 SSE 轉譯器。呼叫開始後即可讀取對話 ID。SDK 在傳送最終完成事件前儲存該輪訊息。工具步驟完成後如果上游請求出錯，SDK 會保留已完成的步驟並將助手訊息標記為 `failed`。首個工具步驟完成前發生的上游錯誤不會寫入新訊息。最終結構化輸出無效時，也依同樣規則儲存已完成的工具步驟。宣告了回應結構的 Agent 可使用 `prompt_structured()` 或 `stream_structured()`。

預設資料庫儲存使用成功連線的經濟型文字模型為新對話產生標題。標題 Agent 最多讀取第一條提示詞的 500 個顯示欄。可在 `config/ai.rs` 將 `conversations.generate_title` 設為 `false`，或設定 `AI_CONVERSATIONS_GENERATE_TITLE=false`，直接使用 50 個顯示欄的預覽。標題產生失敗時使用 100 個顯示欄的預覽。Agent Fake 可以透過 `ConversationTitleAgent::fake` 指定標題。

執行已發佈的遷移前，可在 `config/ai.rs` 中設定 `conversations.tables.conversations` 和 `conversations.tables.messages`，也可用環境變數 `AI_CONVERSATIONS_TABLE` 和 `AI_CONVERSATION_MESSAGES_TABLE`。預設表名為 `agent_conversations` 和 `agent_conversation_messages`。使用兩個不同的 SQL 識別碼，可含字母、數字和底線，也可使用資料庫支援的 Schema 前綴。遷移、應用程式執行和回復必須使用同一套設定。既有部署要變更表名時，先遷移現有資料表及資料，再切換設定。

已完成的 SDK 助手訊息會儲存模型、彙總用量，以及 `steps` 中各步驟的實際模型、用量、結束原因和工具結果。既有歷史可繼續讀取原有步驟資料。繼續對話時，SDK 依序將助手的工具呼叫及已記錄結果交給模型。`conversations.max_messages` 在展開前限制讀取的儲存列數，歷史分頁仍逐列傳回訊息及其 `steps`。手動建立歷史時，先呼叫 `Message::assistant_with_tool_calls`，再用 `Message::tool_results` 加入結果。

連線、驗證資料和請求模型相同時，工具步驟可以重播 Chat Completions 私人推理、Responses 加密推理、Anthropic 簽名區塊或 Gemini 思考簽名。供應商回報具體模型版本時，仍可用同一個請求別名重播。變更連線或模型時使用已記錄的工具呼叫與結果。公開歷史、回呼與串流事件沿用原有回應結構。

對話訊息和待審批快照在資料庫儲存檔案參照。附件預設寫入 `local` 磁碟，檔案可見性為私人。可在 `config/ai.rs` 設定 `conversations.attachments.disk`，或使用 `AI_CONVERSATION_ATTACHMENTS_DISK` 為多個應用程式程序設定共用的私人磁碟。變更設定時，須保留既有紀錄所參照的磁碟。

已有 Base64 內嵌附件的應用程式可以執行 `./artisan ai:migrate-attachments`。命令寫入並驗證檔案後才更新資料庫參照。移轉失敗的紀錄可繼續依舊格式讀取。命令可以重複執行，JSON 輸出提供移轉的訊息與審批數量。

需要其他對話後端時，實作 `ConversationStore`，並在應用程式啟動時透過 `Ai::set_conversation_store(Arc::new(store))` 登錄。`Ai::conversation_store()` 回傳目前的儲存實作。實作負責檢查參與者歸屬，並以原子操作儲存使用者和助手訊息。需要持久工具審批時使用 `DatabaseConversationStore`。自訂儲存可實作 `supports_attachments` 和 `store_turn_with_message` 以支援附件。

匯入歷史記錄時，可透過 `Ai::conversation_store()` 依序呼叫 `store_conversation`、`store_user_message` 和 `store_assistant_message`。建立時可傳入既有 ID，傳入 `None` 會產生 UUIDv7。每次寫入都需要參與者，新增訊息時會核對會話歸屬。使用者附件由預設資料庫後端寫入設定的私有磁碟。儲存助手訊息時可傳入 `Some(&error)`，記錄會標示為 `failed` 並保存錯誤摘要。每次呼叫各自提交，普通 Agent 會話仍以整輪為單位進行不可分割的儲存。

## 文字生成選項 {#text-generation-options}

在 Agent 上設定輸出 Token 上限與取樣參數。匿名 Agent 用 `with_generation_options`，具名 Agent 實作 `Agent::generation_options`。這些選項也用於結構化輸出與串流工具續步。Agent 中介層可覆寫目前步驟的選項。

```rust
use openrust_ai::{Agent, TextGenerationOptions, agent};

let response = agent("Keep the answer concise.")
    .with_generation_options(TextGenerationOptions {
        max_tokens: Some(256),
        temperature: Some(0.5),
        ..Default::default()
    })
    .prompt("Summarize our return policy.")
    .await?;
```

驅動會產生供應商協定所需的欄位。Anthropic 只向支援取樣的 Claude 模型傳送取樣參數。兩項都設定時，優先傳送 temperature。

### 供應商擴充選項 {#provider-options}

匿名 Agent 用 `with_provider_options` 為具名連線設定擴充欄位，具名 Agent 可實作 `Agent::provider_options`。只有所選連線的擴充欄位會隨請求傳送，並保留在該連線的工具續步中。模型、訊息、串流模式與結構化 Schema 等欄位透過 SDK 設定。Gemini 的 `generationConfig` 可以追加擴充欄位。

```rust
use openrust_ai::{Agent, ProviderOptions, agent};
use serde_json::json;

let options = ProviderOptions::from_iter([("reasoning".into(), json!({"effort": "low"}))]);
let assistant = agent("Keep the answer concise.")
    .with_provider_options("openai", options);
let response = assistant.prompt("Summarize our return policy.").await?;
```

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

所選模型須支援圖片輸入。OpenAI Responses、Chat Completions、Anthropic Messages 與 Gemini 分別使用原生圖片格式。SDK 接收 PNG、JPEG 與 WebP，單次最多 16 張，原始圖片總量須小於 50 MB，同時受供應商限制。可只傳送圖片，也可附上文字。附件會隨本輪呼叫及工具續步。

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

透過 `Message::user_with_attachments(text, attachments)` 和 `with_messages` 可為指定的歷史使用者訊息新增附件。歷史與本輪合計最多 16 個檔案，原始資料必須小於 50 MB。在 Rust 中透過 `Message::attachments` 讀取檔案。一般訊息的 JSON 表示包含角色和文字。

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

### 工具執行事件 {#tool-execution-events}

透過 `Event` 門面監聽 `InvokingTool`、`ToolInvoked` 和 `ToolFailed`。事件共用 Agent 呼叫 ID 和本次處理器執行的工具呼叫 ID。`event.invocation.call` 包含工具名稱、供應商呼叫 ID 和實際參數，其中包含審批時的編輯。終態事件提供結果或型別化錯誤及處理器耗時。等候審批、遭拒絕或略過的呼叫不會執行處理器或分派執行事件。

### 選擇工具 {#tool-choice}

匿名 Agent 使用 `with_tool_choice`，具名 Agent 實作 `Agent::tool_choice`。`Auto` 讓模型自行選擇，`None` 禁止呼叫，`Required` 要求至少呼叫一個工具。`ToolChoice::tool(name)` 指定已註冊的工具。強制呼叫與指定工具只套用於第一步，後續步驟可根據工具結果繼續回答。

```rust
use std::sync::Arc;
use openrust_ai::{Agent, ToolChoice, agent};

let response = agent("Check inventory before answering.")
    .with_tools(vec![Arc::new(LookupInventory)])
    .with_tool_choice(ToolChoice::tool("lookup_inventory"))
    .prompt("Is this item in stock?")
    .await?;
```

### 相似度搜尋工具 {#similarity-search}

`SimilaritySearch::using_model` 把工具收到的查詢文字轉成向量，再透過 Eloquent 查詢文件並回傳業務欄位。需要附加應用程式查詢條件時使用 `using_model_with`，也可用 `using` 提供非同步檢索函式。`rerank` 接收從每筆結果擷取文字的函式與選用的回傳筆數，並使用已設定的重排序連線。

產生 Schema 時，預設工具說明與 `query` 參數說明使用目前語言。可在 `lang/vendor/ai/<locale>/tools.json` 覆寫 `similarity_search.description` 與 `similarity_search.query`。明確呼叫 `with_description` 時，直接採用應用程式提供的說明。

```rust
use std::sync::Arc;
use openrust_ai::{Agent, SimilaritySearch, agent};

let search = SimilaritySearch::using_model::<PolicyDocument>("embedding", 0.6, 15)?;
let response = agent("Search the policy documents before answering.")
    .with_tools(vec![Arc::new(search)])
    .prompt("When can I return an order?")
    .await?;
```

### 將 Agent 作為工具 {#agent-as-tool}

用 `AgentTool::new(your_agent)` 包裝子 Agent，再和一般工具一樣傳給父 Agent。預設使用子 Agent 的型別名稱，工具要求提供 `task` 字串。實作 `CanActAsTool` 並呼叫 `into_tool()` 可明確指定名稱和說明。子 Agent 實作了 `HasStructuredOutput` 時，可透過 `with_structured_output()` 將其 JSON 字串作為工具結果。

子 Agent 使用自己的指令、模型和連線，父 Agent 只提交這項任務。即使父 Agent 正在串流輸出，子 Agent 完成後也會一次回傳工具結果。

預設工具說明與 `task` 參數說明會在產生 Schema 時，依目前應用程式語言及其後備設定解析。可在 `lang/vendor/ai/<locale>/tools.json` 覆寫 `agent.description` 或 `agent.task`。說明中的 `:agent` 會替換成工具名稱。

### MCP 用戶端工具 {#mcp-tools}

應用程式透過已連線的 `rmcp` 用戶端選擇要提供給 Agent 的工具，再將連線對端和工具定義傳給 `McpTool::new`。MCP 的連線、驗證與傳輸由應用程式管理。模型看到帶有 `mcp_tools_` 前綴的工具名稱，OpenRust 呼叫 MCP 時會送出原本的名稱與參數。建立配接器時遇到不支援的參數 Schema 會傳回 `AiError::InvalidSchema`。

```rust
use std::sync::Arc;
use openrust_ai::{McpTool, agent};

let tool = McpTool::new(client.peer().clone(), definition)?;
let assistant = agent("").with_tools(vec![Arc::new(tool)]);
```

可用 `with_approval` 為工具呼叫指定應用程式審批策略。`annotations()` 會傳回 MCP 服務公布的提示。

使用 Streamable HTTP 時，在應用程式的 `rmcp` 依賴啟用 `transport-streamable-http-client-reqwest`，透過 `StreamableHttpClientTransport` 建立連線。共用同一個 MCP 工作階段的 Agent 呼叫可重複使用該連線對端。

若要將應用程式自身的 MCP 服務工具直接提供給 Agent，可將工具定義和非同步處理函式傳給 `McpServerTool::new`。處理函式接收 `CallToolRequestParams` 並傳回 `AiResult<CallToolResult>`。Agent 使用工具原名，並可沿用應用程式審批策略。

### Agent 技能 {#agent-skills}

用 `Skill::from_directory` 開啟含有 `SKILL.md` 的目錄。YAML 中繼資料宣告技能名稱與說明，Markdown 正文提供操作指引。也可用 `Skill::new` 在 Rust 中定義技能，透過 `files` 提供內嵌資源。

匿名 Agent 可用 `with_skills` 宣告技能，具名 Agent 可實作 `Agent::skills`。來源可為 `Skill`、包含多個技能的目錄或解析函式。實際呼叫準備工具時執行解析函式。Agent 同時註冊了 `LoadSkill` 時，OpenRust 會把宣告的技能合併至該工具。

模型呼叫 `load_skill` 時，傳入 `name` 讀取操作指引和檔案清單，另傳相對 `path` 讀取不超過 256 KiB 的 UTF-8 配套檔案。工具提示使用應用程式目前的語言。

使用 `LoadSkill::new` 可直接註冊已解析的技能。`LoadSkill::from_sources([])` 會從應用程式的 `resources/skills` 目錄尋找技能。

```rust
use openrust_ai::{Skill, agent};

let skill = Skill::from_directory("resources/skills/catalog")?;
let assistant = agent("").with_skills(skill);
```

### 工具審批策略 {#tool-approval}

實作 `Tool::should_request_approval` 可依工具參數要求審批。目前呼叫沒有可恢復的審批上下文時，SDK 在執行該步驟的任何工具前回傳 `AiError::ApprovalNotResumable`。一般回應的錯誤包含待審批的呼叫 ID、工具名稱、參數及原因。

```rust
use openrust_ai::{Approval, ToolRequest};

fn should_request_approval(&self, request: &ToolRequest<'_>) -> Option<Approval> {
    let amount = request.get("amount")?.as_u64()?;
    (amount > 100).then(|| Approval::new(Some("Confirm this amount.".into())))
}
```

### 工具審批後恢復對話 {#resume-tool-approval}

用 `./artisan vendor:publish --tag=ai-migrations` 發布 AI 遷移，重新建置應用程式以註冊遷移檔案，再執行 `./artisan migrate --force`。實作 `RemembersConversations` 的 Agent 可透過 `prompt_with_approvals` 在執行目前批次的任何工具前暫停。

```rust
use openrust_ai::{ApprovalResponse, Decision, RemembersConversations};

let result = agent
    .for_participant(participant.clone())
    .prompt_with_approvals(prompt, Default::default())
    .await?;

if let ApprovalResponse::Pending(pending) = result.response {
    let conversation_id = result.conversation_id;
    let approval_id = pending.id;
    // 已驗證的參與者提交決定後，再恢復執行。
    let _result = agent
        .r#continue(conversation_id, participant)
        .resume_approval(&approval_id, Decision::approve_all())
        .await?;
}
```

核准完成後，`ApprovalResponse::Completed(response)` 包含 `Box<AgentResponse>`，可直接透過 `response.steps` 讀取模型步驟。

保留對話 ID 和審批 ID。在同一已驗證的參與者和 Agent 下，用 `pending_approvals_for` 重新讀取待審批項目，再顯示給使用者。使用 `resume_approval` 提交每筆請求的決定。`Decisions::new` 支援逐項核准、拒絕或編輯參數，也可為其餘項目套用預設決定。

使用 `Decision::reject(None)`、布林值 `false` 或空白結果直接拒絕時，本輪在儲存審批決定和已執行的工具結果後結束，後續模型用量為零。若希望模型依拒絕結果繼續，請傳入非空結果，例如 `Decision::reject(Some("使用者要求保留原金額".into()))`。同一批次中已明確核准的工具仍會執行。

`ToolResult.denied` 表示審批拒絕，`ToolResult.failed` 表示執行失敗。`successful()` 會同時檢查兩者，`error()` 會回傳未成功的結果文字。Vercel 串流以 `tool-output-denied` 表示拒絕，AG-UI 會在工具結果的 `metadata.denied` 回報拒絕。手動建立結果時可使用 `ToolResult::new(id, name, result)`，再依需要設定旗標。

`ToolResult.arguments` 保存呼叫使用的參數，包括審批通過的編輯。拒絕時保留原擬呼叫參數。舊記錄缺少該欄位時會以空映射讀取。

Responses 工具呼叫有獨立的項目 ID 與關聯 ID 時，`ToolCall.id` 保存項目 ID，`result_id` 保存協定關聯 ID。提交審批決定時使用審批請求回傳的 ID。上游結果和歷史重播優先使用 `result_id`。

審批暫停時，對話歷史會立即顯示本輪使用者訊息和狀態為 `paused` 的助手訊息。恢復後，工具結果會寫回同一則助手訊息。再次暫停或完成仍沿用該訊息的 ID，完成或失敗時更新狀態。附件參照隨原使用者訊息保留。

審批恢復會在下一次模型請求前儲存這一步的工具結果。應用程式可按同一參與者、Agent、對話和審批 ID 呼叫 `approval_results_for`，讀取已儲存且仍在處理中的結果。完成後可從對話歷史讀取結果。如果恢復程序在續跑途中結束，仍可查詢已儲存結果，重複提交同一審批會被拒絕，以防工具重複執行。

使用 `Decision::edit` 時，工具會以審批後的參數執行。審批快照會將變更後的參數和結果一起儲存，完成後的回應與對話歷史也會保留實際執行參數。重播到需要原生推理區塊的同一連線與模型時，SDK 依供應商協定保留原始區塊，並附上實際工具結果。通用協定重播會使用編輯後的參數。

SDK 在恢復前驗證參與者、Agent 和供應商連線，同一待審批批次只能由一個請求領取。目前審批有效期為 24 小時。領取後的請求失敗或中斷時，工具可能已執行，SDK 不會自動重播。工具執行後如果下一次模型請求失敗，SDK 會把已完成結果儲存到失敗的對話回合，並結清審批。審批快照透過私人檔案參照保留本輪與歷史中的圖片及文件。恢復時先驗證參與者、Agent 和連線，再讀取檔案。

需要串流輸出時，透過 `stream_with_approvals` 建立回應，並使用既有的 SSE 配接器。SDK 先儲存快照，再發出審批事件。預設的 `tool_approval_request` 事件包含 `conversation_id` 與 `approval_id`。Vercel 在 `data-tool-approval` 中交付批次 ID，並為每個呼叫傳送 `tool-approval-request`。AG-UI 在各中斷項目的中繼資料中交付 ID。用 `resume_approval` 提交決定，恢復對話並接收一般回應。

如需串流續跑，呼叫 `resume_approval_stream(&approval_id, decisions)` 並回傳現有 SSE 配接器產生的回應。工具結果儲存後會先送出，接著交付模型增量。下一步再次需要審批時，繼續使用同一個審批 ID。最終結束訊框會在對話寫入成功後送出，歷史保留暫停前與續跑後的步驟及累計用量。

應用程式顯示審批錯誤時，文字使用目前的語言。可在 `lang/vendor/ai/<locale>/errors.json` 的 `approval` 下覆寫個別訊息。選擇 HTTP 狀態或決定是否重試時，可比對 `AiError` 的個別變體。

監聽 `openrust_ai::events::ToolApprovalRequested::NAME` 取得已儲存的審批請求，監聽 `ToolApprovalResolved::NAME` 取得已儲存的處理結果。型別化事件攜帶對話、參與者、Agent 型別、呼叫和審批識別碼。處理完成事件提供該批審批的工具結果。

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

`broadcast_on_queue` 在 Worker 內產生並發布串流。派發前可透過 `invocation_id()` 取得呼叫識別碼，用於關聯用戶端事件。成功回呼接收合併文字、所有模型步驟的彙總用量、工具結果和逐步中繼資料，終止失敗會傳送同一呼叫識別碼的 `stream_failed`。廣播工作預設只嘗試一次，因為訂閱者可能已收到部分事件。

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

在 Agent 上透過 `Agent::middleware` 設定中介層，匿名 Agent 則使用 `with_middleware`。產生的每一步都會收到 `PendingStep`，其中包含目前模型、步驟編號、已完成步驟和用量。呼叫 `next.run(step)` 繼續執行。用 `with_model`、`with_instructions` 和 `only_tools` 修改目前步驟。

非串流呼叫實作 `handle`，串流呼叫實作 `handle_stream`。產生的中介層範本包含兩個進入點。在 `handle_stream` 中，`NextStreamStep::run` 回傳持續讀取上游的 `StepResult`。用 `StepResult::then` 讀取終止訊框的回應，也可用 `StepResult::from_response` 由中介層直接回傳結果。只實作 `handle` 的中介層用於串流呼叫時會回傳 `StreamingMiddlewareNotSupported`。

可在回傳前用 `result.response().await` 讀取完整步驟。這會緩衝該步驟並將事件保留給呼叫方。若要結束串流，在呼叫 `next.run` 前回傳 `StepResult::stop()`，串流會以取消狀態結束，回應保留已產生的步驟和用量。

使用 `with_middleware(vec![Arc::new(Stop::when(|step: &PendingStep<'_>| !step.is_first_step()))])` 可以讓 Agent 在第一次模型請求後停止。從 `openrust_ai` 匯入 `Stop` 和 `PendingStep`。`Stop::new`、`Stop::when` 和 `Stop::unless` 都接受布林值或步驟條件函式。`unless` 會在條件為假時停止。

停止發生在送出模型請求前。首步停止時沒有步驟和用量，也不會建立會話記錄。後續步驟停止會保留已完成的工具結果和用量。串流結束訊框的原因為 `cancelled`。如果結構化產生因停止而沒有最終物件，非串流結果中的 `structured` 是空映射，仍可讀取已完成的步驟。自訂非串流中介層可在呼叫 `next.run` 前回傳 `StepResponse::stop()`。

### 測試 Agent {#agent-fake}

使用 `SupportAgent::fake` 提供有序字串，也可用 `SupportAgent::fake_with` 根據 `RecordedPrompt` 決定結果。回呼可用 `.into()` 回傳文字、JSON 物件或完整的 `AgentResponse`。Fake 為每次呼叫記錄一條提示，包括回呼回傳錯誤的呼叫，可透過 `assert_prompted` 和 `assert_prompted_times` 驗證。回呼在每個模型步驟執行，可看到中介層選定的模型及 `prompt.steps` 中先前的工具結果。模擬模型回應要求已註冊的工具時，會經過正常工具處理器。串流回應保留所提供的模型、用量、公開推理摘要、引文和工具結果。

使用 `SupportAgent::fake_responses` 可依序混合文字、JSON 物件、完整回應和 `ToolCall`。序列為空或耗盡時，結構化提示會依屬性、列舉、預設值、長度與數值範圍產生樣本。啟用 `prevent_stray_prompts(true)` 可要求每個模型步驟都有明確結果。

用 `SupportAgent::assert_queued`、`assert_not_queued` 和 `assert_never_queued` 驗證派發的提示。排隊提示和已執行提示分開記錄。獨立啟動的 Worker 使用所屬應用程式中登錄的 Fake。

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

已發佈的 Bedrock 向量模型也可透過 `/v1/embeddings` 呼叫。Cohere Embed 接受單筆或一批文字，採用模型原生維數。Titan Text Embeddings V2 接受單筆或最多 2048 筆文字，支援 256、512 或 1024 維。閘道在送出前預留整批預算，每筆輸入對應一次 AWS 請求，依輸入順序傳回向量。批內請求失敗時傳回錯誤，保留已確認的上游成本，整批不產生下游應收。兩者皆可傳回浮點陣列，或將小端序 float32 向量編碼為 Base64。所有呼叫均回報輸入 Token 時，閘道傳回合計 `usage`。

將 `AI_EMBEDDINGS_PROVIDER` 設為 `cohere`、`jina` 或 `voyageai`，即可使用對應連線產生文字向量。每條連線與重排序共用金鑰和基礎 URL。

| 連線 | 向量模型 |
| --- | --- |
| `cohere` | `COHERE_EMBEDDING_MODEL` |
| `jina` | `JINA_EMBEDDING_MODEL` |
| `voyageai` | `VOYAGE_EMBEDDING_MODEL` |

Cohere 讀取原生浮點向量和計費輸入 Token，Jina 使用 `retrieval.passage`。Voyage 預設走原生文字介面，選擇 `voyage-multimodal-3` 或 `voyage-multimodal-3.5` 時由多模態介面接收文字。原版 `voyage-multimodal-3` 固定傳回 1024 維。Cohere 的請求維度用於驗證模型輸出。

使用 Gemini 原生 API 時，設定 `AI_EMBEDDINGS_PROVIDER=gemini` 與 `GEMINI_EMBEDDING_MODEL`。SDK 透過 `batchEmbedContents` 傳送文字，單筆也使用同一入口。模型 ID 可帶或省略 `models/` 前綴。結果依輸入順序回傳，指定的維度會傳給上游，Token 數量以 Gemini 實際回報為準。

使用 Groq 時，設定 `GROQ_API_KEY`、`AI_PROVIDER=groq` 和 `GROQ_MODEL`，可用 `GROQ_URL` 覆寫預設位址 `https://api.groq.com/openai/v1`。驅動支援 Chat Completions、SSE、本機工具續步與支援模型上的圖片輸入。無工具的非串流結構化請求使用原生 JSON Schema。使用工具或串流時，SDK 將 Schema 放入提示詞，並檢查最終結果能否解析為 JSON 物件。轉錄設定 `AI_TRANSCRIPTION_PROVIDER=groq` 和 `GROQ_TRANSCRIPTION_MODEL`，傳回文字、時間戳記和上游回報的時長。需要區分說話者時，可選用其他轉錄連線。

使用 Amazon Bedrock Converse 時，設定 `AI_PROVIDER=bedrock` 和 `BEDROCK_MODEL`，並用 `AWS_REGION` 或 `AWS_DEFAULT_REGION` 指定 AWS 區域。可用 `BEDROCK_URL` 覆寫執行階段位址。憑證可使用 `AWS_BEARER_TOKEN_BEDROCK`、明確的 AWS 憑證及選用的 `AWS_SESSION_TOKEN`，或 AWS 憑證鏈。SDK 支援一般文字、會話歷程及內嵌 PNG、JPEG、WebP、PDF、純文字和 Markdown。用 `BEDROCK_MODEL_CHEAPEST` 或 `BEDROCK_MODEL_SMARTEST` 設定獨立模型級別。 如需代入 IAM 角色，在具名連線設定 `assume_role.arn`。可在 `assume_role` 中設定 `session_name`、`duration_seconds` 和 `external_id`，暫時憑證由 AWS SDK 取得和重新整理。

用 `Agent::stream` 呼叫 Bedrock ConverseStream。SDK 讀取 AWS 原生事件串流，並透過 SSE 傳回增量文字和可見推理。結束原因和 Token 用量隨最終中繼資料傳回。串流截斷或 AWS 事件無效會產生串流錯誤，請求逾時也適用於讀取事件。

要透過閘道發布 Bedrock 模型，在 `config/ai_gateway.rs` 的模型 `endpoints` 中加入 `chat/completions`。用戶端以公開模型名稱呼叫 `POST /v1/chat/completions`。一般 JSON 和 SSE 都支援前置 system 或 developer 指令、文字歷史、最大輸出 Token、temperature、top-p 和最多四個停止字串。將 `stream_options.include_usage` 設為 `true` 可收到末尾用量訊框。省略該訊框時閘道仍記錄用量。

此入口支援文字和用戶端函式工具。提交 `tools` 與選用的 `tool_choice`，下一輪保留工具定義，並用帶原始 `tool_call_id` 的 `tool` 訊息送回每個結果。平行結果會合併為一則 Bedrock 訊息。若 `parallel_tool_calls` 為 `false`，閘道會拒絕上游一次傳回多個呼叫的回應。串流工具呼叫在 Bedrock 送出完整參數及最終用量後交付。

Bedrock 已完成的回合含簽名推理時，Chat Completions 會在助理訊息或一筆 SSE 增量裡傳回 `openrust_reasoning_replay`。下一輪將該欄位保留在重建的助理訊息中，原樣提交先前訊息，再附加工具結果或新的使用者訊息。OpenRust SDK 在工具執行中自動處理該欄位。重播使用相同的閘道 Key、公開模型和 Bedrock 連線。明確設定的 Bearer 或 SigV4 憑證沿用原有繫結。使用 AWS 預設憑證鏈時，閘道透過 STS `GetCallerIdentity` 核對呼叫者。同一 IAM 角色更新臨時憑證並切換角色工作階段後，可以繼續原交談。加密狀態有效期一小時，可由共用應用程式密鑰的程序恢復。

使用 Responses 時，設定 `store: false`，依序提交原始輸入和上一回合的全部輸出項目，再附加新的使用者訊息或 `function_call_output`。原樣保留推理項目中的 `reasoning.encrypted_content`。一般 JSON 和 SSE 均傳回該推理項目，也可透過 `include: ["reasoning.encrypted_content"]` 明確請求。

使用者訊息可附帶內嵌 PNG、JPEG、WebP 圖片及 PDF、UTF-8 文字或 Markdown 文件。Chat Completions 在 `image_url.url` 傳 Base64 data URL，文件用 `file` 傳 `filename` 和 Base64 `file_data`。Responses 使用 `input_image` 和 `input_file`。帶文件的訊息需同時提供文字區塊，圖片的 `detail` 支援 `auto`。單次請求最多 16 個附件，其中最多 5 份文件。單張圖片上限 3.75 MB，每邊最多 8,000 像素。單份文件上限 4.5 MB。這兩個入口只接受內嵌資料。

需要結構化輸出時，將 `response_format.type` 設為 `json_schema`，並在 `response_format.json_schema` 中提供 `name`、物件 `schema` 和選用的 `description`。同時發布 `responses` 時，可將這些欄位與 `type: "json_schema"` 一起放在 `text.format`。一般 JSON 和 SSE 均使用 Bedrock 原生約束。請選擇支援結構化輸出的模型，並在 Schema 的每個物件上明確設定 `additionalProperties: false`。此入口需要明確的 Schema，Bedrock 會拒絕不支援的定義。閘道為此模式提供 120 秒，包含首次編譯 Schema 的時間。

Bedrock 的 Converse 和 ConverseStream 均可透過 `with_tools` 呼叫本機函式。SDK 保留原始工具 ID，在下一回合隨結果送回供應商簽名推理。僅在模型支援對應 Bedrock 工具選擇時使用 `ToolChoice::Required` 或 `ToolChoice::tool`。串流工具等到訊息結束和用量事件完整送達後才執行。

使用 AWS 預設憑證鏈時，SDK 透過 STS `GetCallerIdentity` 確認呼叫者。另一個應用程式程序可以用相同的 Bedrock 連線、模型和 IAM 角色核准待執行的工具，也可以在繼續對話時重播已儲存的簽名工具步驟。同一角色更新臨時憑證後仍可繼續。恢復待核准工具時，身分變更或無法確認會在執行工具前拒絕請求。簽名及遮蔽推理保留在內部重播狀態中。明確設定的 Bearer 或 SigV4 憑證繼續使用原有重播規則。

Bedrock 的一般及串流呼叫均支援 `with_schema`。SDK 透過專用工具定義傳送 Schema，並將該工具的參數作為最終 JSON 回應。與本機函式混用時，前面的回合可呼叫本機函式，最後一輪選擇結構化工具。對於固定 Laravel 基線中拒絕強制選擇工具的模型，SDK 將 Schema 加入系統指令，並檢查最終文字能否解碼為 JSON 物件。

使用 Bedrock 文字向量時，設定 `AI_EMBEDDINGS_PROVIDER=bedrock` 和 `BEDROCK_EMBEDDING_MODEL=amazon.titan-embed-text-v2:0`，再呼叫 `Embeddings::r#for`。Titan 每次請求傳送一筆輸入，預設 1024 維。向量索引需要其他維數時，Titan V2 支援 `.dimensions(256)` 和 `.dimensions(512)`。Cohere Embed 模型用 `search_document` 一次傳送整批輸入，維數由模型決定。

使用 Bedrock 重新排序時，設定 `AI_RERANKING_PROVIDER=bedrock` 和 `BEDROCK_RERANKING_MODEL=cohere.rerank-v3-5:0`，再呼叫 `Reranking::of(documents)?.limit(3).rerank(query).await?`。結果依上游傳回的順序提供從零開始的原文索引與相關性分數。需要依排名傳回原始項目時，在 `Collection` 上使用 `Reranks`。

使用 Bedrock 文字生圖時，設定 `AI_IMAGE_PROVIDER=bedrock` 和 `BEDROCK_IMAGE_MODEL=stability.stable-image-core-v1:1`，再呼叫 `Image::of(prompt)?.portrait().generate().await?`。驅動支援 Titan Image Generator，以及使用 Bedrock `images` 回應的 Stability Stable Image 模型。AWS 已於 2026 年 9 月 30 日退役 Nova Canvas，驅動保留其相容後端的請求映射。透過 `square`、`portrait` 或 `landscape` 選擇長寬比，再用 `store` 或 `store_as` 存到已設定的磁碟。需要使用參考圖編輯時，請選擇支援圖片附件的驅動。

執行 `./artisan ai:models bedrock` 可探索目前區域的基礎模型與有效推論設定檔。目錄使用已設定的 Bedrock Bearer Key 或 AWS IAM 憑證鏈。憑證需允許 `bedrock:ListFoundationModels` 和 `bedrock:ListInferenceProfiles`。需要覆寫控制平面基礎位址時，設定 `BEDROCK_CATALOG_URL`。其中一個清單呼叫失敗時，指令仍顯示另一個清單的結果、已設定的文字模型 ID 與探索錯誤。預設模型仍由應用程式選擇。

使用 xAI 文字模型時，設定 `AI_PROVIDER=xai`、`XAI_API_KEY` 和 `XAI_MODEL`。用 `XAI_URL` 覆寫預設位址 `https://api.x.ai/v1`，用 `XAI_MODEL_CHEAPEST` 和 `XAI_MODEL_SMARTEST` 設定模型級別。連線使用原生 Responses，支援 SSE、結構化輸出、本機函式工具，以及受支援模型的影像輸入。上游傳回加密推理時，本機工具續步會將它隨請求送回。執行 `./artisan ai:models xai` 查看探索到的模型及已設定的模型。

使用 xAI 原生連線時，SDK 用量和閘道帳本將推理 Token 計入輸出總量一次，並另外記錄推理子項。上游同時回報總量時，依總量判斷輸出欄位是否已經包含推理。閘道原生轉發保留上游回應中的用量欄位。

xAI 原生回應以美元 ticks 回報實際費用。文字讀取 `response.usage.cost_in_usd_ticks`，影像讀取 `response.usage.tokens.cost_in_usd_ticks`，1 美元等於 10¹⁰ ticks。一般多步 Agent 回應在每一步都回報費用時傳回合計。啟用閘道計費後，`./artisan ai:gateway:usage:list --billing` 的 `reported_cost_usd` 顯示原生 Responses 最終回報的費用，費率估算及下游應收可分別查詢。帳單金額與回報不一致時，`actual.state` 為 `conflicting_evidence`。既有安裝需套用最新的 `ai-gateway-migrations`。

在 xAI 使用供應商工具時，將 `ProviderTool::options` 的結果透過 `with_provider_options` 傳給對應連線。可選 `WebSearch`（最多指定五個允許的網域）、`CodeExecution`（供應商沙箱）和 `FileSearch`（搜尋既有 xAI 集合）。`vector_store_ids` 填寫 xAI 集合 ID。同一個 Agent 可透過 `with_tools` 加入本機函式。

```rust
use openrust_ai::{ProviderTool, agent};

let assistant = agent("").with_provider_options(
    "xai",
    ProviderTool::options([
        ProviderTool::WebSearch {
            allowed_domains: vec!["example.com".into()],
        },
        ProviderTool::FileSearch {
            vector_store_ids: vec!["collection-id".into()],
            max_num_results: Some(10),
        },
    ]),
);
```


使用 Mistral 原生 API 時，設定 `MISTRAL_API_KEY`、`AI_PROVIDER=mistral` 和 `MISTRAL_MODEL`。基礎 URL 預設為 `https://api.mistral.ai/v1`，可用 `MISTRAL_URL` 覆寫。執行 `./artisan ai:models mistral` 查看模型。原生聊天支援一般與 SSE 回應、結構化輸出、工具續步、圖片和文件。SDK 分別傳送推理區塊和答案文字。產生向量時設定 `AI_EMBEDDINGS_PROVIDER=mistral` 和 `MISTRAL_EMBEDDING_MODEL`。SDK 使用模型原生維度，並驗證呼叫端明確指定的維度。

選擇 Ollama 原生 SDK 連線時，設定 `AI_PROVIDER=ollama` 和 `OLLAMA_MODEL`。預設連線為 `http://127.0.0.1:11434`，可用 `OLLAMA_URL` 覆寫。使用需要驗證的代理時可設定 `OLLAMA_API_KEY`。模型目錄使用 `/api/tags`，一般聊天、結構化輸出與工具續步使用 `/api/chat`，支援非串流回應和原生 NDJSON 串流。SDK 逐步傳送文字和推理內容，收到上游步驟終止訊框後才執行該步驟的工具。向量入口使用 `AI_EMBEDDINGS_PROVIDER=ollama` 和 `OLLAMA_EMBEDDING_MODEL` 呼叫 `/api/embed`。

發布 Gemini 原生向量模型時，將模型映射指向 Gemini 連線，並明確設定 `"endpoints": ["embedContent", "batchEmbedContents"]`。已授權的模型會顯示在 `/v1beta/models`。透過 `x-goog-api-key` 呼叫 `POST /v1beta/models/{alias}:embedContent` 或 `POST /v1beta/models/{alias}:batchEmbedContents`，採用 Google 請求格式。正文中的模型名稱必須與發布別名相符。閘道會替換為上游模型，輸入可直接內嵌。

呼叫 `cache(Some(600), None)` 可將向量快取 600 秒。在 `ai.caching.embeddings` 設定預設有效期、快取存放位置，以及是否為所有 SDK 呼叫啟用快取。逐筆快取可跨批次重用向量，只將未命中的輸入送給供應商。第二個參數傳入 `Some(false)` 會依原順序快取整批。單次呼叫可使用 `cache(Some(0), None)` 略過快取。部分命中時，用量只記錄實際送往上游的輸入。

### 使用文字查詢向量 {#querying-embeddings}

匯入 `ToEmbeddings` 可從字串產生一個向量。查詢建構器與 Eloquent 也可透過設定好的 SDK 連線將搜尋文字轉成向量。使用 PostgreSQL 時，啟用 pgvector，並讓向量欄位的維度與嵌入模型一致。

```rust
use openrust::{database::Db, support::Str};
use openrust_ai::ToEmbeddings;

let embedding = Str::of("return policy").to_embeddings().await?;
let documents = Db::table("documents")?
    .where_vector_similar_to_text("embedding", "return policy", 0.8, true)
    .await?
    .get()
    .await?;
```

兩條路徑都使用 `ai.caching.embeddings` 的快取設定。應用程式已註冊自己的資料庫 Embedding Provider 時，可以繼續使用該服務。

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

用 Gemini 產生和編輯影像時，設定 `AI_IMAGE_PROVIDER=gemini`，並用 `GEMINI_IMAGE_MODEL` 指定支援影像的模型。連線沿用 `GEMINI_API_KEY` 和可覆寫的 `GEMINI_URL`。SDK 呼叫 Interactions API。可用 `size("16:9")`、`square()`、`portrait()` 或 `landscape()` 指定長寬比。畫質 `low`、`medium` 和 `high` 分別對應 `1K`、`2K` 和 `4K`，以所選模型支援的規格為準。回應包含傳回的所有影像及上游回報的影像 Token。

使用 xAI 時，設定 `AI_IMAGE_PROVIDER=xai`、`XAI_API_KEY` 和 `XAI_IMAGE_MODEL`。可透過 `XAI_URL` 覆寫連線位址。產生和編輯共用 `Image` 入口，一次編輯最多可提供五張參考影像。用 `size("16:9")` 設定長寬比。畫質 `low`、`medium` 和 `high` 分別請求 `1k`、`2k` 和 `2k` 解析度。

用 `Image::fake([])?` 回傳佔位位元組。測試需要讀取或儲存有效影像時，可傳入含有效圖片的 `ImageResponse`。用 `Image::fake_with` 依記錄的提示詞、參考圖、尺寸、品質和連線建立回應。用 `assert_generated` 檢查產生，用 `assert_queued` 檢查派發。呼叫 `prevent_stray_images(true)` 後，回應序列用盡會回報錯誤。

影像產生或編輯可透過 `queue()` 或 `queue_with(provider, model)` 排入佇列。設定任務後呼叫 `dispatch().await`。透過 `ImageAttachment::from_storage` 或 `from_path` 載入排入佇列任務的參考圖，確保 Worker 能重新讀取。佇列依附件順序保存檔案參照。在應用程式啟動時用 `Image::register_queue_then` 和 `Image::register_queue_catch` 註冊具名回呼。

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

Mistral 沿用 `Audio` 和 `Transcription` 入口。設定 `MISTRAL_API_KEY`，需要替換服務位址時設定 `MISTRAL_URL`。

```dotenv
AI_AUDIO_PROVIDER=mistral
MISTRAL_AUDIO_MODEL=voxtral-mini-tts-3-0
AI_TRANSCRIPTION_PROVIDER=mistral
MISTRAL_TRANSCRIPTION_MODEL=voxtral-mini-transcribe-3-0
```

語音預設回傳 MP3，也支援 WAV、Opus、FLAC 和 PCM。`male()` 使用 `en_paul_neutral`，預設音色和 `female()` 使用 `gb_jane_neutral`。可透過 `voice()` 指定 Mistral 音色 ID。此驅動沿用 Laravel 的對應方式，朗讀風格透過輸入文字表達。`diarize(true)` 請求帶有說話者標記的分段，優先於 `language()`。

Gemini 透過原生 Interactions API 合成語音和轉錄音訊。合成設定 `AI_AUDIO_PROVIDER=gemini` 和 `GEMINI_AUDIO_MODEL`，轉錄設定 `AI_TRANSCRIPTION_PROVIDER=gemini` 和 `GEMINI_TRANSCRIPTION_MODEL`。兩者皆使用 `GEMINI_API_KEY`，可用 `GEMINI_URL` 覆寫連線位址。語音預設回傳 WAV，也可明確選擇 PCM。上游舊版回傳裸 PCM 時，SDK 可封裝為 WAV。專用轉錄模型依詞級時間戳記合併同一說話者的片段。通用 Gemini 模型透過相同入口回傳文字或含時間戳記的片段。

使用 ElevenLabs 時，設定 `ELEVENLABS_API_KEY`，透過 `AI_AUDIO_PROVIDER` 或 `AI_TRANSCRIPTION_PROVIDER` 選用 `eleven`，再用 `ELEVENLABS_AUDIO_MODEL` 或 `ELEVENLABS_TRANSCRIPTION_MODEL` 選擇模型。預設位址是 `https://api.elevenlabs.io/v1`，可用 `ELEVENLABS_URL` 覆寫。執行 `./artisan ai:models eleven` 查看模型。合成支援 MP3、WAV、Opus 和 PCM，可用 `voice()` 指定 ElevenLabs 音色 ID。啟用說話者區分後，轉錄回傳含時間戳記的詞及上游回報的說話者 ID。

使用 xAI 語音轉錄時，設定 `AI_TRANSCRIPTION_PROVIDER=xai` 和 `XAI_API_KEY`。SDK 呼叫原生 `/v1/stt`。設定 `XAI_TRANSCRIPTION_MODEL=grok-voice-transcribe-2.0`，或透過 `generate_with` 指定本次呼叫的模型。可用 `XAI_URL` 覆寫基礎位址。詞級時間戳位於 `segments`。啟用 `diarize(true)` 後，數字說話者編號以字串回傳，例如 `"0"`。供應商回報的音訊時長位於 `usage.seconds`。[xAI 語音轉錄](https://docs.x.ai/developers/model-capabilities/audio/speech-to-text)

在應用測試作用域內呼叫 `Audio::fake([])?` 或 `Transcription::fake(["測試轉錄".into()])?`。用 `fake_with` 依請求產生替身回應。預設語音替身傳回佔位位元組。測試需要播放或轉錄該音訊時，透過 `AudioResponse::from_bytes` 提供有效的音訊樣本。用 `assert_generated`、`assert_not_generated` 和 `assert_nothing_generated` 檢查呼叫。對傳回的替身呼叫 `prevent_stray_audio(true)` 或 `prevent_stray_transcriptions(true)`，可在回應序列耗盡時回報錯誤。

### 排入佇列處理媒體請求 {#queued-audio}

兩種 Builder 都提供 `queue()` 和 `queue_with(provider, model)`。設定佇列和重試選項後呼叫 `dispatch().await`。轉錄排入佇列時，用 `Transcription::from_storage` 或 `from_path` 選擇現有檔案，確保 Worker 能存取具名磁碟上的物件或同一本機路徑。佇列資料保存檔案參照，Worker 執行時解析供應商憑證。

```rust
use openrust_ai::Transcription;

Transcription::from_storage("recordings/notice.mp3", Some("local"))
    .await?
    .diarize(true)
    .queue()?
    .on_queue("media")
    .then("save_transcript")
    .dispatch()
    .await?;
```

在應用程式啟動時用 `Audio::register_queue_then`、`Audio::register_queue_catch` 或 `Transcription` 的同名方法註冊具名回呼，派發端和 Worker 使用相同註冊。重試用盡後執行失敗回呼。測試用 `assert_queued`、`assert_not_queued` 和 `assert_nothing_queued` 檢查派發記錄，可獨立驗證後續是否產生結果。

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

## Gemini 上下文快取 {#gemini-caches}

用 `GeminiCaches::for_connection("gemini")` 建立和管理 Gemini 顯式快取。向 `create` 傳入模型與原生快取欄位。範例中的 `reference` 是達到上游模型最低可快取輸入大小的文字。

```rust
use openrust_ai::{Agent, GeminiCaches, PromptOptions, agent};
use serde_json::json;

let cache = GeminiCaches::for_connection("gemini")?;
let entry = cache.create("your-model", &json!({
    "displayName": "reference",
    "ttl": "1800s",
    "contents": [{"role": "user", "parts": [{"text": reference}]}]
})).await?;
let options = json!({"cachedContent": entry.name});
let response = agent("")
    .with_provider_options("gemini", options.as_object().unwrap().clone())
    .prompt_with("Summarize the reference.", PromptOptions {
        provider: Some("gemini"),
        model: Some(&entry.model),
        ..Default::default()
    }).await?;
```

用 `get` 讀取中繼資料，用 `list` 和回傳的分頁權杖瀏覽，用 `delete` 刪除。更新到期時間時，向 `update` 傳入 `GeminiCacheExpiration::Ttl("3600s")` 或 `GeminiCacheExpiration::ExpireTime(...)`。生成時沿用相同的具名連線與模型。將快取前綴的系統指令與工具定義放在快取中。一般和串流回應的快取輸入量由 `response.usage.cache_read_input_tokens` 回傳。快取操作使用 Gemini SDK 直接連線。

## Gemini 批次工作 {#gemini-batches}

使用 `GeminiBatches::for_connection("gemini")` 選擇具名 Gemini Developer API 連線。將模型 ID 和 Google 的 `batch` 物件內容傳給 `generate(model, &batch)` 或 `embed(model, &batch)`。批次工作支援內嵌請求和 Gemini 輸入檔案。

```rust
use openrust_ai::GeminiBatches;
use serde_json::json;

let batches = GeminiBatches::for_connection("gemini")?;
let job = batches.generate("your-model", &json!({
    "displayName": "inventory",
    "inputConfig": { "requests": { "requests": [{
        "request": { "contents": [{ "parts": [{ "text": "彙整庫存。" }] }] },
        "metadata": { "key": "inventory-1" }
    }] } }
})).await?;
let latest = batches.get(&job.name).await?;
```

儲存回傳的 `batches/…` 名稱，稍後可在 Worker 查詢。`state()`、`done`、`response` 和 `error` 提供 Google 回傳的狀態、結果和單項失敗。使用 `download(&job.name)` 串流讀取已完成工作的 JSONL 結果檔案。分頁時將 `list(limit, cursor)` 回傳的 `next_page_token` 原樣傳回。`cancel` 和 `delete` 採用所選 Gemini 帳號的權限。以上 SDK 呼叫直接連線至 Gemini。[Google Batch API](https://ai.google.dev/gemini-api/docs/batch-api)

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
