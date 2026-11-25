# Webhook 已验签事件去重

本批补 WH-03 的稳定事件身份和重复 HTTP 投递边界。固定 [Standard Webhooks 规范](https://github.com/standard-webhooks/standard-webhooks/blob/daee8284cfe9d4e435ec16e3e68a8f30084d6aaf/spec/standard-webhooks.md) 将签名覆盖的事件 ID 用作幂等键，每次重试允许使用新的签名时间戳。固定 Spatie 默认 HMAC 只保护正文。沿用其现有布尔验签入口，并给验签 trait 增加默认空实现的 `verified_event_id`。自定义实现可在已验材料中找到 ID。开启去重且没有有效 ID 时拒绝接收。Standard 去重入口拒绝含协议分隔符句点的 ID。默认关闭，原有接收行为保持兼容。

模式要求现有同库原子派发。数据库唯一键保护 scope、profile 和稳定事件 ID，请求指纹涵盖方法、原始路径及查询串、正文和 Content-Type。键和指纹均为定长摘要。签名时间戳、签名和凭据修订不参与重复判断。同键同指纹走通常的确认响应，同键异指纹的公开控制器返回 409。两个写入方竞争唯一索引时，失败方先回滚临时接收行，再从写连接读取已提交结果。接收、去重凭据、原生队列三者必须同一事务提交，去重凭据随接收记录清除。

隔离源码为 `4f19d7c29` 加上一批 `a6d725045` 及本批文件。Rust 1.94.0 和项目 build_guard。真实 HTTP 测试用独立客户端生成签名，两个独立服务器进程并发发送同一事件给共享数据库。用 Worker 和 Artisan 读取命令分别检查入队和快照。验证首次入队失败回滚、恢复后首次成功接收、重复和冲突、大小写及 profile 隔离、默认正文验签拒绝提供未签身份、Standard 分隔符限制和外部验证器从签名正文提取独立 ID。删除接收记录后核对去重键联动清除。

本批统一采用接收保留期作为已知 ID 的去重窗口。无稳定 ID 的摘要去重、未知来源提示及待关联、跨后端补偿／定时恢复、正式多主机与供应商验收仍分别开放。三语用法同步。

SQLite、PostgreSQL 18.7、MySQL 8.4.13 每库实际通过本批 13 个 HTTP 请求和 4 次独立 Artisan Worker，另复跑共 139 个已有 HTTP 请求与 91 次已有 Worker。迁移首次安装、回滚三个接收扩展迁移后重新迁移、升级前记录读取均通过。旧 HTTP／Artisan、许可、全仓格式、两 Webhook feature workspace/all-targets 严格 Clippy、三语 Rust 示例、文档站和浏览器检查通过。浏览器覆盖根／子目录和三语桌面、移动端及导航。站点仍有既有块体积提示。日志保留于 `target/tmp/webhooks-identity`，清理隔离数据库和本批站点产物。
