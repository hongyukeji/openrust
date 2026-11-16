# 临时 Docker 验收与清理

在 macOS 的 Docker Desktop 上，以 Docker Engine 29.9.3、Buildx 0.38.1、BuildKit 0.34.2 和 Compose 5.7.0 验收 [Docker 验收入口](../../../docker/accept.py)。测试使用隔离登记目录和真实 `docker-container` builder，构建上下文使用静态 Linux 测试程序。

| 场景 | 实测结果 |
| --- | --- |
| 成功 | 镜像构建、加载、Compose 启动和本地导出均完成，退出码 0。验收容器、镜像标签、Compose 网络、builder 及其状态卷移除。导出文件保留且内容正确。 |
| 失败 | Dockerfile 的 `RUN` 退出 37，入口传回 Buildx 的退出码 1。已加载的临时镜像、builder 和状态卷清理完成。 |
| 取消 | 真实 `RUN` 执行中向入口发送 SIGINT，Buildx 取消构建，入口退出 130。临时镜像、builder 和状态卷清理完成。 |
| 强杀后复查 | SIGKILL 终止入口后，先验证有存活工作进程锁时保留，再验证锁释放但验收容器运行时保留，随后验证真实 BuildKit 请求在途时也保留。三个条件均解除后，下次启动回收遗留，回收前专属 BuildKit 缓存为 3.363 MB。 |
| 保护其他资源 | 与验收镜像同 ID 的发布标签、挂载的数据卷和既有数据卷均保留。MySQL、PostgreSQL 和 Redis 容器 ID 与运行状态一致。默认 BuildKit 缓存前后均为 0 B，另一个同命名前缀但无验收登记的 builder 保留了全部 4 条缓存记录及其大小。 |

示例命令见[项目规范](../conventions.md#临时-docker-验收)。本次验证的是临时 Docker 验收入口的生命周期，正式项目 Dockerfile 和 Compose 骨架仍待填充。

## 临时 MySQL 数据卷

MySQL 8.4 镜像的数据目录会生成匿名卷。临时容器按旧规则删除后，匿名卷继续占用空间。启用 `python3 docker/accept.py --mysql -- <测试命令>` 后，入口显式创建带本次标签的数据卷并挂载；收尾按标签移除本次容器和测试卷。

在 Docker Desktop 上通过入口启动真实 MySQL 8.4 并查询版本，分别验证正常退出 0、测试失败退出 37 和收到 SIGTERM 后退出 143。三种情况下本次容器和卷均已回收。两批并行验收分配了不同的端口和数据卷，先退出的一批未影响另一批。挂到验收容器的既有卷保留，显式标记的测试卷被回收。现用 MySQL、PostgreSQL 和 Redis 的容器 ID、启动时间和挂载均保持一致，两个 SQL 服务均返回 `SELECT 1`，Redis 返回 `PONG`。验收资源回收后带归属标签的卷为 0，宿主可用约 62.2 GiB。并发任务仍在写盘，整机差值无法归因于本批。

## Reverb 的 Redis 重启测试

当前工作树只发现 Reverb 脚本在验收入口之外直接调用 Docker。它此前允许按调用方提供的容器名重启 Redis。现在由 `docker/accept.py --mysql --redis` 创建测试服务，Reverb 核对本次容器名和归属标签后，按容器 ID 重启。Docker Desktop 实测会在重启时更换自动发布的端口，因此入口为本批 Redis 显式发布一个空闲回环端口。

在隔离 Cargo 目标目录用 Rust 1.94.0 编译带 `reverb` 特性的应用，完整运行 `tests/reverb_runtime.py` 通过。覆盖 HTTP/WebSocket、Redis 跨进程投递、共享 MySQL 和 Redis 的三节点集群、Redis 重启后重新订阅。定向检查验证现有 Redis 的重启请求被拒绝，临时 Redis 重启后原端口和测试数据仍可用，正常退出、失败和 SIGTERM 均回收本批卷。复查归属标签下的容器和卷均为零。现有 MySQL、PostgreSQL、Redis 的启动时间和数据挂载一致，连接检查仍通过。
