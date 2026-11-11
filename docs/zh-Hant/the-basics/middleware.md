# 中介軟體

在 `bootstrap/app.rs` 的 `withMiddleware` 閉包中設定全域中介軟體，使用 `append` 加到末端，或用 `prepend` 加到前端。用 `RouteGroup::middleware` 為路由群組設定中介軟體，用 `Route::middleware` 為單一路由設定中介軟體。

全域中介軟體包覆路由比對流程，也能讀取 404 和 405 回應。符合路由後，先執行外層路由群組的中介軟體，再執行內層路由群組和單一路由的中介軟體。每一層都可以呼叫 `next.run(request).await` 繼續執行並讀取傳回的回應，也可以直接傳回回應，略過其餘中介軟體和路由處理器。

## 命名群組與優先順序

在 `withMiddleware` 中用 `group` 註冊命名群組，用 `alias` 註冊單一執行個體。群組成員可以是 `MiddlewareEntry::Instance`，也可以用 `MiddlewareEntry::named` 參照另一個群組或別名。透過 `middleware_name` 將群組名稱或別名附加到路由或路由群組。名稱不存在或參照形成循環時，應用程式會在開始監聽前啟動失敗。

用 `priority` 傳入依執行順序排列的具體中介軟體型別 ID，例如 `TypeId::of::<Authenticate>()`。框架先展開群組，再將順序顛倒的高優先項目移到它前面遇到的低優先項目之前。未列入優先順序的項目保留彼此的相對順序。同一個執行個體經多次參照時只執行一次。優先順序作用於路由中介軟體鏈。全域中介軟體依設定順序在路由鏈外層執行。

在路由或路由群組上用 `withoutMiddleware("name")` 排除指定的別名或群組。排除也適用於透過巢狀群組引用的同一實例。全域中介軟體仍會執行。

## 可終止中介軟體

實作 `TerminableMiddleware`，在 HTTP 傳輸層釋放回應本文後取得請求和回應的中繼資料。在 `withMiddleware` 中用 `appendTerminable`、`prependTerminable` 或 `aliasTerminable` 註冊。向群組加入具體實例時用 `MiddlewareEntry::terminable`。

核心先按解析後的順序呼叫相符路由的終止回呼，再呼叫全域回呼。請求處理與終止階段使用同一個已註冊實例。傳輸層消費串流本文期間，終止回呼會等待。用戶端斷線且傳輸層釋放本文時也會執行回呼。服務正常關閉時會等待未完成的回呼。

直接呼叫 `Kernel::handle` 的應用應在傳輸結束後，用請求和回應組件呼叫 `Kernel::terminate`。內建 HTTP 服務會自動處理。
