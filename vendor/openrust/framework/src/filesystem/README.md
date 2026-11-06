# 文件存储

Provider 装配的存储 disk 按名称异步构造并缓存；`disk/default_disk` 需要 await。driver 的 `validate` 默认不执行专属检查，自定义 driver 应覆盖它，才能在最终配置校验时拒绝无效选项。

`FilesystemServiceProvider` 支持在统一容器中装配内置和自定义 driver。

`openrust::filesystem` 提供具名存储 disk、流式上传/下载、对象元数据和公开 driver 扩展。Provider 负责装配配置；业务 handler 必须先完成用户身份和资源授权，再调用存储服务。框架不提供公开匿名下载路由，也不把用户文件名用作对象 key。

## 配置

启用本地文件系统时，在应用依赖中打开 `storage-local`，并在 `ApplicationDefinition::providers` 清单中注册 `FilesystemServiceProvider::new()`。未配置 `[storage]` 时，默认使用 `local` disk 和当前工作目录下的 `storage/app`。

```toml
[storage]
default_disk = "local"
max_upload_bytes = 33554432
max_chunk_bytes = 1048576

[storage.disks.local]
driver = "fs"
root = "storage/app"
visibility = "private"
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

对于 HTTP 请求正文，`FilesystemAdapter::put_body` 将交给 inspector 和 driver 的块切到配置上限，并保留流式背压；它不能限制 Axum 上游在产生单个 body frame 时已分配的内存。直接使用 `put_stream` 时，调用方应按有界块提供 `StorageStream`，单个上游块也会占用内存。两种入口均执行总字节数、单块大小、声明 MIME allowlist 和可选 `UploadInspector`；检查按块有序执行，EOF 时调用 `finish`，任何失败都不会发布部分对象。MIME 检查只校验客户端声明，敏感内容检测由应用提供 Inspector 实现。

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

## 可见性

`[storage.disks.<name>].visibility` 可选，取 `private` 或 `public`；未配置时 local 默认为 `private`。单次上传用 `UploadOptions { visibility: Some(Visibility::Public), ..Default::default() }` 覆盖。`disk.default_visibility()` 返回显式配置的 `Option<Visibility>`，`None` 不保证第三方 driver 的实际默认权限；显式 public/private 要求自定义 driver 生效或返回不支持。`disk.visibility(&path).await?` 读取实际权限，`disk.set_visibility(&path, Visibility::Private).await?` 修改权限。三个入口均经过租户路径隔离。Rust API 用 `Result` 返回失败；Laravel 的 `throw=false` 返回值策略尚未对应。

Unix local 在发布前把暂存文件设为 `0600` 或 `0644`，再原子改名到目标；新建目录分别为 `0700` 或 `0755`。写公开对象不会放宽已有私有父目录；父目录无法供其他用户遍历时返回 `storage_capability_unsupported`，只需 other execute 权限，不要求列目录权限。存储根及父目录由部署者负责设置权限，框架不修改已有目录。目录句柄按段拒绝符号链接，现有文件要求单一硬链接，修改权限通过文件句柄执行；存储根仍须由应用用户独占写入，以防目录被并发迁移。非 Unix 平台保留未指定 visibility 的普通写入，显式权限写入及读取/修改权限返回不支持。

S3 driver 仍由 OpenDAL 将完整内容写入私有 staging，再由 S3 SDK 在最终 `CopyObject` 请求中携带目标 ACL；未指定可见性时沿用 S3 的私有默认。`visibility` 读取对象 ACL 中 `AllUsers` 的读取权限，`set_visibility` 修改目标对象 ACL；缺失或不认识的 ACL 字段会报错，不推断为 private。SDK 的 `GetObjectAcl`、`PutObjectAcl`、`CopyObject` 各请求设 30 秒操作超时且不自动重试；OpenDAL 的 staging 写入、关闭、stat 和清理不在此超时内，因此整条上传链没有 30 秒总时限。最终 Copy 的提交任务在调用方 future 丢弃后继续完成并清理 staging；Copy 超时仍报告发布结果不确定，不删除目标。ACL 禁用、权限不足或策略阻止会返回错误，不修改 bucket policy。真实 S3 服务与 ACL 策略验证尚待集成验收；public 不等于互联网可读，bucket policy 和服务端设置仍会决定最终访问。

## Driver 与版本链接

应用可实现 `StorageBackend` / `StorageDriver`，通过 `FilesystemServiceProvider::new().with_driver(driver)` 注册，或在后续 Provider boot 中解析 `FilesystemManager` 并调用 `extend(driver)`。默认 disk 在 boot 解析，其他 named disk 按需构造并缓存；替换 creator 不改变已解析实例，调用 `forget_disk` / `purge` 后可重建。driver 可实现 `validate` 检查专属选项，未实现时框架无法验证自定义选项。driver 必须维持单调用背压、版本读取 metadata 对齐、失败/取消时尽力 abort，以及只有完成提交后目标 key 才可见。写入完成检查后通过 `UploadCancellation::begin_publication()` 与调用方取消争夺同一发布权；取消先赢绝不提交，发布先赢须完成原子提交，且持有的 `UploadPublication` 仅在成功后 `complete()`、已知失败后 `fail()`，结果不明时 `uncertain()` 并返回 `storage_publication_uncertain`。框架对已领取但未结清、或标记结果不明后返回其他错误的 driver 也保守映射为 `storage_publication_uncertain`，已知失败仍保留 driver 的原始错误。异步提交要与可能被丢弃的调用方 future 解耦；响应丢失时不得盲目删 final，应报结果不确定。进程强杀可能留下不可见的服务端 multipart 残片；driver 需提供有界清理策略。

内置 S3 driver 将通过检查的完整内容先写到当前 disk root 下的私有 `.staging/` 随机 key，完成写入、EOF 检查与关闭后，才以同 bucket copy 发布到目标 key。领取发布权的 CAS 是与取消仲裁的提交边界；随后同一同步段构造 Copy 请求并将其交给独立任务执行。此后调用方取消或远端响应丢失，完整目标对象仍可能已发布；`storage_publication_uncertain` 要求调用方读取目标状态核对，不能盲目补偿删除目标。正常失败和取消会有界尝试清理暂存对象，清理失败记安全告警；进程强杀或远端持续不可用时不保证同步清理。运维应只针对各 disk root 内的 `.staging/` 设置短期对象过期与未完成 multipart 回收；版本化 bucket 还需处理该暂存前缀的旧版本和删除标记，不能把同一规则施加到业务目标前缀。既有 `.openrust-staging/` 对象不会自动迁移或删除，仍禁止通过业务路径读取；部署时需单独核查旧前缀的生命周期规则与遗留对象。

`presign_read` 只为当前对象的确切版本生成短期只读 URL。bucket 未启用版本控制、对象没有 version ID 或 backend 不支持版本化 presign 时会 fail-closed。此 URL 是到期前可转发的 bearer 凭据，不能通过 `SignedLinks` 撤销；local disk 不提供 provider presign，需要应用自行建立授权 route。框架不提供 presigned write URL。


本地 disk 首次创建 `.staging/` 时写入 `.storage-owner` 标记；后续启动只接管标记有效的目录。此前合法的同名业务目录或符号链接会使初始化失败，框架不会改权限或清理其内容，应用应人工选择新存储根并核查原对象。staging 回收允许其他实例在扫描后正常关闭并删除自身 run 目录。仅该条目已消失且 staging 根目录当前可读时忽略 NotFound；根目录丢失、非目录、权限或其他 I/O 错误仍阻止初始化。根目录检查不是原子身份校验，不能抵御并发替换；storage root 仍须由当前应用系统用户独占写入。
