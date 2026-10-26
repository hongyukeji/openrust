# ADR-050 统一文件存储与驱动扩展

- 状态：Accepted
- 日期：2026-10-27
- 依据：[ADR-009](adr-009-extensible-capability-drivers.md)、[M5 能力覆盖](../plans/v1-capability-coverage.md#v1-能力覆盖与单一归属)

## 背景

文件存储需要 local 开发路径、生产 S3-compatible 路径、流式 I/O 和可替换 driver。仓库尚无 `src/storage`，逐项自写存储协议会重复安全、资源管理和服务差异处理；仅重导出 OpenDAL 又不能统一应用配置、named disks、路径校验与框架错误。

## 决策

- 在根 `openrust::storage` 提供 `StoragePlugin`、`Storage` 与具名 `StorageDisk`。`[storage]` 选择 default disk，`[storage.disks.<name>]` 配置各实例。默认应用配置使用 local filesystem；缺省目录位于应用当前工作目录的 `storage/` 下。命名隔离及未知名称均显式失败，不回退到 default。
- OpenDAL 是内置 I/O 实现：`storage-local` 提供 filesystem driver，`storage-s3` feature 在 local 基础上加入 S3-compatible driver。仓库使用 OpenDAL facade 并默认关闭未使用服务，固定依赖版本见 `stack.md`。M5 验收以实际 MinIO 服务验证 S3 path、流和对象 metadata，不以 OpenDAL feature 存在替代。
- `StorageDriver` 是应用可实现的公开 driver contract；通过 `StorageDrivers` 显式注册工厂，由应用配置选择。driver config 的 Debug 只显示 option key，不显示路径外的凭据或 secret 值。自定义 driver 必须声明实际 capabilities，未支持能力显式拒绝。
- `StoragePath` 只接受相对、规范化的对象 key；拒绝绝对路径、空/`.`/`..` segment、反斜线和控制字符。用户文件名不直接决定物理 key；应用生成稳定的 opaque key 并在业务数据中保存。local root 目录及父目录必须由运行 OpenRust 的系统账户独占写入；框架路径校验不替代 OS 的权限/符号链接隔离。
- `StorageDisk` 提供流式 put/open、stat 与 delete；对象 metadata 返回可用的 size、content type、etag、version 等 backend 信息，字段不存在时为 `None`。driver 契约要求流式写入采用 staging 或等价原子发布：只有成功 close 后目标对象整体可读。source/inspection/backend 错误显式 abort 并清理 staging；调用 future 被丢弃时由 `UploadCancellation` 通知 driver 尽力 abort。调用在最终 close/原子提交已开始后被取消，backend 可能完成并发布完整对象；任何路径都不能发布部分对象。进程强杀不提供异步清理保证，backend/provider 的残留分片或 staging 文件须由有界生命周期清理策略回收且不能对读取者可见。内置 filesystem 用同卷 staging + rename，正常 drop 清理本次 staging，启动时回收超过 24 小时的遗留 staging；S3 multipart 取消时调用 provider abort，进程强杀的 multipart 残片需由 provider 生命周期策略回收。上传 API 将传给 inspector/driver 的块大小限制在配置范围，但不保证限制 HTTP 上游构造单个 body frame 时的峰值内存；自定义 stream 来源须自行控制单块分配。应用必须在读写前完成主体和业务资源授权。
- S3-compatible disk 只提供有限时长的 presigned read URL，不提供可绕过大小/inspection 校验的 presigned write URL。请求版本绑定时，必须从 stat 取得具体 object version 并在 presign/read 中指定该版本；backend 缺少 version capability 或当前对象无 version 时 fail-closed，不退回按 key 签名。该 URL 是到期前可转发的 bearer 凭据，不能经 `SignedLinks` 撤销。filesystem 不支持 provider presign，应用需通过自己的授权 route 接入 `SignedLinks`；框架不建立通用公开下载 route。Filesystem 仅保证当前节点可见，不能用于多节点共享。
- Storage 领域扩展限于逐块 inspection 与每次调用的应用授权。提交后业务流程、队列事务派发及统一流程 pipeline 按后续扩展点功能簇处理，不创建第二套全局 Hooks 系统。

## 验收

- local 临时目录真实读写、范围/路径/MIME/大小边界、错误/取消时目标 key 不可见与 staging 清理、流式下载及 custom driver 独立 consumer；另明确进程强杀可能留下不可见 provider staging，由有界 lifecycle 规则清理。
- 启动隔离 MinIO 服务，以真实 S3 API 验收上传/读取/删除、metadata、multipart 大对象、超限/inspection 拒绝/取消清理、版本化 bucket 的 presigned URL 过期和 exact-version 绑定；同时验证关闭 versioning 时 fail-closed、对象被覆盖后旧 URL 仍读旧版本、presigned write 不存在；集成测试仅清理本任务创建的资源。
- `apps/server` 接入及真实 TCP/HTTP 有界 multipart 上传和 streamed download；授权由应用 handler 显式检查，绕过前置授权的路径不得暴露对象。
- 按 `AGENTS.md` 执行 workspace fmt、Clippy、tests、warnings-denied Rustdoc、no-default-features、`cargo deny check`、生成/独立 consumer；真实 S3 用例必须明确执行，跳过不能作为通过。

## 非目标

本 ADR 不提供媒体转换、恶意内容检测产品、数据库文件索引模型、公开匿名文件路由、跨节点 local filesystem、数据库与对象存储分布式事务、multipart 直传 bypass 校验，或云厂商专属 API。应用可通过公开 Inspector/driver 和自己的业务授权流程实现检测与生命周期逻辑。
