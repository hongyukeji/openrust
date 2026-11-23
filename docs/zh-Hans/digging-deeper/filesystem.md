# 文件存储

## 配置

用 `FILESYSTEM_DISK` 设置默认磁盘，在 `config/filesystems.rs` 配置具名磁盘。`local` 与 `public` 都使用本地驱动，各自有独立根目录。磁盘管理器在当前应用内解析并缓存具名磁盘。

### 本地驱动

默认 `local` 磁盘的目录是 `storage/app/private`。使用 `FILESYSTEM_LOCAL_ROOT` 覆盖此目录；相对路径从应用根目录解析，绝对路径直接使用。未设置时，默认目录跟随 `APP_STORAGE_PATH` 配置的存储根目录。操作路径相对于磁盘根目录。绝对路径、穿越目录及保留的临时文件名会被拒绝。Unix 驱动通过目录句柄打开文件，遍历时拒绝符号链接，也拒绝读取具有多个硬链接的文件。 读写路径中间重复的 `/` 会合并，例如 `avatars///photo.png` 与 `avatars/photo.png` 访问同一个文件或对象键。`path`、`url` 和临时 URL 构造器保留传入的路径文本，只按各自职责添加前缀或签名。

### 公共磁盘

`public` 磁盘配置在 `config/filesystems.rs`。默认本地根目录为 `storage/app/public`，URL 为应用 URL 加上 `/storage`。使用 `FILESYSTEM_PUBLIC_ROOT` 修改目录，使用 `FILESYSTEM_PUBLIC_URL` 修改公开 URL。相对文件路径从应用根目录解析。

Artisan 可以创建或移除 `filesystems.links` 配置的链接。默认链接为 `public/storage`，指向公共磁盘目录。使用 `--force` 重建已有符号链接。已有普通文件与目录会保留。

```sh
./artisan storage:link --relative
./artisan storage:unlink
```

#### 提供本地文件（OpenRust 扩展）

默认 `routes/web.rs` 按以下顺序挂载：

```rust
Route::static_disk("web")?;
Route::static_disk("public")?;
```

`web` 磁盘描述网站目录，根目录默认是 `public`，URL 默认是 `app.url`。在 `config/filesystems.rs` 中通过 `FILESYSTEM_WEB_ROOT` 和 `FILESYSTEM_WEB_URL` 覆盖默认值。`FILESYSTEM_WEB_ROOT` 优先指定网站磁盘根目录；未设置时使用 `APP_PUBLIC_PATH`（默认 `public`）。相对路径基于应用根目录，绝对路径直接使用。空字符串或全空白值会导致配置加载失败。应用使用 `web.root` 作为 `public_path()`；启动前调用 `use_public_path()` 会覆盖目录并同步 `web.root`。两个挂载统一使用 `static_disk`，读取本地磁盘根目录，并从 `url` 提取完整挂载路径。例如，`FILESYSTEM_PUBLIC_URL=/storage/tmp/` 将 `storage/app/public/photo.jpg` 暴露在 `/storage/tmp/photo.jpg`。绝对 URL 必须与 `app.url` 同源，协议、主机或有效端口不同会导致 HTTP 引导失败。

两个磁盘挂载在 Unix 上都要求文件公开且各级目录可遍历，并拒绝符号链接、写入临时文件及具有多个硬链接的文件。如果 `public/storage` 是符号链接，`web` 磁盘会跳过它，由 `public` 磁盘直接从其配置的根目录提供文件。

应用路由优先于静态文件。静态挂载按声明顺序查找，文件不存在时继续查找后续挂载。默认顺序下，同为 `/storage/photo.jpg` 时，`public/storage/photo.jpg` 优先于 `storage/app/public/photo.jpg`。服务支持 GET、HEAD、MIME 类型、`Last-Modified` 和 `If-Modified-Since`。不列出目录，拒绝路径穿越及逃逸挂载根目录的符号链接。缺失文件使用应用的正常 404 策略。

参阅[路由](../the-basics/routing.md#挂载静态目录)和[公共文件配置](../getting-started/configuration.md#公共文件)。

### 兼容 Amazon S3 的文件系统

为 `s3` 磁盘设置 `AWS_BUCKET` 和 `AWS_DEFAULT_REGION`。如需显式凭据，同时设置 `AWS_ACCESS_KEY_ID` 和 `AWS_SECRET_ACCESS_KEY`。临时凭据还需 `AWS_SESSION_TOKEN`。省略两项密钥时，由 AWS SDK 的凭据链解析。兼容服务可设置 `AWS_ENDPOINT`，服务使用路径式存储桶 URL 时启用 `AWS_USE_PATH_STYLE_ENDPOINT`。

在应用请求或命令中按名称选取磁盘。设置 `FILESYSTEM_DISK=s3` 可作为默认磁盘。磁盘配置中的可选 `root` 用作对象键前缀，操作路径相对于磁盘。

```rust
use openrust::{filesystem::FilesystemResult, support::facades::Storage};

pub async fn store_report() -> FilesystemResult<Option<Vec<u8>>> {
    let disk = Storage::disk(Some("s3"))?;
    if !disk.put("reports/summary.txt", b"Ready").await? {
        return Ok(None);
    }
    disk.get("reports/summary.txt").await
}
```

`read_stream` 为本地磁盘和 S3 磁盘提供相同的异步读取接口。S3 每段写入 8 MiB，完成分段上传后发布对象。当前最多上传 10,000 段，单对象约 78 GiB。上传中断后会尽量取消未完成的上传。连接失败后远端可能已经完成写入，此时 `throw = true` 会返回 `PublishOutcomeUnknown`。重试前可读取目标核对状态。

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

`get` 返回文件内容。读取大文件时可通过 `read_stream` 获取异步读取器。默认 `throw = false` 时读取失败返回 `None`，写入失败返回 `false`。设置 `throw = true` 可返回错误，设置 `report = true` 可记录默认策略处理的错误。无效路径始终返回错误。

### 下载文件

用 `download` 生成附件响应，或用 `response` 在浏览器内显示。两者都流式读取文件，可传入下载名称及响应头。适配器补充内容类型、已知长度及内容处置头。下载私有文件前，由应用授权。处理器返回 `HttpError` 时，文件不存在会转换为 HTTP 404。

```rust
use openrust::{
    http::{HeaderMap, HttpError, Response},
    support::facades::Storage,
};

pub async fn download_report() -> Result<Response, HttpError> {
    Ok(Storage::disk(Some("public"))?
        .download("reports/summary.txt", Some("summary.txt"), HeaderMap::new())
        .await?)
}
```

### 文件 URL

`Storage::url` 使用默认磁盘。具名磁盘优先使用配置的 `url`，本地磁盘未配置时使用 `/storage`。本地 URL 请使用适合 URL 的文件名。S3 可通过 `AWS_URL` 指定公共基址。未指定时由 AWS SDK 解析存储桶地址，适配器追加编码后的对象键及其配置前缀。文件是否可访问由磁盘挂载或存储桶控制。

### 文件元数据 {#file-metadata}

用 `size` 取得本地磁盘文件的字节数，用 `last_modified` 取得 `SystemTime`。两者在元数据不可用时返回错误。通过 `Storage` 调用时使用默认磁盘。

```rust
use openrust::filesystem::{FilesystemAdapter, FilesystemResult};

async fn report_metadata(disk: &FilesystemAdapter) -> FilesystemResult<(u64, Option<String>)> {
    let path = "reports/today.txt";
    Ok((disk.size(path).await?, disk.mime_type(path).await?))
}
```

用 `mime_type` 检查内容签名，并回退到扩展名推断或 `application/octet-stream`。用 `checksum` 流式计算 MD5，或用 `checksum_with::<D>` 选用 Rust 摘要算法。这两项操作在关闭 `throw` 时将磁盘错误回退为 `None`。无效路径始终返回错误。

## 存储文件

`write_stream` 从异步读取器复制数据。本地驱动先写同目录临时文件，流写完后通过重命名发布。源失败或发布前取消会保留原文件。重命名操作已经发出后，即使调用方取消，完整写入仍可能发布。

### 文件上传

通过 `Request::file` 获取 multipart 上传。可调用上传对象的 `store` 或 `store_as`，也可用磁盘的 `put_file` 或 `put_file_as`。`store_publicly` 和 `store_publicly_as` 请求公开可见性。自动文件名使用 40 个随机字符，识别出类型时附加由内容推断的扩展名。客户端文件名与媒体类型属于元数据，保存前应执行应用的上传验证。

```rust
use openrust::{
    http::{HttpError, Request, Response, StatusCode},
    support::facades::Storage,
};

pub async fn store_attachment(request: &mut Request) -> Result<Response, HttpError> {
    let attachment = request.file("attachment").await?.ok_or(HttpError::InvalidRequest)?;
    let disk = Storage::disk(Some("public"))?;
    let path = attachment.store_publicly("attachments", Some("public")).await?
        .ok_or_else(|| HttpError::Internal {
            source: std::io::Error::other("Could not store attachment").into(),
        })?;
    Response::json(StatusCode::CREATED, disk.url(&path)?)
}
```

### 文件可见性

Unix 上公开和私有文件权限分别是 `0644` 和 `0600`。新子目录使用磁盘的目录可见性配置。可用 `WriteOptions` 覆盖单次写入的可见性，通过 `get_visibility` 和 `set_visibility` 管理已有文件。Windows 写入使用文件系统 ACL，显式可见性变更目前支持 Unix。

S3 可见性使用对象 ACL。未指定可见性时，写入沿用存储桶的访问控制。显式可见性需要存储桶支持 ACL，确认设置结果还需要读取 ACL 的权限。实际匿名可访问性取决于存储桶策略及服务是否执行 ACL。

#### 自定义 Unix 权限 {#local-permissions}

具名本地磁盘可以覆盖每种可见性对应的 Unix 权限。可设置 `permissions.file.public`、`permissions.file.private`、`permissions.dir.public` 或 `permissions.dir.private`。未指定的项沿用默认值。在 Rust 配置中使用 `0o664` 等八进制数值。

```rust
use openrust::filesystem::{FilesystemResult, LocalPermissions};

fn permissions() -> FilesystemResult<LocalPermissions> {
    LocalPermissions::new(0o664, 0o640, 0o775, 0o750)
}
```

按需磁盘可用 `Storage::build_with_permissions` 同时传入 `LocalDiskConfig` 和权限表。新建文件和目录采用配置的权限，`set_visibility` 会给文件应用对应模式。公开磁盘挂载还要求 Unix 的公共读权限和目录遍历权限，并遵守映射为 `private` 的模式。自定义 Unix 模式适用于 Unix。

## 临时文件 URL

用 `disk.temporary_url(path, expiration).await?` 生成下载链接，或用 `disk.temporary_upload_url(path, expiration).await?` 生成上传链接。上传时向返回的 `url` 发送请求，并携带返回的 `headers`。本地磁盘须在磁盘配置中开启 `serve`，路由使用该磁盘 `url` 的路径，省略时使用 `/storage`。本地上传链接接收 PUT。S3 适配器按配置的桶端点签名，客户端需要能够访问该端点。

### S3 临时 URL 基地址 {#s3-temporary-url-origin}

在 S3 磁盘设置 `temporary_url`，或用 `AWS_TEMPORARY_URL` 配置默认 S3 磁盘。它替换 GET 和 PUT 链接的协议、主机和端口，保留已签名的路径和查询串。代理转发到 S3 端点时需要使用签名中的原始 Host、路径和查询串。

### S3 请求选项 {#s3-request-parameters}

用 `temporary_url_with` 签入下载响应头或版本 ID。用 `temporary_upload_url_with` 签入上传内容头和对象元数据，并在 PUT 请求中携带返回的请求头。存储桶和对象路径由磁盘提供。

```rust
use std::time::{Duration, SystemTime};

use openrust::{
    filesystem::{FilesystemResult, TemporaryUrlOptions},
    support::facades::Storage,
};

async fn report_url() -> FilesystemResult<String> {
    Storage::disk(Some("s3"))?
        .temporary_url_with(
            "reports/current.csv",
            SystemTime::now() + Duration::from_secs(300),
            TemporaryUrlOptions {
                response_content_type: Some("text/csv".into()),
                response_content_disposition: Some("attachment; filename=report.csv".into()),
                ..Default::default()
            },
        )
        .await
}
```

如需更多 S3 选项，在指定磁盘上调用 `temporary_download_request` 或 `temporary_upload_request`，通过闭包配置 AWS SDK 请求构造器。两种方法都返回 URL 和签名请求头，客户端发送请求时需携带这些头。磁盘在应用选项后设置存储桶和带前缀的对象键。

```rust
use std::time::{Duration, SystemTime};

use openrust::{
    filesystem::{FilesystemResult, TemporaryDownloadUrl},
    support::facades::Storage,
};

async fn report_preview() -> FilesystemResult<TemporaryDownloadUrl> {
    Storage::disk(Some("s3"))?
        .temporary_download_request(
            "reports/current.csv",
            SystemTime::now() + Duration::from_secs(300),
            |request| request.range("bytes=0-1023"),
        )
        .await
}
```

### 自定义本地磁盘签名回调 {#customizing-temporary-urls}

在服务提供者启动时，对本地磁盘注册 `build_temporary_urls_using` 或 `build_temporary_upload_urls_using`。回调接收已校验的路径、过期时间和类型化选项。上传回调返回包含 URL 和所需请求头的 `TemporaryUploadUrl`。

## 目录 {#directories}

对本地磁盘使用 `files` 和 `directories` 列出直接子项，使用 `all_files` 和 `all_directories` 包含后代。传入 `None` 表示磁盘根目录。结果为以 `/` 分隔的磁盘相对路径。文件列表按路径排序，目录缺失时返回空列表。

```rust
use openrust::filesystem::{FilesystemAdapter, FilesystemResult};

async fn daily_reports(disk: &FilesystemAdapter) -> FilesystemResult<Vec<String>> {
    disk.make_directory("reports/daily").await?;
    disk.all_files(Some("reports")).await
}
```

用 `make_directory` 按磁盘配置的目录可见性创建缺失的父目录，用 `delete_directory` 删除目录及其内容。列表跳过符号链接和临时写入项。创建和删除时须传入具名相对路径。也可通过 `Storage` 在默认磁盘调用这些方法。

## 契约与常用操作

容器可解析 `dyn contracts::filesystem::Factory`、默认 `Filesystem` 及配置的 `Cloud`，复用现有 Manager 和 Adapter。契约中的流接口为 `read_stream` / `write_stream`；Adapter 和 Storage Facade 使用 `read_stream` / `write_stream`。`copy`、`move`（Rust 为 `r#move`）、`append`、`prepend`、`json` 复用当前磁盘；缺失文件和非法 JSON 返回空结果，路径越界仍拒绝。`Cloud` 解析依赖 `filesystems.cloud`，不表示本地磁盘拥有 S3 的签名上传能力。

Rust 中 `append(path, text, separator)` 和 `prepend(path, text, separator)` 需要显式传入分隔符。换行可传 `"\n"`，需要 Windows 行尾时传 `"\r\n"`。`move` 是 Rust 关键字，因此使用原始标识符 `r#move`。

## 使用 File 操作本地文件 {#local-file-facade}

`openrust::support::facades::File` 直接操作本地路径。它解析应用级 `files` 单例（`openrust::filesystem::Filesystem`），与 `Storage` 的具名磁盘相互独立。相对路径从进程当前目录解析；运行目录不固定时应传应用的绝对路径。此服务使用同步文件操作，大文件任务应放入阻塞任务，避免占用异步请求线程。

```rust
use openrust::{filesystem::FileResult, support::facades::File};
use std::path::Path;

fn write_report(directory: &Path) -> FileResult<()> {
    File::ensure_directory_exists(directory)?;
    let path = directory.join("report.txt");
    File::put_with_lock(&path, b"ready\n", true)?;
    File::append_with_lock(&path, b"complete\n", true)?;
    let bytes = File::shared_get(&path)?;
    File::replace(directory.join("copy.txt"), bytes)?;
    for file in File::all_files(directory)? {
        println!("{}", file.relative_pathname().display());
    }
    Ok(())
}
```

`get` 返回原始字节；`get_text` 要求有效 UTF-8。`json` 遇到非法 JSON 返回 null，`json_strict` 则返回解码错误；两者都传播文件访问错误。`lines` 返回惰性字节集合，每次遍历重新打开文件；末尾换行会产生最后一个空项。`hash` 默认使用 MD5，`hash_with_algorithm` 支持 MD5、SHA-1、SHA-256/384/512 和 xxh128。`has_same_hash` 使用 xxh128，任一文件无法读取时返回 false。

`files` 和 `directories` 按相对路径排序，只列直接子项。`all_files` 和 `all_directories` 递归枚举，但不进入目录符号链接。默认排除隐藏项，即使启用隐藏文件，也仍排除版本控制目录。用 `files_with_options(path, hidden, DirectoryDepth::parse([">= 1", "< 3"])?)` 或 `directories_with_depth` 设置深度条件，直接子项深度为零。`glob` 支持 Shell 通配符，除非显式匹配，否则排除点文件。

`replace` 在同目录创建临时文件后原子重命名。Unix 下跟随已有目标符号链接，默认权限为经 umask 过滤的 `0777`；用 `replace_with_mode` 指定权限。`chmod(path, None)` 或 `chmod(path, Some(0))` 返回包含八进制权限的 `FileMode::Current`，非零模式则修改权限。`delete_many` 会尝试所有路径，任一失败便返回 false。`clean_directory` 保留根目录，递归删除只移除子项符号链接，不删除链接目标。目录复制拒绝目标位于源目录内以及符号链接循环。

带类型的 `r#macro`、`mixin`、`macro_call`、`has_macro` 和 `flush_macros` 复用当前应用的本地服务，不与其他应用共享。`File` 需要有效的应用作用域，包括 HTTP、Artisan 和 Worker。HTTP 处理器通过 `?` 传播的读取错误接入正常异常报告；本地文件缺失不会自动变成 HTTP 404。

MIME 检测复用 OpenRust 内容识别器，最多读取 8 KiB，不等同于完整的 libmagic 数据库。此批 File API 不执行动态 PHP／Rust 源码，也未覆盖全部 PHP glob／JSON 标志和摘要算法。Windows 目录链接目前依赖符号链接能力；原生 junction 等价行为及 Windows ACL 尚未包含在已验证的 Unix 行为范围内。

### 编译后的文件模块 {#compiled-file-modules}

在应用装配时通过 `File::register_module(path, callback)` 注册实际存在的源码路径。回调是编译进应用的 Rust 代码，不在运行时解释或编译文件；具类型参数代替动态提取局部变量，无参数时传 `()`。直接复用应用函数，不新增运行时脚本引擎。修改 Rust 代码后需要重新构建应用。

```rust
use openrust::{filesystem::{FileResult, RequiredFile}, support::facades::File};
use std::path::Path;

fn register_and_load(path: &Path) -> FileResult<()> {
    File::register_module(path, |files, name: String| {
        // 中文参数说明：回调是编译进应用的 Rust 函数。
        Ok(format!("Hello {name}: {}", files.exists("Cargo.toml")))
    })?;
    let value: String = File::get_require(path, "OpenRust".to_owned())?;
    let once: RequiredFile<String> = File::require_once(path, "ignored".to_owned())?;
    assert_eq!(once, RequiredFile::AlreadyIncluded);
    assert!(value.starts_with("Hello OpenRust"));
    Ok(())
}
```

`get_require` 每次执行回调并返回具类型结果。`require_once` 首次返回 `RequiredFile::Value(value)`，之后返回 `AlreadyIncluded`，与模块返回 `true` 区分。任何 `get_require` 也会记入包含集合。执行前标记，因此递归以及失败或 panic 后的再次调用都会跳过；替换注册不清空包含状态，但显式 `get_require` 仍执行替换后的回调。

路径规范化后按实际路径判断，符号链接别名共用包含状态。缺失、已删除或非普通文件路径返回文件错误，即使之前已包含；已存在但未注册的文件返回 `UnregisteredModule`，参数或结果类型不符明确报错，不伪造模块值。注册及包含集合属于应用的 `files` 单例，在同应用 HTTP／Artisan／Worker 调用间共享，与其他应用隔离，不采用解释器的进程全局包含表。并发 `require_once` 只认领一次执行；其他调用可能在首个回调尚未完成时看到 `AlreadyIncluded`。执行回调和销毁被替换的注册时不持注册表锁。

### 条件文件操作 {#conditional-file-calls}

通过 `File::when(value, callback)` 和 `unless` 执行条件调用。`ConditionValue` 保留原始参数类型，支持布尔、数字、字符串（`""` 和 `"0"` 为假）、可选值和集合；回调接收实际本地文件服务与原始值。业务对象可自行实现 `ConditionValue`。JSON 对象即使为空也为真，JSON 数组按集合是否为空判断。

```rust
use openrust::{filesystem::FileResult, support::facades::File};
use std::path::Path;

fn write_if_missing(path: &Path) -> FileResult<()> {
    File::unless_test(
        |files| files.exists(path),
        |files, _| files.put(path, b"created").map(Some),
    )?;
    let bytes = File::when_condition(true)?
        .call(|files| files.get(path))?
        .into_value();
    assert!(bytes.is_some());
    Ok(())
}
```

所选回调返回 `FileResult<Option<T>>`。`Some(value)` 返回 `ConditionalResult::Value(value)`，包括 `false`、`0` 和空字符串；`None` 返回 `Target`，保留原服务实例，没有其他类型线索时使用 `None::<()>`。未执行分支也返回 `Target`。用 `into_value`／`into_target` 取出两种结果，错误直接传播。`when_else`／`unless_else` 增加回退回调，只执行一个分支。

条件来自文件服务查询时使用 `when_test`／`unless_test`，解析器只执行一次，并把原结果交给所选回调。`when_condition(value)`／`unless_condition(value)` 返回具类型高阶代理，`.call(|files| ...)` 仅在条件满足时调用真实公开操作。先捕获查询时使用 `when_capture()?.capture(|files| ...)` 或 `unless_capture`；`try_capture` 接受会返回错误的查询。未捕获条件时返回 `MissingCondition`。代理方法返回真实操作值，因此操作返回 `None` 时仍为 `Value(None)`，不套用回调式的空值回退。

门面代理持有原应用服务，移动到另一应用作用域不会重新解析为另一份服务。直接调用 `Filesystem` 提供相同方法，回退目标为原借用。Rust 用明确后缀与 `ConditionalResult` 表达不同参数数量、动态方法代理和目标／标量联合返回，条件、副作用选择及错误传播仍可直接验证。
