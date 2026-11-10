# 路由

在 `routes/web.rs` 声明 Web 路由。应用启动时通过 `openrust::routing::Router` 注册这些路由和 `/up` 健康检查。需要读取请求的处理器接收可变的 `Request` 并返回异步 HTTP 结果。框架调用 Web 路由模块的 `register()` 时绑定当前应用的 Router。导入 `openrust::support::facades::Route` 后，可以用 `Route::get("/", || view("welcome"))` 声明命名视图。

API 路由的默认 URL 前缀为 `/api`。用 `Routing::default().api(routes::api::register).apiPrefix("v1")` 可以挂到 `/v1`，传入空字符串则省略前缀。`api` 中间件组仍会应用。应用的异常渲染策略由 `withExceptions` 单独配置。

`{id}` 表示必填参数。在末尾参数上加 `?` 即可省略它，例如 `/profiles/{id?}/{tab?}` 或 `/report/{name}.{format?}`。使用 `request.route("id")` 读取匹配值。省略参数时返回 `None`，也可以用路由的 `defaults` 提供字符串默认值。参数后面还有必填参数或固定文本时，该参数仍须提供。

用 `Route::r#where` 指定正则表达式，也可使用 `where_number`、`where_alpha`、`where_alpha_numeric`、`where_uuid`、`where_ulid` 或 `where_in`。在注册路由前调用 `Router::pattern`，可给后续路由的同名参数设置约束。单条路由上的约束优先。`where_in` 将值拼成正则的备选分支，正则元字符保留原义。OpenRust 使用 Rust 的 `regex` 语法，PCRE 的环视和反向引用会在注册时被拒绝。

匹配时按 UTF-8 对路径执行百分号解码，并忽略查询字符串。解码结果无效时返回 HTTP 400。匹配路由和检查可用方法时都应用约束。例如 `/profiles/{id?}` 是限制数字 ID 的 GET 路由时，`POST /profiles/7` 返回 405 和 `Allow`。如果没有其他路由匹配该路径，`POST /profiles/alice` 返回 404。

`Router::get` 同时注册 GET 和 HEAD。HEAD 保留响应状态和响应头，发送空响应体。未知路径返回 404。路径存在但方法不受支持时返回 405，并附带 `Allow` 响应头。路径存在且未显式注册 OPTIONS 时，路由器返回 200 并列出允许的方法。同一方法有多条路由可匹配时，使用最先注册的匹配项。

## 挂载静态目录

应用骨架先挂载网站公共目录，再挂载公共存储磁盘。目录和 URL 前缀见[公共文件配置](../getting-started/configuration.md#public-files)。

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

相对目录从应用根目录解析。例如，`public/css/app.css` 对应 `/css/app.css`。如需关闭公共磁盘挂载，移除 `routes/web.rs` 中的 `Route::staticDisk("public")?` 调用。网站目录默认挂到 `/`，公共磁盘默认挂到 `/storage`。两个挂载都能响应同一个 URL 时，框架按声明顺序返回第一个命中的文件。按骨架的顺序，请求 `/storage/avatar.png` 时，`public/storage/avatar.png` 优先于 `storage/app/public/avatar.png`。公共磁盘无需创建 `public/storage` 符号链接。应用路由优先，只有未匹配任何应用路由的 GET 和 HEAD 请求才会查找文件。文件不存在或请求指向目录时返回 404，符号链接无法访问挂载目录外的文件。

静态文件有可用的修改时间时，响应会携带 `Last-Modified`。GET 或 HEAD 请求可以把该值放入 `If-Modified-Since` 重新验证文件。文件未修改时返回 `304 Not Modified`，响应体为空。

## 健康检查

`bootstrap/app.rs` 的 `health("/up")` 注册健康端点。普通请求返回包含“Application up”的 HTML 页面，期望 JSON 的请求返回 `{"status":"up"}`。两者均返回 200，HEAD 不发送正文。HTML 标题使用经过转义的应用名称。

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

这表示 HTTP 引导已成功。诊断事件监听器及其失败响应等待事件分发器实现。健康页面使用本地 HTML，未使用 Laravel 的 Blade 视图和外部样式资源。
