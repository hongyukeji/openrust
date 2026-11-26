# Webhook 单机与双节点验收

同一 Rust 验收制品同时启用接收和发送包。双节点使用独立应用目录、配置缓存、路由缓存和 HTTP 进程。测试负载均衡逐请求轮询，HTTP、Artisan Worker 和 Scheduler 都通过真实入口。两个应用节点运行在同一台 macOS ARM64 主机，数据库和 Redis 使用隔离 Docker 容器。附件通过两节点共同可访问的私有目录读取。

| 权威数据库 | 队列 | 调度缓存 | HTTP 节点 | 结果 |
| --- | --- | --- | ---: | --- |
| SQLite | database | database | 1 | 通过 |
| PostgreSQL 18.7 | database | database | 2 | 通过 |
| MySQL 8.4.13 | database | database | 2 | 通过 |
| PostgreSQL 18.7 | Redis 8.10.3 | Redis | 2 | 通过 |
| MySQL 8.4.13 | Redis 8.10.3 | Redis | 2 | 通过 |

- 五个组合均执行 Artisan 迁移及配置、路由缓存，实测缓存前后的 HTTP。双节点中由节点 0 写入私有附件、节点 1 的 Worker 读取；两节点并发收到同一个已验签事件时，保存一个接收记录并只入队一次。
- 接收节点在持久提交后、派发前实际退出。另一节点仍能确认重试请求，两个回收命令竞争后只入队一次，独立 Worker 核对原始正文。另对 database 队列的接收 Worker 注入领取后退出，超过实际租约后另一 Worker 读取原文并完成新代次，尝试历史保留过期及完成状态。
- 两个 `schedule:run` 进程使用 `on_one_server()` 竞争同一 UTC 分钟的共享缓存选举，首次恢复只执行一次。共享队列消费后，每个投递只有一次已记录尝试，HTTP 收件端验证正文和 HMAC。并发发送跨节点共用作用域额度，第三笔在消耗尝试前等待，其它作用域仍可独立发送。
- Redis 组合以 Redis 承载持久派发和出站队列，并以 Redis 缓存选举。需要与接收记录同库事务的附件和事件仍走 database 队列。固定 Laravel 的单服务器调度和队列连接继续由框架承担，领域状态、领取和限额保存在共享主写库。旧版 `6d1a81924` 无可复用的同类验收。 [固定 Laravel 调度](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/scheduling.md) · [固定 Laravel 队列](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/queues.md)

`tests/webhooks_topology_acceptance.py` 接收实际编译的 `webhooks_app` 路径。SQLite 用 `OPENRUST_TEST_SMS_SQLITE=1`，PostgreSQL 和 MySQL 使用各自隔离验收包装器。额外设 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_REDIS=1` 验证 Redis。验收制品通过 `cargo fmt --all -- --check` 及两包 feature 下的 `cargo clippy --test webhooks_app -- -D warnings`。数据库、Redis 和临时应用目录已清理。

## 拓扑中的同步派发 {#synchronous-dispatch}

沿用固定 Spatie 源码中同步条件派发当场执行的行为，复用 OpenRust 已有 Builder、投递记录、统一领取和验签收件端。在缓存前由节点 0 派发，缓存后由节点 0 派发，再将相同幂等键和快照交给负载均衡后的下一节点重试。SQLite 的重试由同一节点处理。测试核对 HTTP 返回前已有一次 `succeeded` 尝试及 204 的远端状态、出站队列计数未增加，再次调用返回同一个投递 ID，收件端仅看到一次含正确签名的原始正文。 [固定 Spatie `WebhookCall`](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/src/WebhookCall.php)

基线 `699ad83cd`，Rust 1.94.0，macOS ARM64。上表五种本地组合均通过，包括 SQLite 单节点、PostgreSQL 18.7／MySQL 8.4.13 双节点，以及后两者分别搭配 Redis 8.10.3。每组执行三次同步 HTTP 调用并验证两笔真实发送，既有迁移、独立 Artisan／Worker、缓存、异步发送及故障恢复同次复跑。冻结源码通过全工作区格式检查，以及启用收发包后应用和两包所有目标的严格 Clippy。本批仅调整验收和文档。

## 单机 file 缓存 {#local-file-cache}

使用同一个 Rust 1.94.0 制品，设置 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_FILE_CACHE=1`，分别在内嵌 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 启动一个 HTTP 节点，以 database 队列和同一应用目录下的 file 缓存运行。每库由两个独立 `schedule:run` 进程在同一分钟竞争，只有一个进程执行恢复扫描并推送唯一任务。独立 Worker 完成发送，真实收件端验签。各库同时复跑缓存前后同步、接收记录、HTTP／Worker 退出恢复及共享投递额度。file 缓存覆盖依据为当前应用的本地文件目录与同一主机的进程，跨主机配置见上表。

## 同制品扩缩容 {#replica-scaling}

开启 `OPENRUST_TEST_WEBHOOK_TOPOLOGY_SCALING=1`。先在两节点登记同一个签名事件和一笔异步投递，通过独立 Artisan 将投递入列。第三个节点用同一个二进制、独立应用目录和独立配置／路由缓存接入负载均衡。三个节点并发收到同一原文和签名时仍只有一个接收记录和一个接收任务。将前两个节点移出负载均衡并停止进程后，保留节点继续处理重复请求，并由其独立 Worker 处理原先的两项任务。测试核对原接收 ID、原文、原投递 ID、单次成功尝试、收件端 HMAC 和清空的两侧队列。

在 `699ad83cd` 冻结 Rust 源码上实测 PostgreSQL 18.7、MySQL 8.4.13 各搭配 database 及 Redis 8.10.3，四组均通过。MySQL＋Redis 首轮在磁盘停线时由守卫中断，持锁清理后用同一源码及 Rust 1.94.0 重新构建并单独通过。Rust 源码沿用前述格式与严格 Clippy 结果，本批另核对 Python 语法和改动差异。

此验收覆盖 WH-V25、WH-V26 和 WH-V27 的上述路径。WH-V26 已补[同主机双节点共享 S3 附件](shared-object-storage.md)，真实跨主机、托管对象存储、生产入口和目标操作系统仍待验。WH-V27 已补[共享主写库和 Redis 断连与恢复](backend-interruption.md)及[Scheduler 退出、旧 Worker 回归、端点撤销与凭据缺失](cluster-ownership.md)。[同主机两版本滚动升级](rolling-upgrade.md)已覆盖选定的增量迁移。节点时差、真实跨主机与生产升级仍待验收。完整 Webhook 门禁保持开放。

<a id="separate-vm-guests"></a>

## Linux 与 Windows 双虚机

复用 [ARM64 原生验收](native-platforms.md) 的 Rust 1.94.0 验收应用。Linux HTTP 接收进程运行在 Docker Desktop 的 Linux 虚机，Windows HTTP 生产进程及两个独立 Worker 运行在 Parallels Windows 11 ARM64 虚机。两端使用各自的应用目录。接收地址和 PostgreSQL 18.7 只发布到宿主的 Parallels 私有接口，Linux 端通过容器网络访问数据库，Windows 端通过私有接口访问同一个数据库队列。

在 Windows 执行 Artisan 迁移、配置缓存和路由缓存。实际 HTTP 写入出站记录，Artisan 入队，Windows 出站 Worker 将签名 JSON 发送到 Linux 的 `retention-digest` 接收配置。Linux 验签后同库原子入队，Windows 接收 Worker 执行领取和完成，在独立目录输出逐字节相同的正文。Artisan 查询确认出站成功。PostgreSQL 保留一条已处理记录和一条完成的领取尝试，双方队列和失败账本为空。

前两次使用普通接收配置和通用原子配置，均完成跨虚机投递及读取。两个配置的测试 Job 不写租约完成字段，最终用同时启用原子入队和领取的配置验证持久终态。日志保存在 `target/tmp/webhook-guest-topology/`，本批应用目录、数据库、容器和网络已清理。双虚机共享同一物理宿主，物理跨机、托管服务、生产 TLS 入口和独立节点时钟仍按发布部署复验。
