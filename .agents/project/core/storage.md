# 存储

公共实现位于根 `openrust` package 的 `openrust::storage`，配置位于 `src/config/storage.rs`，用户用法见 [`src/storage/README.md`](../../../src/storage/README.md)。

- `StoragePlugin` 从 `[storage]` 初始化 default/named disk。缺少该配置区时使用 local disk、`storage/app` 和默认上传限制；错误配置、未知 driver、重复 driver 或 backend 初始化失败均阻止应用启动。
- 默认 builtin driver 名为 `fs`，由 `storage-local` feature 提供；`s3` 由 `storage-s3` 提供。关闭 feature 时相应 driver 名仍注册为明确失败的占位项，避免错误悄悄落到另一个 disk。
- `StorageDriver` / `StorageBackend` / `StorageDrivers` 是公开扩展契约。自定义 driver 由应用插件 `build` 阶段注册；配置对象的 Debug 只显示 option 名。
- `StoragePath` 限制相对、规范化 key；本地临时 staging 前缀为保留路径。可信 tenant context 存在时，所有 `StorageDisk` 后端操作使用 `tenants/<tenant-sha256>/<logical-path>` 物理路径，调用方返回/inspector path 保持逻辑值；无 context 时不改路径。直接调用自定义 backend 会绕过此 facade 保护。应用需以 opaque key 存文件并先完成业务授权。
- `StorageDisk::put_stream` 检查总大小、单块大小、声明 MIME allowlist 和按序 `UploadInspector`；`put_body` 限制交给 inspector/driver 的 HTTP 数据块并保留背压，但不能限制 Axum 上游已分配单个 body frame 的内存。直接 stream 调用方同样须限制源 chunk。后端写入需具有提交后整体可见语义；取消通过 `UploadCancellation` 通知 driver 尽力 abort。
- filesystem 通过 OpenDAL `atomic_write_dir` 将 staging 与发布置于同一 root filesystem；staging 名称使用进程内单调序号并对目录碰撞作有界重试，允许多个应用实例并发初始化同一 storage root。正常 backend drop 清理 staging，后续启动回收超过 24 小时的遗留 staging。storage root 必须由当前应用系统用户独占写入。
- 下载 stream 在 framework 边界再次限制为配置 chunk。按 version 打开时 metadata 和 body 必须使用同一版本。`presign_read` 从 stat 读取具体版本并要求 provider 的版本读取能力，不支持时 fail-closed。
- 根 `bootstrap/app.rs` 只在 feature 开启时安装 `StoragePlugin`。HTTP 授权与下载路由由业务应用负责；`storage_http` 集成测试以真实 TCP 验证授权、流式上传与下载。MinIO/S3 的可复跑专项和结果见 [`memory/testing.md`](../memory/testing.md)。

决策和剩余验收要求见 [ADR-050](../decisions/adr-050-storage-drivers.md)。
