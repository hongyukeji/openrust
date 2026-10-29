# 文件存储

Provider 装配的存储 disk 按名称异步构造并缓存；`disk/default_disk` 需要 await。driver 的 `validate` 默认不执行专属检查，自定义 driver 应覆盖它，才能在最终配置校验时拒绝无效选项。

`StorageServiceProvider` 支持在统一容器中装配内置和自定义 driver。

`openrust::storage` 提供具名存储 disk、流式上传/下载、对象元数据和公开 driver 扩展。Provider 负责装配配置；业务 handler 必须先完成用户身份和资源授权，再调用存储服务。框架不提供公开匿名下载路由，也不把用户文件名用作对象 key。

## 配置

启用本地文件系统时，在应用依赖中打开 `storage-local`，并在 `Application::providers` 清单中注册 `StorageServiceProvider::new()`。未配置 `[storage]` 时，默认使用 `local` disk 和当前工作目录下的 `storage/app`。

```toml
[storage]
default_disk = "local"
max_upload_bytes = 33554432
max_chunk_bytes = 1048576

[storage.disks.local]
driver = "fs"
root = "storage/app"
```

`[storage.disks.<name>]` 中的 `driver` 选择已注册 driver，其余键由该 driver 读取。路径相对当前工作目录解析。进程用户必须独占存储根目录及父目录的写权限；本地文件系统只对当前节点可见。原子写入暂存目录位于 storage root 内，正常释放时删除；进程强杀留下的目录在后续启动时按 24 小时阈值回收。

启用 S3-compatible 存储时增加 `storage-s3` feature。配置须显式提供 bucket、region、endpoint、access key ID 和 secret access key；可额外设置 root 前缀。建议通过环境变量或仅本地 `.env` 提供凭据，不要提交真实密钥。

```toml
[storage]
default_disk = "media"

[storage.disks.media]
driver = "s3"
bucket = "app-media"
region = "us-east-1"
endpoint = "http://127.0.0.1:9000"
root = ""
access_key_id = "本地环境变量提供"
secret_access_key = "本地环境变量提供"
```

启动时校验默认 disk 和已登记 driver 的配置；默认 disk 初始化失败会使启动失败。其他 disk 按需初始化，未知名称、初始化失败或 driver 专属配置错误会在解析时返回错误，不会静默回退。自定义 driver 的专属选项是否在启动时校验取决于其 `validate` 实现。使用具名 disk 时调用 `storage.disk("media").await?`。 在可信 `TenantContext` 作用域内，框架会将 tenant ID 的 SHA-256 摘要加到后端物理 key 前缀；应用获得的 `StoredObject.path` 与上传检查器仍只看到逻辑路径。没有租户作用域时沿用原有物理路径。自定义 driver 通过同一 facade 调用时也会收到隔离后的路径；直接使用 driver 不受此保护。

## 上传与下载

使用 `StoragePath::new` 校验应用生成的相对 key。路径拒绝绝对路径、空路径段、`.`、`..`、反斜线、控制字符和框架保留的暂存目录。不要直接把用户提供的文件名作为 key；可将文件名保存在业务记录中，并另行生成不可猜测的对象 key。

对于 HTTP 请求正文，`StorageDisk::put_body` 将交给 inspector 和 driver 的块切到配置上限，并保留流式背压；它不能限制 Axum 上游在产生单个 body frame 时已分配的内存。直接使用 `put_stream` 时，调用方应按有界块提供 `StorageStream`，单个上游块也会占用内存。两种入口均执行总字节数、单块大小、声明 MIME allowlist 和可选 `UploadInspector`；检查按块有序执行，EOF 时调用 `finish`，任何失败都不会发布部分对象。MIME 检查只校验客户端声明，敏感内容检测由应用提供 Inspector 实现。

```rust,ignore
let disk = storage.default_disk().await?;
let key = StoragePath::new(format!("users/{opaque_id}"))?;
let stored = disk
    .put_body(
        key,
        Some("image/png".into()),
        request.into_body(),
        UploadOptions::default(),
        Some(inspector),
    )
    .await?;
```

`StorageReader` 暴露对象元数据和按序有界 `Bytes` stream；下载时可用 Axum `Body::from_stream(reader.body)` 接回 HTTP 响应。检查 caller 授权后再打开对象，响应头只采用通过 `HeaderValue` 校验的元数据；不要把未验证的文件名拼入 `Content-Disposition`。

## Driver 与版本链接

应用可实现 `StorageBackend` / `StorageDriver`，通过 `StorageServiceProvider::new().with_driver(driver)` 注册，或在后续 Provider boot 中解析 `Storage` 并调用 `extend(driver)`。默认 disk 在 boot 解析，其他 named disk 按需构造并缓存；替换 creator 不改变已解析实例，调用 `forget_disk` / `purge` 后可重建。driver 可实现 `validate` 检查专属选项，未实现时框架无法验证自定义选项。driver 必须维持单调用背压、版本读取 metadata 对齐、失败/取消时尽力 abort，以及只有完成提交后目标 key 才可见。写入关闭时是 backend 发布边界；如果调用方在提交已经开始后取消，完整对象可能仍被发布，但部分对象不得可见。进程强杀可能留下不可见的服务端 multipart 残片；driver 需提供有界清理策略。

内置 S3 driver 将通过检查的完整内容先写到当前 disk root 下的私有 `.openrust-staging/` 随机 key，完成写入、EOF 检查与关闭后，才以同 bucket copy 发布到目标 key。copy 开始是提交边界：此后调用方取消或远端响应丢失，完整目标对象仍可能已发布；`storage_publication_uncertain` 要求调用方读取目标状态核对，不能盲目补偿删除目标。正常失败和取消会有界尝试清理暂存对象，清理失败记安全告警；进程强杀或远端持续不可用时不保证同步清理。运维应只针对各 disk root 内的 `.openrust-staging/` 设置短期对象过期与未完成 multipart 回收；版本化 bucket 还需处理该暂存前缀的旧版本和删除标记，不能把同一规则施加到业务目标前缀。

`presign_read` 只为当前对象的确切版本生成短期只读 URL。bucket 未启用版本控制、对象没有 version ID 或 backend 不支持版本化 presign 时会 fail-closed。此 URL 是到期前可转发的 bearer 凭据，不能通过 `SignedLinks` 撤销；local disk 不提供 provider presign，需要应用自行建立授权 route。框架不提供 presigned write URL。


本地 disk 的 staging 回收允许其他实例在扫描后正常关闭并删除自身 run 目录。仅该条目已消失且 staging 根目录当前可读时忽略 NotFound；根目录丢失、非目录、权限或其他 I/O 错误仍阻止初始化。根目录检查不是原子身份校验，不能抵御并发替换；storage root 仍须由当前应用系统用户独占写入。
