# Scout

Scout 為 Eloquent 模型提供搜尋引擎、模型生命週期同步和佇列整合。內建 `collection` 引擎在記憶體中搜尋模型資料，`null` 停用搜尋；應用程式可以註冊其他引擎。

## 安裝 {#installation}

使用 `./artisan` 時，在應用程式 `Cargo.toml` 的 `[features].default` 中加入 `"scout"`，保留既有功能。套件探索載入 `ScoutServiceProvider`，設定歸套件所有，不建立搜尋資料表。

```sh
./artisan vendor:publish --tag=scout-config
```

發佈後的 Rust 設定需要接入編譯型應用程式載入器並重新建置。重複發佈保留既有檔案；設定快取命中時不重新讀取套件環境變數。預設骨架無需額外登記擴充 Provider 或預裝設定。

## 設定 {#configuration}

`SCOUT_DRIVER` 預設 `collection`，`SCOUT_PREFIX` 預設空字串。`SCOUT_QUEUE`、`SCOUT_AFTER_COMMIT`、`SCOUT_SOFT_DELETE` 和 `SCOUT_IDENTIFY` 預設 `false`。匯入分塊預設 500。編譯型設定可使用 `scout.queue = {"connection":"database","queue":"search"}` 指定連線和佇列；設為 `true` 則使用原生預設佇列。

集合搜尋使用模型的 PostgreSQL、MySQL 或 SQLite 連線，讀取符合資料庫條件的所有候選模型，再對 `to_searchable_array` 執行不區分大小寫的子字串比對。空查詢保留符合條件的模型，空白按字面比對，`"0"` 是有效搜尋詞。該引擎適合較小資料集；資料庫全文搜尋可使用獨立的[原生搜尋介面](../digging-deeper/search.md)。

集合引擎不維護獨立索引，更新、刪除、清空和索引建立沒有額外索引狀態需要變更；清空不會刪除模型資料。未內建遠端引擎、Scout database 引擎、語意搜尋、混合搜尋和嵌入提供方。未知驅動及集合引擎上的語意／混合搜尋明確回傳錯誤。

## 可搜尋模型 {#searchable-models}

為既有 `Model + Clone` 實作 `openrust_scout::Searchable`。預設 `to_searchable_array` 使用 `to_array`，可以覆寫以選擇欄位。`should_be_searchable` 控制搜尋資格，`search_index_should_be_updated` 控制自動更新。

```rust
use openrust_scout::{EngineManager, Searchable};

impl Searchable for Article {}

// 在應用程式 Provider 的 boot 方法中：
app.make::<EngineManager>().await?
    .register::<Article>(app, "Article").await?;
```

明確註冊取代 PHP 動態 trait 啟動，並提供命令使用的模型別名。業務模型保留在應用程式目錄。`boot_searchable(app)` 可使用完整 Rust 型別名稱註冊。`searchable_as` 將前綴加到資料表名稱；可覆寫 `indexable_as`、`get_scout_key` 和 `get_scout_key_name`。自訂字串鍵還應從 `get_scout_key_type` 回傳 `ModelKeyType::String`。

原生觀察器處理儲存、刪除、強制刪除和還原。`after_commit=true` 時等待最外層交易提交，回滾捨棄相應回呼；關閉時，外部引擎可能在交易回滾前已經觀察到改動。`soft_delete=true` 保留軟刪除模型用於索引，普通搜尋仍排除它們。

## 搜尋 {#searching}

在已啟動的 HTTP、Artisan 或 Worker 生命週期中呼叫模型介面。

```rust
let page = Article::search("rust")?
    .where_in("category", vec![1_i64.into(), 2_i64.into()])
    .order_by("id", "asc")
    .paginate(15, 1).await?;
```

支援 `where(field, operator, value)`、`where_in`、`where_not_in`、`order_by`、`order_by_desc`、`latest`、`oldest` 和 `take`。Rust 使用明確運算子參數表達 PHP 兩參數 `where` 的多載。集合引擎未指定排序時按 Scout 鍵倒序。`with_trashed` 和 `only_trashed` 控制軟刪除結果。分頁使用原生分頁器，每頁筆數須為正數；集合分頁統計所有符合模型，設定 `take` 不改變該總數。

`raw` 回傳有序 ID、總數和提供方 JSON。`keys`、`first`、`get`、`paginate` 和 `simple_paginate` 回傳 ID、模型或分頁。`cursor` 回傳串流：集合比對先確定 ID，再透過原生 SQL 按結果順序串流讀取模型。`query(callback)` 修改模型載入查詢；`callback` 取代集合引擎初始資料庫篩選；`with_raw_results` 在映射模型前調整原始結果。回呼回傳 `Result` 並保留錯誤。`within` 和 `options` 向自訂引擎傳遞設定；集合引擎沒有具名索引。

## 同步與佇列 {#synchronization}

模型支援 `searchable`、`unsearchable`、`searchable_sync` 和 `unsearchable_sync`。匯入 `SearchableCollection` 後，原生 Eloquent 集合也可使用這些方法。Sync 方法繞過佇列。佇列更新重新載入目前模型資料；刪除工作攜帶鍵值，資料列已經刪除時仍能移除索引。

為設定的連線和佇列執行普通[佇列 Worker](../digging-deeper/queues.md)。每次應用程式啟動都要註冊搜尋模型，確保新 Worker 程序知道工作處理器。佇列錯誤交由原生重試和失敗處理。集合引擎執行搜尋本身不需要 Worker。

`disable_search_syncing` 與 `enable_search_syncing` 按模型型別控制應用程式內自動同步。`without_syncing_to_search(future)` 暫時抑制目前非同步呼叫鏈的自動同步，並在巢狀呼叫和取消後還原；手動呼叫 `searchable` 仍表示明確執行要求。

## 命令 {#commands}

```sh
./artisan scout:import Article
./artisan scout:import Article --fresh --chunk=500
./artisan scout:flush Article
```

`scout:import` 遵循模型搜尋資格及佇列設定；`--fresh` 先清空引擎再匯入，`-c` 為 `--chunk` 的短選項。`scout:flush` 只清空引擎狀態。未知模型別名和零分塊大小明確失敗。部署後使用 `./main artisan`。

透過 `Searchable::make_all_searchable(Some(chunk))` 從模型入口批次匯入。它與命令共用匯入流程：`make_all_searchable_using` 約束查詢，`should_be_searchable` 篩選模型，並遵循已設定的軟刪除與佇列規則。傳入 `None` 使用設定中的 chunk，明確傳入零則失敗。保留這些模型鉤子，無須另外實作一套分頁迴圈。

## 自訂引擎 {#custom-engines}

實作 `engines::Engine<Article>`，透過 `manager.extend::<Article>("custom", engine)` 註冊實例，再使用 `scout.driver` 或模型 `scout_driver` 選擇。引擎接收強型別模型與 `Builder` 設定，回傳 `SearchResults`，透過 `ScoutFuture` 保留錯誤。結果 ID 按引擎順序映射回原生模型。引擎註冊按應用程式隔離；自訂引擎自行提供網路用戶端、憑證、索引設定和一致性保證。
