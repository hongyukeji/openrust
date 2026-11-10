# 文件存储

## 公共磁盘

`public` 磁盘配置在 `config/filesystems.rs`。默认本地根目录为 `storage/app/public`，URL 为应用 URL 加上 `/storage`。使用 `PUBLIC_STORAGE_ROOT` 修改目录，使用 `PUBLIC_STORAGE_URL` 修改公开 URL。相对文件路径从应用根目录解析。

### 提供本地文件（OpenRust 扩展）

默认 `routes/web.rs` 按以下顺序挂载：

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

`staticPublic` 读取网站目录 `app.public_path`，默认为 `public`，可用 `APP_PUBLIC_PATH` 覆盖。`staticDisk` 读取本地磁盘根目录，从磁盘的 `url` 提取完整挂载路径，没有单独的 `url_prefix` 配置。例如，`PUBLIC_STORAGE_URL=/storage/tmp/` 将 `storage/app/public/photo.jpg` 暴露在 `/storage/tmp/photo.jpg`。绝对 URL 必须与 `app.url` 同源；协议、主机或有效端口不同会在 HTTP 引导时被拒绝。S3 等远程磁盘不能通过这个本地文件服务挂载。

Laravel 通常通过 `storage:link` 创建符号链接来公开公共磁盘。OpenRust 的挂载直接提供本地文件，无需该链接。`storage:link` 命令及通用文件系统 API 不属于本次已交付范围。

应用路由优先于静态文件。静态挂载按声明顺序查找，文件不存在时继续查找后续挂载。默认顺序下，同为 `/storage/photo.jpg` 时，`public/storage/photo.jpg` 优先于 `storage/app/public/photo.jpg`。服务支持 GET、HEAD、MIME 类型、`Last-Modified` 和 `If-Modified-Since`。不列出目录，拒绝路径穿越及逃逸挂载根目录的符号链接。缺失文件使用应用的正常 404 策略。

参阅[路由](../the-basics/routing.md#挂载静态目录)和[公共文件配置](../getting-started/configuration.md#公共文件)。
