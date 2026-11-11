# 文件存储

## 配置

用 `FILESYSTEM_DISK` 设置默认磁盘，在 `config/filesystems.rs` 配置具名磁盘。`local` 与 `public` 都使用本地驱动，各自有独立根目录。磁盘管理器在当前应用内解析并缓存具名磁盘。

### 本地驱动

默认 `local` 磁盘的目录是 `storage/app/private`。操作路径相对于磁盘根目录。绝对路径、穿越目录及保留的临时文件名会被拒绝。Unix 驱动通过目录句柄打开文件，遍历时拒绝符号链接，也拒绝读取具有多个硬链接的文件。

### 公共磁盘

`public` 磁盘配置在 `config/filesystems.rs`。默认本地根目录为 `storage/app/public`，URL 为应用 URL 加上 `/storage`。使用 `PUBLIC_STORAGE_ROOT` 修改目录，使用 `PUBLIC_STORAGE_URL` 修改公开 URL。相对文件路径从应用根目录解析。

Artisan 可以创建或移除 `filesystems.links` 配置的链接。默认链接为 `public/storage`，指向公共磁盘目录。使用 `--force` 重建已有符号链接。已有普通文件与目录会保留。

```sh
./artisan storage:link --relative
./artisan storage:unlink
```

#### 提供本地文件（OpenRust 扩展）

默认 `routes/web.rs` 按以下顺序挂载：

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

`staticPublic` 读取网站目录 `app.public_path`，默认为 `public`，可用 `PUBLIC_PATH` 覆盖。`staticDisk` 读取本地磁盘根目录，从磁盘的 `url` 提取完整挂载路径，没有单独的 `url_prefix` 配置。例如，`PUBLIC_STORAGE_URL=/storage/tmp/` 将 `storage/app/public/photo.jpg` 暴露在 `/storage/tmp/photo.jpg`。绝对 URL 必须与 `app.url` 同源；协议、主机或有效端口不同会在 HTTP 引导时被拒绝。本地文件服务只挂载本地磁盘。

OpenRust HTTP 服务也可以直接提供公共磁盘文件。在 Unix 上，磁盘挂载只返回公开文件，并要求各级目录允许遍历。符号链接、写入临时文件及具有多个硬链接的文件会被拒绝。

应用路由优先于静态文件。静态挂载按声明顺序查找，文件不存在时继续查找后续挂载。默认顺序下，同为 `/storage/photo.jpg` 时，`public/storage/photo.jpg` 优先于 `storage/app/public/photo.jpg`。服务支持 GET、HEAD、MIME 类型、`Last-Modified` 和 `If-Modified-Since`。不列出目录，拒绝路径穿越及逃逸挂载根目录的符号链接。缺失文件使用应用的正常 404 策略。

参阅[路由](../the-basics/routing.md#挂载静态目录)和[公共文件配置](../getting-started/configuration.md#公共文件)。

## 获取磁盘实例

调用 `Storage::disk(Some("public"))` 从当前应用选择具名磁盘。`Storage::disk(None)`、`Storage::put` 和 `Storage::get` 使用默认磁盘。下例运行在请求或命令作用域内。

```rust
use openrust::{filesystem::FilesystemResult, support::facades::Storage};

pub async fn store_note() -> FilesystemResult<Option<Vec<u8>>> {
    if !Storage::put("notes/example.txt", b"Hello").await? {
        return Ok(None);
    }
    let disk = Storage::disk(Some("local"))?;
    disk.get("notes/example.txt").await
}
```

使用 `Storage::build(LocalDiskConfig)` 按需创建本地磁盘。具名磁盘缓存属于单个应用。

## 读取文件

`get` 返回文件内容。读取大文件时可通过 `readStream` 获取 Tokio 文件句柄。默认 `throw = false` 时读取失败返回 `None`，写入失败返回 `false`。设置 `throw = true` 可返回错误，设置 `report = true` 可记录默认策略处理的错误。无效路径始终返回错误。

## 存储文件

`writeStream` 从异步读取器复制数据。本地驱动先写同目录临时文件，流写完后通过重命名发布。源失败或发布前取消会保留原文件。重命名操作已经发出后，即使调用方取消，完整写入仍可能发布。

### 文件可见性

Unix 上公开和私有文件权限分别是 `0644` 和 `0600`。新子目录使用磁盘的目录可见性配置。可用 `WriteOptions` 覆盖单次写入的可见性，通过 `getVisibility` 和 `setVisibility` 管理已有文件。Windows 写入使用文件系统 ACL，显式可见性变更目前支持 Unix。
