# HTTP 請求

## 請求標頭 {#request-headers}

用 `request.header("X-Trace")?` 讀取首個文字值，請求標頭缺少時回傳 `None`。核心會將無效名稱或非 UTF-8 值產生的錯誤轉為 HTTP 400。用 `has_header` 檢查是否存在，`headers()` 讀取完整的請求標頭對應，`bearer_token()?` 從 Authorization 中擷取 Bearer 權杖。

### 瀏覽器語言偏好 {#language-preferences}

`request.get_languages()` 依權重由高到低傳回有效的 `Accept-Language` 範圍，同權保留輸入順序。標籤使用 BCP 47，例如 `en-US`、`zh-Hant`；`*` 保留為萬用字元。多個同名請求標頭合併處理，重複標籤保留最高有效權重。無效標籤、非法或重複的權重參數、`q=0` 項目會被忽略；權重必須介於 0 到 1，最多三位小數。沒有有效範圍時傳回空清單。

```rust,ignore
let languages = request.get_languages();
let preferred = request.get_preferred_language(Some(&["en", "fr-FR"]))?;
```

`get_preferred_language` 依請求偏好逐一比對有序候選：先完整標籤，再語言／文字體系／地區組合及基礎語言。萬用字元或沒有相符項目時選擇首個候選。傳入 `None` 或空候選時，傳回請求的首選範圍，沒有則為 `None`。候選使用其他語言介面相同的 BCP 47 驗證；任意候選無效都會傳回翻譯錯誤，即使前面的候選已相符。傳回值是 `Result` 包裝的獨立 `Option<String>`。 這是偏好選擇助手，不會自動傳回 406；忽略的 `q=0` 項目不形成永久排除清單，沒有相符項目時仍使用有序候選的首項。

這兩個方法始終讀取標準 `Accept-Language`，不受 `app.locale_header` 影響，不改變應用程式語言，也不依中文地區推斷文字體系。業務、翻譯與頁面需要目前實際語言時使用 `App::current_locale()?`。全域 `SetLocale` 另按配置來源選擇語言，更換其標頭不會改變 Request 標準方法；可透過 `middleware.set_locale(...)` 覆寫候選或關閉辨識，見[應用程式語言協商](../digging-deeper/localization.md#locale-header)。標準解析結果依請求快取，透過 `parts_mut()` 修改標頭時失效；自訂協商標頭共用解析器，但不使用該標準標頭快取。

## 請求路徑、主機和 URL {#request-path-host-and-method}

`path()` 移除路徑兩端的斜線，`decoded_path()` 再進行百分號解碼。用 `segments()` 或從 1 開始的 `segment(index)` 讀取路徑區段。`is(pattern)` 比對解碼後的路徑，`route_is(&["reports.*"])` 比對目前的路由名稱，`is_method("GET")` 檢查生效的 HTTP 方法。

`host()?`、`http_host()?` 和 `scheme_and_http_host()?` 使用生效的連線資訊和可信代理設定。HTTP 主機包含非預設連接埠。`root()?` 也包含可信代理基底路徑。`url()?` 回傳去掉查詢字串的目前 URL。`full_url()?` 和 `full_url_is(patterns)?` 使用正規化查詢字串。用 `full_url_with_query(&serde_json::json!({"page": 2}))?` 或 `full_url_without_query(&["page"])?` 依請求查詢資料產生位址。這些方法回傳新的 URL 字串。簽名驗證使用原始請求目標。

## 取得輸入

### 取得全部輸入 {#retrieving-all-input-data}

用 `request.all().await?` 一起讀取一般輸入和上傳檔案。`RequestValue` 表示 JSON 純量、帶整數或字串鍵的有序集合，以及型別化的 `UploadedFile`。

一般輸入與上傳檔案同名時，優先使用一般輸入，明確的 null 也優先。巢狀集合保留兩邊各自獨有的欄位。上傳檔案仍可從檔案集合讀取。

用 `request.all_only(&["items.1.photo", "name"]).await?` 依路徑選取欄位，缺少的欄位保留為 null。用 `request.keys().await?` 依序列出一般輸入鍵和檔案鍵。兩邊都提供的鍵會出現兩次。

### 取得單一輸入值 {#retrieving-an-input-value}

欄位名稱可直接傳入，既有 `Some(key)` 和 `None` 呼叫仍可使用。預設值分別用 `request.input_or("name", "訪客").await?`、`request.query_or("page", "1")?` 和 `request.post_or("name", "訪客").await?`。`input_or` 保留明確的 null；`query_or`、`post_or` 在缺少值或 null 時使用預設值。空字串、零和 false 均保留。解析錯誤仍向上傳遞。讀取查詢參數優先用 `query`，不另加意義重疊的 `get` 別名。

```rust
let name = request.string_or("name", "訪客").await?;
let page = request.integer_or("page", 1).await?;
let remember = request.boolean("remember").await?;
let keyword = request.query_or("keyword", "")?;
let title = request.post_or("title", "").await?;
```

用 `request.query("key")` 讀取精確的頂層查詢鍵。用 `request.input("team.name").await` 讀取巢狀輸入，傳入 `None` 可讀取整組輸入。JSON 和表單的頂層欄位優先於查詢參數。只讀取 JSON 本文時使用 `request.json`。

用 `request.post(None).await?` 讀取本文參數集合，或用 `request.post("name").await?` 讀取精確的頂層鍵。JSON 請求會讀取解碼後的本文。查詢參數和 multipart 上傳物件各自透過獨立集合讀取。明確的 null 在完整集合中保留，依名稱讀取該值時回傳 `None`。

用 `request.fluent(Some("user")).await?` 將一組一般輸入包裝為獨立的 `Fluent` 屬性集合。傳入 `None` 可包裝全部一般輸入，欄位缺少或為 null 時可用 `fluent_or("user", default)` 提供預設屬性集合。純量無法建構屬性集合，會回傳 HTTP 400。用 `fluent_only(&["user.name", "document"]).await?` 取得保留上傳物件的獨立型別化快照。用 `get` 讀取路徑，用 `value` 讀取精確的頂層鍵。

用 `scope("user")` 取得獨立的子屬性集合。純量和檔案以索引 `0` 包裝。用 `set("user.name", RequestValue::Value(serde_json::json!("Ada")))?` 修改型別化屬性集合。萬用字元路徑會更新現有成員，原請求的輸入不受影響。

OpenRust 成功讀取本文後會保留原始位元組，供後續讀取。預設上限為 8 MiB。輸入無效回傳 400，超量回傳 413。

### 請求邊界 {#request-boundaries}

路徑經百分號解碼後若不是有效 UTF-8，HTTP 核心回傳 400。宣告的 `Content-Length` 超過 8 MiB 時，框架在讀取本文前回傳 HTTP 413。框架為輸入解析或 `body_bytes()` 收集本文時，實際讀取量也以 8 MiB 為上限，包括分塊傳輸的請求。

### 型別化輸入

用 `request.integer("page").await?`、`request.float("price").await?`、`request.boolean("remember").await?` 或 `request.string("name").await?` 讀取。缺少值時分別使用 `0`、`0.0`、`false` 和空字串。自訂預設值用 `integer_or("page", 1)`、`float_or("price", 1.25)`、`boolean_or("remember", true)` 或 `string_or("name", "訪客")`，再加 `.await?`。明確的 null 不會被一般預設值取代，而是依原有規則轉換。

嚴格轉換用 `integer_strict("value", 42)`、`float_strict("value", 1.25)`、`boolean_strict("value", true)` 或 `string_strict("label", "fallback")`。缺少值或型別無效時採用預設值；嚴格模式不是表單驗證，不會因此回傳 422。點號路徑和輸入優先順序保持不變。`str`、`str_or`、`str_strict` 分別是對應字串方法的同義入口。

缺少值時使用預設值。啟用嚴格模式後，無效值也使用預設值。例如，`"01"` 無法讀作嚴格整數，但可以讀作浮點數。關閉嚴格模式後，數值依 PHP 強制轉換，可讀取字串開頭的數字部分。布林讀取忽略大小寫，識別 `1`、`true`、`on`、`yes` 為真，`0`、`false`、`off`、`no` 為假。寬鬆模式下無法識別的布林輸入會傳回 false。正文格式錯誤仍會傳回輸入錯誤。

使用 `request.string_strict("label", "fallback").await?.trim()` 取得可鏈式操作的字串，`request.str` 是同義入口。嚴格模式接受文字和數字。`request.array(Some("items")).await?` 保留清單和關聯物件，將純量包成單項清單，缺少值或 null 時傳回空清單。`request.collect` 傳回保留原始鍵的集合。傳入 `None` 可讀取整組輸入。

實作 `BackedEnum`，並以 `String` 或 `i64` 為 backing 型別。用 `request.r#enum::<Stage>("stage", None).await?` 讀取單項，用 `request.enums::<Stage>("list").await?` 讀取集合。集合保留原始鍵和重複項目，略過未知 case。單項遇到未知 case 時採用給定的預設值。無法轉換的 backing 型別會傳回 HTTP 400。

使用 `request.date("when", None, None).await?` 依應用程式時區解析日期。以 `request.date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo")).await?` 指定 PHP 格式和時區。缺少值、null 和空白時傳回 `None`，日期無效時傳回 HTTP 400。解析結果保留實際時區和微秒。

測試中，`TestApp::travel_to` 和凍結時間方法會為相對日期及格式中省略的欄位提供參考時刻。例如，`tomorrow` 從所用時區的凍結日期計算。格式以 `!` 開頭時，省略的欄位會重設。詳見[控制測試時間](../testing/mocking.md#interacting-with-time)。

使用 `request.interval("duration", None).await?` 讀取 ISO 8601 區間或 `2 hours 30 minutes` 等表示式。欄位以指定單位提供數值時，使用 `request.interval("seconds", Some("second")).await?`。結果保留日曆分量和微秒，明確傳入的 90 秒仍屬於秒分量。缺少輸入或只有空白時傳回 `None`，區間或單位無效時傳回 HTTP 400。

### 透過動態屬性讀取輸入 {#retrieving-input-via-dynamic-properties}

用 `request.field("name").await?` 執行框架動態請求屬性的查找順序。先讀取合併後的輸入和檔案，明確的 null 也算命中。欄位缺少時檢查目前比對路由的同名繫結模型或原始參數。比對 `RequestField::RouteModel(model)` 後用 `model.downcast::<T>()` 讀取繫結模型。一般欄位、上傳檔案和原始路由參數使用 `RequestField::Value`。

### 取得部分輸入 {#retrieving-a-portion-of-the-input-data}

用 `request.only(&["user.name", "document"]).await?` 從一般輸入和上傳檔案中選取欄位。缺少的路徑會略過，明確的 null 和型別化檔案會保留。`request.except(&["document"]).await?` 移除欄位並保留其餘數字鍵。用 `array_only` 或 `collect_only` 將選取欄位讀為型別化陣列或集合。

## Request 門面 {#request-facade}

無請求參數的路由回呼可直接使用 `openrust::support::facades::Request`。門面借用目前的原始請求，讀寫同一份輸入、上傳、使用者與 Session，不會提前消費或複製請求本文。

```rust
use openrust::{http::HttpError, support::facades::{Request, Route}};
use serde_json::json;

Route::post("/profile", || async {
    let name = Request::input("name").await?;
    Request::merge(json!({"verified": true})).await?;
    Ok::<_, HttpError>(json!({"name": name}))
})?;
```

不需要請求參數的控制器動作以 `ControllerDispatcher::without_request(Controller::show)` 註冊，回傳型別仍為 `HttpFuture<'_, Result<T, HttpError>>`；動作及其服務可直接使用門面。明確接收 `&mut http::Request` 的處理器已獨佔借用請求，可使用實例；呼叫使用門面的服務前，以 `request.with_container(async { /* 呼叫服務 */ }).await` 借出請求，完成或取消後自動還原。在 `RequestHandle::with_mut`／`with_async` 回呼內，使用收到的請求，不要再次借用門面。

| 功能 | 入口範例 |
| --- | --- |
| 中繼資料與 URL | `method`、`uri`、`path`、`header`、`full_url`、`get_port` |
| 輸入與上傳 | `input`、`all`、`only`、`file`、`all_files`、`merge`、`replace` |
| 型別化輸入 | `integer`、`string`、`r#enum`、`date`、`interval`、`clamp` |
| 內容協商 | `accepts`、`prefers`、`get_format`、`get_preferred_format` |
| 工作階段與快閃 | `session`、`old`、`flash`、`flash_only`、`flush` |
| 使用者與路由 | `user`、`route`、`route_parameter`、`set_user_resolver`、`set_route_resolver` |
| 驗證與簽章 | `validate`、`validate_with_bag`、`has_valid_signature` |
| 擴充 | `r#macro`、`mixin`、`has_macro`、`flush_macros`、`call` |

`session(|store| ...)`／`get_session(|store| ...)` 在一次安全借用內存取 Store，引用不得離開回呼。`route()` 回傳可擁有的 `ResolvedRoute`，包含名稱、中繼資料、參數及模型繫結；具名參數或模型用 `route_parameter(name)`。`UserResolver`／`RouteResolver` 為明確型別的請求級回呼。使用者解析器接收可變請求與可選 Guard，回傳非同步 `HttpFuture<AuthResult<...>>`，可等待業務查詢。`set_json` 僅接受物件或陣列，驗證失敗時保留原輸入。條件輸入回呼在分支未選中時回傳 `None`，並先釋放內部借用再執行業務回呼。

`instance()` 及 `Facade::get_facade_root()` 回傳 `RequestHandle`，以 `with`、`with_mut`、`with_async` 存取實例；句柄可在同一 HTTP 請求的多次借用之間重用，包括建構注入，但僅在請求被借出時可存取；HTTP 請求結束後失效。容器的 `request` 名稱與 `RequestHandle` 型別也解析到目前句柄，不跨請求快取。`Facade::swap(Arc::new(RequestHandle::from_request(request)))` 替換目前借用的請求，供明確重建或測試使用，不會改動其他請求的容器繫結。

`get_payload()` 讀取表單或 JSON 載荷，不混入 URL 查詢參數；`get_content()` 回傳受大小限制的原始位元組。`uri()` 回傳完整的 `support::Uri`，傳輸層原始 URI 則由請求實例的 `uri()` 讀取。請求語言設定使用 BCP 47，獨立於 `App::set_locale`；格式擴充也按請求隔離。`dump` 輸出選定資料；`dd` 透過 HTTP 例外結束目前回應，不退出伺服器程序。

### 原生服務的設定與建構邊界 {#request-facade-native-boundaries}

PHP 程序全域設定不複製成並行請求共用的可變靜態狀態，對應入口如下：

| 原機制 | OpenRust 入口 |
| --- | --- |
| 全域變數擷取、全域工廠及回寫全域變數 | 伺服器明確提供 `http::Request<Body>` 給 `Request::capture`／`create`／`create_from_base`；自訂工廠使用一般函式或閉包，不讀寫 PHP 全域變數 |
| 從框架請求建構 | `create_from` 轉移框架請求所有權，保留輸入快取、上傳及 Session |
| 複製／重新初始化 | `duplicate().await` 匯出獨立的 `http::Request<Bytes>` 傳輸快照；`initialize(native_request)` 替換目前借用的請求。快照保留原始本文，不複製後續輸入覆寫、Session、路由模型或中介軟體狀態，不能視為完整框架請求複本；需要原狀態時使用句柄 |
| 可信代理及主機的程序級設定與讀取 | 在 `bootstrap/app.rs` 的 `with_middleware` 中持有並設定 `trust_proxies`／`trust_hosts`，或使用既有 `trustedproxy` 設定；不新增全域 Request 靜態開關 |
| 方法覆寫設定 | 在裝配回呼外以 `MethodOverride::new(parameter, allowed)` 建構並檢查策略，再呼叫 `middleware.method_override(policy)`；`None` 允許現有安全規則下的目標方法，空清單關閉全部覆寫。`parameter=false` 只關閉 `_method`，允許清單也限制覆寫標頭。GET、HEAD、CONNECT、TRACE 不可加入允許清單 |

`get_http_method_parameter_override()` 與 `get_allowed_http_method_override()` 讀取目前應用的有效策略，在路由比對前套用，不會跨應用洩漏。沒有 CGI 入口的原生服務中，`get_script_name()` 為空字串，基礎路徑來自可信代理前綴。`server(key)` 回傳明確的傳輸中繼資料，`REMOTE_ADDR` 保留原始連線地址，客戶端地址使用 `ip()`。

## 用戶端 IP 位址 {#request-ip-address}

用 `request.ip()` 讀取第一個解析出的用戶端位址，用 `request.ips()` 讀取位址鏈。直接連線時使用傳輸層提供的對端位址。設定受信任代理後，位址鏈會略過受信任節點，從最近的未受信任位址開始。若所有節點都受信任，則回傳鏈中的第一個受信任位址。見[受信任代理設定](#configuring-trusted-proxies)。

在目前 HTTP 請求內的控制器和服務中，也可以透過 IP 門面讀取：

```rust
use openrust::support::facades::Request;

let ip = Request::ip()?;   // Option<std::net::IpAddr>
let ips = Request::ips()?; // Vec<std::net::IpAddr>
```

`Request::get_client_ip()` 和 `Request::get_client_ips()` 提供對應的 IP 入口，請求實例也提供這兩個別名。它們讀取 HTTP 核心依受信任代理原則解析的連線資訊。HTTP 請求缺少傳輸位址時回傳 `None`／空清單；不在目前 HTTP 請求內時，門面回傳 `BindingResolutionError::FacadeNotSet`。獨立啟動的工作不會繼承此上下文，背景工作需要的位址應明確傳入。其他入口與借用邊界見 [Request 門面](#request-facade)。

## 內容協商 {#content-negotiation}

用 `request.get_acceptable_content_types()` 讀取依品質權重排序的 `Accept` 項目。用 `request.accepts(&["text/html", "application/json"])` 檢查是否接受任一類型，或用 `request.prefers(&["html", "json"])` 選擇最佳候選。缺少該請求標頭或候選皆不相符時，`prefers` 回傳 `None`。`request.format("html")` 回傳首個識別出的回應格式，找不到時回傳傳入的預設值。

`accepts_any_content_type()`、`accepts_json()`、`accepts_html()` 和 `accepts_markdown()` 檢查是否接受對應類型。`wants_json()` 和 `wants_markdown()` 檢查排序第一的項目。`expects_json()` 也識別 `Accept` 為空或萬用字元的非 PJAX XMLHttpRequest。`is_json()` 讀取請求本文的 `Content-Type`。

重複傳送 `Accept` 請求標頭時，依固定框架實作使用首個欄位。需要傳送多種類型時，請在同一欄位中以逗號分隔。

### 原生 HTTP 請求 {#native-http-requests}

HTTP 伺服器將每個原生請求交給 `Kernel::handle_transport`，由它呼叫 `Request::from(native)` 建立框架請求。受信任代理與方法覆寫都在本次請求的生命週期內處理。

與 Rust HTTP 函式庫互操作時，用 `Request::from(native)` 接收擁有所有權的 `http::Request<Body>`，用 `into()` 轉回原生請求。所有權轉換會移動目前的本文串流、請求標頭、協定版本和擴充資料。在路由中用 `request.to_http_request().await?` 建立具有緩衝本文的獨立原生請求。它採用框架輸入的 8 MiB 限額，框架請求仍可讀取本文。

原生請求攜帶 `original_uri()` 傳回的傳輸 URI，以及 `method()` 傳回的生效方法，包含方法覆寫後的結果。用 `uri()` 讀取可信代理處理後的框架 URI，用 `get_real_method()` 讀取原始方法。`version()` 和 `extensions()` 讀取對應的傳輸中繼資料。

傳回原生型別的寫法見[原生 HTTP 回應](./responses.md#native-http-responses)。

### 檢查輸入是否存在 {#input-presence}

用 `request.has(&["profile.name"]).await?` 檢查鍵是否存在，明確設為 `null` 的鍵也算存在。用 `has_any` 檢查任一鍵，用 `missing` 檢查是否有鍵缺少。這些檢查也識別上傳檔案。

用 `filled`、`is_not_filled` 和 `any_filled` 檢查輸入值。缺少的值、`null` 和空白字串視為空。布林值、零和空陣列視為已填寫。只有上傳檔案的欄位可用 `has_file` 檢查。

用 `when_has`、`when_filled` 或 `when_missing` 在條件成立時執行非同步回呼。`when_has` 和 `when_filled` 將合併後的欄位作為 `Option<RequestValue>` 傳給回呼。比對 `RequestValue::File(file)` 即可操作上傳檔案。回呼執行時回傳 `Some(result)`，可用 `unwrap_or_else` 提供預設結果。

### 合併額外輸入 {#merging-additional-input}

用 `request.merge(serde_json::json!({"profile.name": "Ada"})).await?` 按點路徑設定輸入。`merge_if_missing` 保留請求已有的鍵，包括明確的 `null`、查詢後備值和上傳檔案。用 `replace` 傳入物件或陣列可取代目前的輸入來源。

JSON 請求更新 JSON 輸入。非 JSON 的 GET 或 HEAD 更新查詢資料。其他請求更新表單輸入，並繼續使用查詢後備值。這些方法保留原始 URI、原始本文和上傳檔案。

## 舊輸入 {#old-input}

在使用工作階段中介層的路由中，用 `request.flash().await?` 儲存目前輸入，供下一次請求讀取。用 `request.flash_only(&["profile.name", "page"]).await?` 選取欄位，或用 `request.flash_except(&["password", "password_confirmation"]).await?` 排除欄位。欄位選擇支援點路徑。上傳檔案保存在檔案資料中。

用 `request.old(Some("profile.name"))` 讀取舊輸入的一個欄位，或用 `request.old(None)` 取得整份資料。先比對字面鍵，再解析點路徑。明確空值保留為 `Some(Value::Null)`，缺少時可用 `Option` 方法提供預設值。

用 `request.flush()?` 將舊輸入替換為空。工作階段內的其他值仍可讀取。舊輸入在下一次請求結束後過期。

## Cookie {#cookies}

用 `request.cookie(Some("theme"))?` 讀取指定 Cookie，用 `request.cookie(None)?` 讀取整個 Cookie 集合。`request.has_cookie("theme")?` 檢查 Cookie 是否存在，空字串也算存在。

啟用 `EncryptCookies` 後，受保護的 Cookie 會在進入處理器前解密並驗證。驗證失敗時，整個集合仍保留該鍵，其值為 null。依名稱讀取會傳回 `None`，呼叫端可用 `Option` 提供預設值。

## 輸入修剪與正規化 {#input-trimming-and-normalization}

全域中介層先修剪查詢、JSON 與表單中的字串，再把空字串轉為 `null`。頂層 `current_password`、`password` 與 `password_confirmation` 略過修剪，其中的空字串仍會轉為 `null`。原始 URI、本文及上傳檔案仍可讀取。

在 `bootstrap/app.rs` 設定欄位模式，或為特定請求略過某一層中介層：

```rust
.with_middleware(|middleware| {
    middleware
        .trim_strings(["rows.*.secret"])
        .skip_trim_strings_when(|request| request.is("hooks/*"))
        .convert_empty_strings_to_null(|request| request.is("hooks/*"));
})
```

欄位模式使用點路徑和 `*` 萬用字元。例外只對設定它的應用程式生效。

## 檔案 {#files}

### 讀取上傳檔案 {#retrieving-uploaded-files}

用 `request.file("attachments.0").await?` 讀取單一 multipart 上傳檔案。`UploadedFile` 提供檔案位元組，以及用戶端提供的檔名和媒體類型。儲存前應驗證檔案內容。

用 `request.file_value("photos.*.image").await?` 讀取一組檔案或萬用字元比對結果，用 `request.all_files().await?` 讀取整個檔案集合。兩者用 `UploadedFileValue` 表示檔案及帶鍵集合。沒有檔名的上傳欄位會被移除，有檔名的零位元組檔案仍可讀取，其餘檔案保留原有的數字索引。

`request.has_file("attachments").await?` 檢查單一檔案或集合的直接成員。若要檢查更深層的檔案，請指定巢狀欄位或使用萬用字元選取。

輸入讀取及 JSON 回應的完整範例見[控制器](./controllers.md)。

## 設定可信任代理 {#configuring-trusted-proxies}

在應用程式裝配時設定可信任的代理位址。來自非可信任對端的轉送標頭不會生效。策略接受 IPv4 與 IPv6 的確切位址和 CIDR。標頭遮罩指定允許代理設定的欄位。

```rust
use openrust::http::TrustedProxyHeaders;

.with_middleware(|middleware| {
    middleware
        .trust_proxies(&["192.0.2.10", "10.20.0.0/16"], TrustedProxyHeaders::DEFAULT)
        .expect("valid trusted proxy policy");
})
```

預設遮罩 `TrustedProxyHeaders::DEFAULT` 對應 X-Forwarded 欄位。RFC 7239 使用 `FORWARDED`，AWS 負載平衡欄位使用 `AWS_ELB`。同時啟用的兩組標頭發生衝突時回傳 HTTP 400。

`Request::secure` 反映生效的協定。URL 產生使用轉送來源及前綴，路由比對使用傳入路徑。

應用程式裝配未指定代理策略時，HTTP 啟動會從設定儲存庫讀取 `trustedproxy.proxies`，支援以逗號分隔的字串或陣列。選用的 `trustedproxy.headers` 支援數值遮罩或 `HEADER_X_FORWARDED_AWS_ELB` 等框架預設名稱。明確的 `trust_proxies` 優先，包含空位址清單。

## 設定可信任 Host {#configuring-trusted-hosts}

啟用 `trust_hosts` 可限制原始和轉送後的主機名稱。模式使用不區分大小寫的 PCRE 運算式，比對移除連接埠的主機名稱。驗證失敗時，在進入路由前回傳 HTTP 400。

```rust
.with_middleware(|middleware| {
    middleware
        .trust_hosts(&[r"^edge\.example\.com$"], true)
        .expect("valid host patterns");
})
```

啟用 `subdomains` 可同時信任 `app.url` 的主機及其子網域。用 `trust_hosts_with` 可在每次請求時非同步取得模式。環境名稱恰好為 `local` 或 `testing` 時會略過 Host 驗證。
## 請求中繼資料

`ajax()` 檢查 `X-Requested-With: XMLHttpRequest`，`pjax()` 檢查 `X-PJAX`。`prefetch()` 對 `X-Moz`、`Purpose`、`Sec-Purpose` 與 `prefetch` 進行忽略大小寫的完整比較。預取請求不覆蓋 Session 上一頁位址和路由。`user_agent()` 傳回可選標頭值。`fingerprint()` 要求已匹配路由，按方法、網域、URI 和用戶端 IP 產生摘要。透過 `user(guard).await` 使用指定 Guard 解析目前使用者。
