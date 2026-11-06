# 存储

公共实现位于独立框架 package 的 `openrust::filesystem`，配置位于 `vendor/openrust/framework/src/config/storage.rs`，用户用法见 [文件存储指南](../../../vendor/openrust/framework/src/filesystem/README.md)。

- `FilesystemServiceProvider` 在 register 提供完整 manager singleton，boot 预解析默认 disk；具名 disk 按名异步懒构造并缓存，`extend` 更新 creator，`forget`/`purge` 后可重建。缺少该配置区时使用 local disk、`storage/app` 和默认上传限制；最终配置 validator 检查已登记 driver 与无 I/O 选项，默认 backend 初始化失败阻止启动，未解析具名 disk 不提前访问外部存储。第三方 driver 不提供专属 validator 时只能保证通用结构。
- 默认 builtin driver 名为 `fs`，由 `storage-local` feature 提供；`s3` 由 `storage-s3` 提供。关闭 feature 时相应 driver 名仍注册为明确失败的占位项，避免错误悄悄落到另一个 disk。
- `StorageDriver` / `StorageBackend` 是公开扩展契约。自定义 driver 通过 Provider `with_driver` 或 manager `extend` 注册；配置对象的 Debug 只显示 option 名。
- `StoragePath` 限制相对、规范化 key；本地临时 staging 前缀为保留路径。可信 tenant context 存在时，所有 `FilesystemAdapter` 后端操作使用 `tenants/<tenant-sha256>/<logical-path>` 物理路径，调用方返回/inspector path 保持逻辑值；无 context 时不改路径。直接调用自定义 backend 会绕过此 facade 保护。应用需以 opaque key 存文件并先完成业务授权。
- `FilesystemAdapter::put_stream` 检查总大小、单块大小、声明 MIME allowlist 和按序 `UploadInspector`；`put_body` 限制交给 inspector/driver 的 HTTP 数据块并保留背压，但不能限制 Axum 上游已分配单个 body frame 的内存。直接 stream 调用方同样须限制源 chunk。后端写入需具有提交后整体可见语义；取消通过 `UploadCancellation` 通知 driver 尽力 abort。
- filesystem 通过 OpenDAL `atomic_write_dir` 将 staging 与发布置于同一 root filesystem；staging 名称使用进程内单调序号并对目录碰撞作有界重试，允许多个应用实例并发初始化同一 storage root。正常 backend drop 清理 staging，后续启动回收超过 24 小时的遗留 staging。回收时已列举的 run 条目若被另一实例删除，仅在复查 staging 根目录当前可读时允许 NotFound；根缺失、非目录及其他IO错误继续失败。复查并非原子目录身份校验，不保证抵御根目录并发替换。storage root 必须由当前应用系统用户独占写入。
- S3 上传先写受保留路径保护的随机 `.openrust-staging/` key，完整校验、EOF、关闭及读取源 metadata 后才以同 bucket copy 发布最终 key。copy 开始是发布边界；调用方取消或远端响应故障可能已完成发布，返回 `storage_publication_uncertain` 时须读取最终 key 核对，不能补偿删除它。正常路径有界重试清理暂存，失败记录安全告警；进程强杀/远端长期无响应时暂存需按 disk root 的专用 lifecycle 与未完成 multipart 策略回收，不能把普通业务 key 纳入该清理。MinIO 双库与版本化回归见 [R1-E testing](../memory/testing.md#r1-e-业务示例定向验收证据整簇进行中)。
- 下载 stream 在 framework 边界再次限制为配置 chunk。按 version 打开时 metadata 和 body 必须使用同一版本。`presign_read` 从 stat 读取具体版本并要求 provider 的版本读取能力，不支持时 fail-closed。
- 根 `bootstrap/app.rs` 只在 feature 开启时安装 `FilesystemServiceProvider`。HTTP 授权与下载路由由业务应用负责；`storage_http` 集成测试以真实 TCP 验证授权、流式上传与下载。MinIO/S3 的可复跑专项和结果见 [`memory/testing.md`](../memory/testing.md)。
- `modules/example` 的文章附件流程另用数据库文章行锁串行化 S3 上传和删除；请求 body EOF 与 deadline 以原子状态裁决，deadline 获胜禁止进入 storage 正常 EOF/发布，EOF 获胜则事务锁持有到发布调用收敛。该是业务模块协调 DB/S3 的保证，不是 `FilesystemAdapter` 提供的跨系统事务。CopyObject 已发送后若返回 `storage_publication_uncertain`，远端仍可能迟到完成；进程强杀与 App 关闭期限亦无法承诺无孤儿。

决策和剩余验收要求见 历史决策 050。

ST-V来源`2262df9`已由main两父提交`8d18021b630ea10652316e025ef6019c7772bbb0`纳入：公开visibility读写/变更、local安全发布与S3 ACL分类/Copy仲裁的限定组合按[实际验收](../memory/testing.md#st-v-main-限定组合验收)通过。真实ACL启用/禁用bucket正反例、远端迟到CopyObject与Laravel `throw=false`失败值仍开放；被忽略的S3用例不算服务证据。
