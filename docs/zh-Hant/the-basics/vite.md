# 資源建置／Vite

OpenRust 透過 `openrust::support::facades::Vite` 解析 JavaScript／CSS 入口清單，產生開發或正式資源標籤。底層服務為 `openrust::foundation::Vite`，無需額外安裝命令；前端專案仍需自行安裝和啟動 Vite。既有 `@fonts` 字型設定見 [Blade 範本](blade.md)。

## Blade 入口 {#available-entry-points}

```blade
@vite(['resources/css/app.css', 'resources/js/app.js'])
@viteReactRefresh

<img src="{{ Vite::asset('resources/images/logo.png') }}">
<meta name="csp-nonce" content="{{ Vite::cspNonce() }}">
```

## 資源設定 {#configuration}

在服務提供者的 `boot` 中設定應用預設值。預設建置目錄為 `public/build`，清單名稱為 `manifest.json`，開發標記檔案為 `public/hot`；以 `use_hot_file(path)` 指定其他標記檔案。`@vite` 接受一個入口字串或入口陣列，第二參數可指定建置目錄。`with_entry_points`／`merge_entry_points` 設定 `to_html()` 使用的入口，一般 `@vite` 仍使用自己明確傳入的入口。

```rust
use openrust::support::facades::Vite;

Vite::use_build_directory("build")?;
Vite::use_manifest_filename("manifest.json")?;
Vite::with_entry_points(["resources/js/app.js"])?;
Vite::use_script_tag_attributes([
    ("defer".into(), true.into()),
    ("data-app".into(), "OpenRust".into()),
].into())?;
```

## 正式與開發模式 {#asset-modes}

前端建置時啟用 manifest，並將輸出寫入設定的公開建置目錄。`Vite::render(entries, None)` 回傳 `HtmlString`，`to_html` 回傳字串。框架遞迴解析靜態匯入及其 CSS，去除重複標籤，將 CSS 標籤排在入口指令碼前，並依據實際清單產生預載標籤和完整性摘要。`preloaded_assets()` 回傳預載屬性；`flush()` 清空本次執行狀態，保留設定和應用內的清單快取。

開發標記檔案存在時，其內容提供開發伺服器 URL，標籤包含 `@vite/client` 與指定入口。`react_refresh()`／`@viteReactRefresh` 僅在開發模式產生 React 前置指令碼。`asset(path, build)` 回傳 URL，`content(path, build)` 回傳原檔案位元組，即使開發模式也仍讀取正式清單；建置目錄參數以 `None` 採用預設值。`manifest_hash(None)` 在開發模式或清單缺少時回傳 `None`，其餘情況回傳目前檔案 MD5；摘要讀取獨立於已解析清單快取。

## 回呼與隔離 {#callbacks-and-lifecycle}

以 `create_asset_paths_using` 自訂資源 URL，`clear_asset_path_resolver` 還原 URL 產生器。標籤靜態屬性或 `use_script_tag_attributes_using`／`use_style_tag_attributes_using` 回呼接收實際入口、URL、資源區塊與清單組成的 `TagContext`，開發模式後兩者為空。`use_integrity_key(Some("integrity"))` 選擇摘要欄位，`None` 停用摘要；`use_preload_tag_attributes(None)` 停用預載。預載回呼回傳 `Option<Attributes>`，`None` 停止該標籤。

回呼執行時不持設定鎖，可重入設定；會失敗的屬性回呼使用 `*_using_fallible`，回傳 `ViteResult`，可用 `ViteError::callback(error)` 保留實際錯誤。屬性值支援純量文字／數字，true 表示獨立屬性，false／null 表示省略；這些值來自受信任的應用設定，巢狀屬性回傳 `InvalidAttribute`。標籤在屬性回呼之後產生，回呼修改 nonce 的生效時機與固定契約一致。

Provider 預設值持續保留。HTTP、Artisan 和每個 Worker 工作分別擁有執行設定、nonce 與預載狀態，一個生命週期的覆寫不會成為下一個生命週期的預設值。在 HTTP 中介層呼叫 `Vite::use_csp_nonce(None)?` 產生 40 位 nonce，再以回傳值設定回應 CSP 標頭。路由持有可變請求、需要呼叫視圖或回應門面時，沿用 `request.with_container(async { ... }).await` 借出請求。

## 字體 {#fonts}

`Vite::fonts(None)` 回傳全部字體的 `HtmlString`；用 `Vite::fonts(Some(&["sans", "mono"]))?` 按順序選擇別名，`Some(&[])` 表示不選字體。`@fonts` 和 `Vite::fonts` 都委派同一服務，與一般 Vite 資源共用預載去重記錄。

正式字體清單預設為 `public/build/fonts-manifest.json`，在 Provider 中透過 `use_build_directory` 和 `use_fonts_manifest_filename` 修改。開發模式讀取已設定 hot 標記旁的 `fonts-manifest.dev.json`，也支援公開目錄之外的自訂標記；清單缺少時不輸出標籤。

版本 1 清單包含 `families`、`preloads` 和可選 `style`。篩選呼叫按別名順序輸出 `familyStyles` 與 `variables`；未篩選時優先 `style.inline`，再讀取 `style.file`。未知別名或本次選中預載項目無效會回傳錯誤。預載回呼收到 `source = "fonts"`、空資源塊和空清單，可以覆寫屬性或停用字體連結；連結與內嵌樣式都使用目前 CSP nonce。

`flush()` 也會清空應用程式已解析的字體清單和預載狀態，保留設定及 JavaScript／CSS 清單快取。缺少檔案和 JSON 解析失敗不快取。字體 CSS 檔案讀取保留公開目錄能力邊界，並拒絕含樣式結束標籤的 CSS；清單屬性與內嵌 CSS 應來自受信任的建置輸出。

```rust
Vite::use_fonts_manifest_filename("fonts-manifest.json")?;
let fonts = Vite::fonts(Some(&["sans", "mono"]))?;
```

## 資源預取 {#prefetching}

`prefetch(None, None)` 在瀏覽器 `load` 事件後啟用積極預取；傳入 `Some(3)` 使用最多三個並行連結的瀑布策略，第二參數 `Some("vite:ready")` 可改為自訂 window 事件。

`use_waterfall_prefetching(None)` 沿用之前的並行數，初始值為 3；`use_aggressive_prefetching()` 改為一次插入全部連結。`use_prefetch_strategy(None, None)` 關閉預取，支援的策略名稱是 `Some("waterfall")` 和 `Some("aggressive")`；未知名稱在渲染時回傳錯誤。開發熱更新模式不輸出預取腳本。

正式清單中的動態匯入及其相依項和 CSS 按原順序發現，按 URL 去重並排除已預載的資源。腳本在指定事件後執行；瀑布策略在連結載入成功或失敗後各釋放一個名額，積極策略一次插入全部連結。現有預載屬性回呼也會用於預取，腳本使用目前 CSP nonce。

啟用預取時，預載屬性回呼回傳 `None` 會產生 `DisabledPrefetchAttributes`，對應固定原始碼無法展開停用值的錯誤邊界。請回傳受支援的屬性對應，或明確關閉預取策略，不把渲染失敗靜默當成成功。事件名稱和標籤屬性來自受信任的應用程式設定。

```rust
Vite::prefetch(Some(3), Some("vite:ready"))?;
Vite::use_prefetch_strategy(None, None)?;
```

## 支援邊界 {#support-status}

具型別 Vite 巨集與繼承的 Facade 測試輔助入口仍在核對。

## 執行範圍 {#scope}

這些渲染呼叫不會自行啟動或守護 Node 開發伺服器。
