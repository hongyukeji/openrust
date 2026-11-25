# Webhook 无稳定事件 ID 的有限摘要去重

本批补 WH-02／WH-03 的无稳定事件 ID 路径。固定 Spatie 默认 HMAC 验证正文，默认入口维持原行为。[固定 Standard Webhooks 规范](https://github.com/standard-webhooks/standard-webhooks/blob/daee8284cfe9d4e435ec16e3e68a8f30084d6aaf/spec/standard-webhooks.md)的已签 ID 去重仍使用原有键。无稳定 ID 的 profile 可显式选择 `deduplicate_body_seconds`，范围为 1–3,600 秒，且要求同库原子模式。两个去重模式在同一 profile 中互斥。应用只有确认完全相同的合法输入在窗口内可以合并，才选择摘要方式。

原始方法、路径及查询串、正文和 Content-Type 按长度编码后统一取指纹。键按 scope 和 profile 隔离，使用独立的长度分隔字段区分摘要和已签事件键。首次收到使用数据库时钟记录截止，重复命中直接确认。过期后事务内替换同键的旧索引行，原接收记录及业务任务仍可读取。不存在的键直接依赖唯一约束，省去无行时先删除触发的间隙锁。新接收、活动键和原生队列任务一起提交，HTTP 确认继续由现有响应合同决定。遗留的已签事件键通过迁移保留空截止时间。

隔离源码使用 `4f19d7c29` 加本任务提交至 `b1b995fdd` 及本批文件，Rust 1.94.0，构建和验收由项目 build_guard 执行。测试走真实 HTTP，分别在首次提交及旧窗口结束后让两请求竞争同键，核对活动键、原接收记录与实际 Worker 数量。核对改变查询、方法、Content-Type 产生独立接收，不采信未签的事件头。错误签名或配置失败留零记录。日志保留于 `target/tmp/webhooks-body-dedup`。

未验签提示、跨后端恢复扫描、正式跨主机和真实供应商仍待相应门禁。保留期清理归后续运维批次，旧索引可在同键复用或删除接收记录时回收。WH-02 和 WH-03 仍在实施。

SQLite、PostgreSQL 18.7 与 MySQL 8.4.13 全部通过最终代码。每库复验 169 个真实 HTTP、116 次独立 Worker、5 次 Artisan 唤醒，其中本批新增 10 个 HTTP、6 次 Worker。两次竞争均通过，过期后保留早期收件、只替换索引。覆盖初始迁移、回滚七个扩展迁移、重新迁移和旧记录读取。旧接收与签名回归、许可检查、全仓格式、两项 Webhook feature 的 workspace/all-targets 严格 Clippy、三语示例及站点浏览器检查通过。既有块体积提示仍在。隔离数据库及站点输出已清理。
