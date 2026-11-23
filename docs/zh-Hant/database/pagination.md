# 資料庫分頁

## 基本用法

`paginate(15)` 先查詢總數，再讀取目前頁面。只需要上一頁和下一頁時，使用 `simple_paginate(15)`，它多讀一列來判斷是否還有下一頁。Query Builder 和 Eloquent 均支援這兩種方法。Eloquent 預設按主鍵排序。Query Builder 可明確指定排序以維持跨頁順序。

使用 `paginate_with` 或 `simple_paginate_with` 並傳入 `PageOptions`，可以自訂頁碼參數名稱、查詢欄位和頁碼。HTTP 從目前請求的查詢字串讀取頁碼。HTTP 之外預設使用第 1 頁。

用 `ModelMetadata::per_page(size)` 宣告模型的預設頁容量。省略 `per_page` 時，Eloquent 及關聯查詢採用所選模型的設定。顯式頁容量優先。Query Builder 預設每頁 15 筆。一般分頁可透過 `PageOptions::total` 傳入預先計算的總數，跳過計數查詢。總數為零時直接回傳空頁，無須讀取紀錄。如果總數來自快取或其他服務，請先取得總數再建立選項。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn page_with_cached_total<M: Model>(total: u64) -> DatabaseResult<LengthAwarePaginator<M>> {
    M::query()?.paginate_with(PageOptions {
        total: Some(total),
        ..Default::default()
    }).await
}
```

一般分頁也支援在 `PageOptions` 中設定回呼。用 `total_using` 提供總數，或回傳 `Ok(None)` 讓查詢執行計數。顯式數值 `total` 優先。總數確定後，`per_page_using` 接收它並選定頁容量，或回傳 `Ok(None)` 沿用顯式容量或模型預設值。回呼發生錯誤時，分頁會在讀取頁面紀錄前終止。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::Model},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn adaptive_page<M: Model>() -> DatabaseResult<LengthAwarePaginator<M>> {
    M::query()?.paginate_with(PageOptions {
        per_page_using: Some(&|total| Ok((total > 100).then_some(50))),
        ..Default::default()
    }).await
}
```

## 游標分頁

`cursor_paginate(15)` 比較排序欄位的值，並多讀一列判斷後續資料。下次請求傳入回傳的 `next_cursor` 或 `prev_cursor`。Query Builder 需按已查詢、非空的純量欄位排序，最後使用唯一欄位區分同值資料。Eloquent 在未宣告排序時使用主鍵。分組查詢、聯合查詢和原始排序運算式使用偏移分頁。

## 手動建立分頁器 {#manually-creating-a-paginator}

資料已在記憶體時，將目前頁面的切片與切片前的總數傳給 `LengthAwarePaginator::new`。使用 `Paginator::new` 時，有下一頁就額外傳入一筆記錄。`CursorPaginator::new` 同樣接收含預讀記錄的排序切片，還需要一個回呼擷取邊界記錄的具名排序鍵。請求上一頁時，依反向查詢順序傳入記錄，分頁器會恢復顯示順序。

```rust
use openrust::{
    database::Value,
    pagination::{Cursor, CursorPaginator, PaginationResult, PaginatorOptions},
};

fn manual_cursor_page(
    ordered: Vec<i64>,
    current: Option<&Cursor>,
) -> PaginationResult<CursorPaginator<String>> {
    let mut page = CursorPaginator::new(
        ordered,
        2,
        current,
        PaginatorOptions {
            cursor_name: Some("position"),
            ..Default::default()
        },
        |id| [("id", Value::Integer(*id))],
    )?;
    page.with_query_string();
    Ok(page.through(|id| id.to_string()))
}
```

## 分頁器實例方法 {#paginator-instance-methods}

用 `count()`、`on_first_page()` 和 `on_last_page()` 取得頁面狀態。`through` 消耗原分頁器並轉換記錄型別，同時保留分頁中繼資料。遊標仍使用轉換前的排序鍵。

用 `with_path` 修改連結路徑，`set_page_name` 或 `set_cursor_name` 修改查詢參數名稱，`with_query_string` 保留請求篩選條件，`fragment` 新增錨點。`get_options()` 傳回目前生效的連結選項。

同一回應包含多個分頁器時，為各分頁器設定獨立的查詢參數。例如，分別將 `PageOptions.page_name` 設為 `articles` 和 `comments`。保留請求查詢時，各分頁器產生的連結只替換自己的頁碼，保留另一個頁碼與篩選條件。用 `appends` 新增指定篩選條件，用 `fragment` 指向對應的結果區域。

## 關聯模型分頁 {#paginating-related-models}

在 `HasMany` 或 `MorphMany` 描述符上呼叫 `paginate`、`simple_paginate` 或 `cursor_paginate`，傳入父模型與每頁筆數。帶 `With` 的變體接受與 Eloquent 查詢相同的選項。查詢保留父鍵、適用的多型類別、已設定的篩選及模型作用域。父模型沒有鍵時回傳空頁。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasMany}},
    pagination::{LengthAwarePaginator, PageOptions},
};

async fn related_comments<P: Model, R: Model>(
    article: &P,
    comments: &HasMany<P, R>,
) -> DatabaseResult<LengthAwarePaginator<R>> {
    comments.paginate_with(article, PageOptions {
        per_page: Some(15),
        page_name: "comments",
        ..Default::default()
    }).await
}
```

`HasOne`、`MorphOne`、`BelongsTo` 和 `MorphTo` 同樣提供三種分頁。單筆關聯保留既有的擇一條件。分頁回傳已持久化的紀錄，缺少反向參照時回傳保留請求分頁中繼資料的空頁。`MorphTo` 依多型類型選擇已註冊的目標，回傳由 `MorphTarget` 組成的分頁。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::{MorphTarget, MorphTo}}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn subject_page<P: Model>(
    child: &P,
    relation: &MorphTo<P>,
) -> DatabaseResult<CursorPaginator<MorphTarget>> {
    relation.cursor_paginate_with(child, CursorOptions {
        per_page: Some(15),
        cursor_name: "subjects",
        ..Default::default()
    }).await
}
```

`HasManyThrough` 和 `HasOneThrough` 也支援這三種分頁方法。查詢使用中間資料表連接，預設排除已軟刪除的中間模型。呼叫 `with_trashed_parents()` 可包含這些模型。預設只選取關聯模型的欄位。連接的資料表有同名欄位時，自訂排序應加上資料表名稱。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::HasManyThrough}},
    pagination::{CursorOptions, CursorPaginator},
};

async fn through_page<P: Model, I: Model, R: Model>(
    parent: &P,
    relation: &HasManyThrough<P, I, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation.cursor_paginate_with(parent, CursorOptions {
        per_page: Some(15),
        cursor_name: "reactions",
        ..Default::default()
    }).await
}
```

`BelongsToMany` 和 `MorphToMany` 支援相同的分頁方法。目前頁面的每個模型都會載入 Pivot，支援自訂模型和存取器。游標分頁會查詢 `order_by_pivot` 使用的欄位以產生游標。透過 `with_pivot` 指定要在 Pivot 上公開的額外欄位。排序欄位的組合必須唯一識別每一列。

```rust
use openrust::{
    database::{DatabaseResult, eloquent::{Model, relations::BelongsToMany}},
    pagination::CursorPaginator,
};

async fn pivot_page<P: Model, R: Model>(
    parent: &P,
    relation: BelongsToMany<P, R>,
) -> DatabaseResult<CursorPaginator<R>> {
    relation
        .with_pivot(["label"])
        .order_by_pivot("weight", "asc")
        .order_by_pivot("label_id", "asc")
        .cursor_paginate(parent, 15)
        .await
}
```

### Eloquent 分頁的集合鍵 {#collection-keys-on-eloquent-pages}

Eloquent 在水合目前頁面並載入關聯後呼叫 `Model::new_collection`。一般和簡易分頁會保留自訂鍵，可透過 `get_by_key` 尋找記錄。遊標分頁依位置重新編號為清單。`items()` 都支援依序遍歷。

```rust
use openrust::{
    database::eloquent::Model,
    pagination::LengthAwarePaginator,
};

fn find_on_page<'a, M: Model>(
    page: &'a LengthAwarePaginator<M>,
    key: &str,
) -> Option<&'a M> {
    page.get_by_key(key)
}
```

### 載入目前頁面的多型關聯 {#loading-polymorphic-relations-on-the-current-page}

在 Eloquent 分頁器上呼叫 `load_morph` 或 `load_morph_count`，可為指定的多型目標載入下一層關聯或關聯計數。這些方法只處理目前頁面，並傳回同一個分頁器供鏈式呼叫。

```rust
use openrust::{
    database::{
        DatabaseResult,
        eloquent::{Model, relations::{ExistenceRelation, MorphTo}},
    },
    pagination::LengthAwarePaginator,
};

async fn load_page_subjects<M: Model, T: Model>(
    page: &mut LengthAwarePaginator<M>,
    subject: &MorphTo<M>,
    comments: &impl ExistenceRelation<Parent = T>,
) -> DatabaseResult<()> {
    page.load_morph("subject", subject, "article", "comments", comments)
        .await?
        .load_morph_count("subject", subject, "article", "comments", comments)
        .await?;
    Ok(())
}
```

## 顯示分頁結果 {#displaying-pagination-results}

用分頁器的 `links()` 建立 Blade 視圖。預設 Tailwind 視圖為 `paginate` 顯示記錄範圍和頁碼連結。`simple_paginate` 和 `cursor_paginate` 顯示上一頁和下一頁。結果只有一頁時不輸出導覽。`on_each_side` 可調整頁碼視窗。

```rust
use openrust::{
    contracts::view::View as _,
    pagination::LengthAwarePaginator,
    view::ViewResult,
};

fn pagination_navigation<T>(page: &LengthAwarePaginator<T>) -> ViewResult<String> {
    page.links()?.render()
}
```

使用 `links_with("articles.navigation", data)` 指定自訂視圖，也可以在 `resources/views/vendor/pagination/` 下放置 `tailwind.blade.html` 或 `simple-tailwind.blade.html` 覆寫預設視圖。自訂視圖會收到 `paginator` 中繼資料，含總數的分頁還會傳入頁碼 `elements`。在 `lang/{locale}/pagination.json` 設定 `previous` 和 `next`，即可本地化導覽和 JSON 連結標籤。

## 自訂分頁視圖 {#customizing-the-pagination-view}

在應用程式服務提供者的 `boot` 中，用 `Paginator::default_view("articles.navigation")` 指定帶頁碼的視圖，用 `Paginator::default_simple_view("articles.simple-navigation")` 指定簡易和游標分頁的視圖。設定只套用於目前的應用程式。呼叫 `links_with` 時指定的視圖優先。

用 `Paginator::use_bootstrap_five()`、`use_bootstrap_four()` 或 `use_bootstrap_three()` 同時設定兩類視圖。`use_bootstrap()` 選擇 Bootstrap 4，`use_tailwind()` 恢復 Tailwind 預設視圖。應用程式需要載入對應的 CSS。

```rust
use openrust::{
    foundation::Application,
    http::HttpFuture,
    pagination::Paginator,
    support::{ProviderResult, ServiceProvider},
};

struct AppServiceProvider;

impl ServiceProvider for AppServiceProvider {
    fn boot(&mut self, _: &Application) -> HttpFuture<'_, ProviderResult> {
        Box::pin(async {
            Paginator::use_bootstrap_five()?;
            Ok(())
        })
    }
}
```

使用 `pagination::semantic-ui` 可指定 Semantic UI 頁碼視圖。命令將九個內建範本發佈到 `resources/views/vendor/pagination`。修改已發佈的範本即可覆寫內建視圖。加上 `--force` 可覆寫現有檔案。

```bash
./artisan vendor:publish --tag=laravel-pagination
```

## 分頁視窗和 JSON {#pagination-windows-and-json}

用 `UrlWindow::make(&page)` 取得根據 `on_each_side` 算出的開頭、滑動視窗和結尾連結。每段依序儲存 `(頁碼, Url)`。`link_collection()` 傳回攤平的頁碼、省略號，以及已翻譯的上一頁和下一頁標籤。

```rust
use openrust::pagination::{LengthAwarePaginator, UrlWindow, UrlWindowLinks};

fn numbered_links<T>(page: &LengthAwarePaginator<T>) -> UrlWindowLinks {
    UrlWindow::make(page)
}
```

`to_array()` 傳回 JSON 物件。`to_json()` 和 `to_pretty_json()` 編碼相同的中繼資料與記錄。手動呼叫 `LengthAwarePaginator::new` 可傳入頁容量零，`last_page()` 依每頁一筆計算。查詢分頁仍要求頁容量為正。

## JSON 和分頁連結

當分頁項目實作 Serde 的 `Serialize` 時，可直接序列化分頁器。用 `map` 將資料庫列或 Eloquent 模型轉換為回應項目，保留分頁中繼資料。`with_query_string` 保留篩選條件，`appends` 加入選定參數。連結使用請求相對路徑。

```rust
use openrust::database::{Db, DatabaseResult};

async fn article_ids(table: &str) -> DatabaseResult<impl serde::Serialize> {
    let mut page = Db::table(table)?
        .order_by("id", "asc")
        .paginate(15)
        .await?;
    page.with_query_string();
    Ok(page.map(|row| match row.get("id") {
        Some(openrust::database::Value::Integer(id)) => Some(id.to_string()),
        Some(openrust::database::Value::Unsigned(id)) => Some(id.to_string()),
        _ => None,
    }))
}
```

## 自訂分頁解析器 {#customizing-pagination-resolvers}

在服務提供者的 `boot` 中登記 `Paginator::current_page_resolver`、`current_path_resolver` 和 `query_string_resolver`。`LengthAwarePaginator` 共用這些設定。游標分頁使用 `CursorPaginator::current_cursor_resolver`。頁碼和游標回呼接收所設定的參數名；游標回呼傳回 `None` 時從第一頁開始。

```rust
Paginator::current_page_resolver(|name| if name == "entries" { 2 } else { 1 })?;
Paginator::current_path_resolver(|| "/articles".to_owned())?;
Paginator::query_string_resolver(|| vec![("category".into(), "rust".into())])?;
```

解析器作用於目前應用中的手工分頁、查詢建構器、模型及關聯分頁。明確傳入的頁碼、游標和路徑優先。`with_query_string()` 在呼叫時讀取查詢解析器，同名值覆蓋先前 `appends` 的參數，排除分頁位置參數。查詢回呼傳回字串鍵值對或 `None`。`None` 和空清單都會停用請求查詢參數的附加；`resolve_query_string` 區分兩者，明確傳回 `None` 時不再使用後備值。巢狀值由應用編碼。

`resolve_current_page(name, default)`、`resolve_current_path(default)`、`resolve_query_string(default)` 及 `CursorPaginator::resolve_current_cursor(name, default)` 提供相同的解析入口。應用請求之外沒有對應回呼時傳回傳入的預設值；預設請求來源中缺少或無效頁碼使用一，缺少或無效游標從頭開始。預設連結路徑仍為請求相對路徑。

`Paginator::view_factory_resolver` 接收傳回 `ViewResult<Arc<openrust::view::Factory>>` 的回呼。`Paginator::view_factory`、`LengthAwarePaginator::view_factory` 及 `CursorPaginator::view_factory` 傳回共用工廠，分頁視圖渲染也使用該工廠。回呼要求 `Send + Sync + 'static`，在目前應用作用域中登記；沒有可用應用容器時登記傳回錯誤。
