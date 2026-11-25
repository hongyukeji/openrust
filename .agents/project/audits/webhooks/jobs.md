# Webhook 接收处理 Job

本批覆盖 WC-11 的原生队列消费者，沿用固定 Spatie Client `ff886d1bae33d35ad98b6c91403f280df158129d` 的 `Jobs::ProcessWebhookJob` 及 Processor 的保存后派发顺序。上游通过 ShouldQueue、Dispatchable、InteractsWithQueue 和 SerializesModels 执行。Rust 复用 Bus、QueueManager、JobHandler、JobOptions 和 JobContext，按接收记录标识恢复，注册方式沿用仓库内 Telescope 的原生消费者模式。旧版没有可复用的通用 Webhook 处理 Job。依赖锁文件只新增本包对已锁定 serde 的引用。

应用在 HTTP 和 Worker 共用的 Provider 注册稳定名称、可信范围的存储和业务处理器。配置可选择该具名派发器。队列仅序列化 ID、scope、profile 和 revision，Worker 通过注册的 Store 读取，并逐项匹配后执行，构造 Worker 时无需解析接收密钥。库保留公开手动派发扩展。原生派发器要求已注册的队列处理器与成功入队结果，拒绝框架归类为当前进程生命周期的直连驱动。自定义或 failover 队列的实际耐久性由部署组合决定，本批仅验收 database。

Worker 在执行前清除旧诊断。业务异常保存固定诊断，原始错误可能含凭据或正文，未写入失败队列或接收记录。原生 JobOptions 传递连接之外的尝试次数、退避和超时，JobContext 提供实际队列、连接、次数及可用事务。业务回调须能承受队列至少一次执行。退出／硬超时导致的未完成状态、记录提交到入队之间的崩溃补偿、租约／代次与并发幂等仍待 WH-03。当前阶段不宣称完整继承契约、全部队列驱动或双拓扑闭合。

源码隔离基线 `d7409544f510b7c0e58ad6ec072a9f7cc6865153`。编译与测试使用原生 Rust 1.94.0、项目 build_guard 和锁文件。实测日志位于 `target/tmp/webhooks-job`。

PostgreSQL 18.7、MySQL 8.4.13、SQLite 各通过 124 个真实 HTTP 请求与 80 次独立 Artisan Worker 运行。三种缓存状态均走正式注册的 Job，保留原文与附件读取，另核对只有四项接收标识入队。每库验证首次业务失败后重试成功、两次失败进入 failed_jobs、敏感异常文本未落入终态异常、无处理器和 null 连接假确认被拒绝。Worker 启动时换用其他接收密钥，已验证记录仍可读取和处理。独立 Artisan 读取回归继续通过。测试核对 12 秒超时配置传入原生任务，本批没有触发超时或进程中断。

接收包 31 项 Rust 回归、最终应用夹具与六段三语文档示例编译、全仓格式、两项 Webhook feature 启用后的 workspace/all-targets 严格 Clippy，以及三语文档站构建通过。隔离数据库和站点生成目录已清理，保留日志与可复用编译缓存。
