# HTTP 請求

## 取得輸入

用 `request.query(Some("key"))` 讀取精確的頂層查詢鍵。用 `request.input(Some("team.name")).await` 讀取巢狀輸入，傳入 `None` 可讀取整組輸入。JSON 和表單的頂層欄位優先於查詢參數。只讀取 JSON 本文時使用 `request.json`。

OpenRust 成功讀取本文後會保留原始位元組，供後續讀取。預設上限為 8 MiB。輸入無效回傳 400，超量回傳 413。

### 型別化輸入

使用 `request.integer("value", 42, true).await?`、`request.float("value", 1.25, true).await?` 或 `request.boolean("value", true, true).await?` 讀取。參數依序為輸入鍵、預設值與嚴格模式，點號路徑和輸入優先順序與 `request.input` 相同。

缺少值時使用預設值。啟用嚴格模式後，無效值也使用預設值。例如，`"01"` 無法讀作嚴格整數，但可以讀作浮點數。關閉嚴格模式後，數值依 PHP 強制轉換，可讀取字串開頭的數字部分。布林讀取忽略大小寫，識別 `1`、`true`、`on`、`yes` 為真，`0`、`false`、`off`、`no` 為假。寬鬆模式下無法識別的布林輸入會傳回 false。正文格式錯誤仍會傳回輸入錯誤。

使用 `request.string("label", "fallback", true).await?.trim()` 取得可鏈式操作的字串，`request.str` 是同義入口。嚴格模式接受文字和數字。`request.array(Some("items")).await?` 保留清單和關聯物件，將純量包成單項清單，缺少值或 null 時傳回空清單。`request.collect` 傳回保留原始鍵的集合。傳入 `None` 可讀取整組輸入。

實作 `BackedEnum`，並以 `String` 或 `i64` 為 backing 型別。用 `request.r#enum::<Stage>("stage", None).await?` 讀取單項，用 `request.enums::<Stage>("list").await?` 讀取集合。集合保留原始鍵和重複項目，略過未知 case。單項遇到未知 case 時採用給定的預設值。無法轉換的 backing 型別會傳回 HTTP 400。

使用 `request.date("when", None, None).await?` 依應用程式時區解析日期。以 `request.date("formatted", Some("!d/m/Y H:i:s.u"), Some("Asia/Tokyo")).await?` 指定 PHP 格式和時區。缺少值、null 和空白時傳回 `None`，日期無效時傳回 HTTP 400。解析結果保留實際時區和微秒。

使用 `request.only(&["user.name", "user.note"]).await?` 選取巢狀輸入，並保留明確提供的 null。`request.except` 從輸入副本移除指定路徑。`arrayOnly` 和 `collectOnly` 分別對應 `array` 和 `collect` 的多鍵讀取。這些方法選取輸入值，上傳檔案由 `request.file` 讀取。

使用 `request.interval("duration", None).await?` 讀取 ISO 8601 區間或 `2 hours 30 minutes` 等表示式。欄位以指定單位提供數值時，使用 `request.interval("seconds", Some("second")).await?`。結果保留日曆分量和微秒，明確傳入的 90 秒仍屬於秒分量。缺少輸入或只有空白時傳回 `None`，區間或單位無效時傳回 HTTP 400。

## 檔案

用 `request.file("attachments.0").await` 讀取 multipart 上傳檔案，用 `request.hasFile("attachments").await` 檢查欄位是否包含檔案。上傳物件提供檔案位元組，以及用戶端提供的檔名和媒體類型。儲存前應檢查檔案內容。

輸入讀取及 JSON 回應的完整範例見[控制器](./controllers.md)。

## Cookie

用 `request.cookie(Some("theme"))?` 讀取指定 Cookie，用 `request.cookie(None)?` 讀取整個 Cookie 集合。啟用 `EncryptCookies` 後，受保護的 Cookie 會在進入處理器前解密並驗證。密文無效或名稱綁定不符時，讀取到的值為 null。

## 舊輸入

用 `request.old(Some("profile.name"))` 讀取巢狀舊輸入，用 `request.old(None)` 讀取整個舊輸入集合。快閃值在下一次請求可用，明確的 null 會保留。
