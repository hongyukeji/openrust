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

## 支援邊界 {#support-status}

已實作入口清單、靜態匯入、標籤屬性、CSP、開發模式、React refresh 及本頁所列 Blade 輔助方法。預取策略、統一 Vite 字型門面和具型別 Vite 巨集仍在補齊，不能據此推定已支援。

## 執行範圍 {#scope}

這些渲染呼叫不會自行啟動或守護 Node 開發伺服器。
