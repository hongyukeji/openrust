# 日常开发切换 PostgreSQL 18 与 Redis 8

2026-11-20 用户将日常环境指定为 Rust 1.94＋PostgreSQL 18＋Redis 8，覆盖此前 MySQL 8.4 日常规则；发布兼容矩阵仍保留。实现与本批受影响路径验收完成，不等于整个发布矩阵完成。

## 配置与实现

- 根 `rust-toolchain.toml` 固定 1.94.0、minimal、rustfmt／clippy；Cargo 的既有最低版本和框架默认 SQLite 保持原契约。
- 本机已有 postgres:18 和 redis:8 服务，实测 PostgreSQL 18.6（server_version_num=180006）、Redis 8.10.3。新建独立 openrust 登录角色与同名开发库，随机密码仅写本地忽略的 `.env`；开发连接 127.0.0.1:5432，Redis 127.0.0.1:6379，缓存／队列选 Redis，Session 保留数据库。原根应用配置实际为 SQLite，未将其误记为 MySQL 数据迁移。
- 旧 `.env` 私密备份于本批目录，目录权限 0700、环境文件权限 0600。原 SQLite／MySQL 数据和既有运行服务保留；已启动的旧服务需重启才能载入新配置，不擅自终止现有用户进程。本批 HTTP 使用独立端口和专属进程。
- 新增 `tests/postgres_acceptance.py`，沿用已有 MySQL 入口的批次标签、临时凭据、随机回环端口、取消与进程组清理机制；核对 PostgreSQL 18 镜像声明的 /var/lib/postgresql，用 1 GiB tmpfs 覆盖包含版本化 PGDATA 的目录。以 TCP 实查版本后注入 PostgreSQL 专属变量。保留 MySQL 入口供兼容专项。
- 更新开发规范、上下文、计划和闭合账本。MySQL 专用测试需按实际影响逐项适配，不只替换变量，不把跳过记成通过。旧版 v0.x 的工具链文件为 stable，本次按用户明确版本固定，不复制旧版选择。

## 实际验收

所有 Cargo 在 1c5f1b8b6 的独立 git archive 加本批工具链／Python 文件、私密环境配置中运行；源码 `/private/tmp/openrust-postgres-dev-20261120/source`、独占同批 target，Rust 1.94.0、3 jobs、增量关闭、dev/test debug=0。后续协作者文档／依赖改动未混入此快照。

- `build.log`：`cargo +1.94.0 build --locked --bin main` 成功。
- `lifecycle.log`：真实 PostgreSQL 的成功、退出码37、SIGTERM取消、并发另一实例存活、tmpfs及容器回收通过（1测试包含全部分支）。
- `checks.log`：全仓 fmt 检查；隔离 PostgreSQL 上 JSON 谓词／更新，覆盖 json/jsonb 及两种预备模式，2函数实际通过；根配置1函数通过。无 Rust 源码修改，未重复全 workspace Clippy。
- `runtime.log`：实际开发库13条迁移、状态查询及 Artisan inspire 成功；根首页与 /up 均200，数据库 Session 实际落1行，专属 HTTP 进程正常退出。
- `redis.log`：独立 Redis 8 tmpfs 实例，命名连接、鉴权、缓存、跨入口／进程锁与 HTTP 的既有根测试1函数通过，不在开发 Redis 修改 ACL 或清库。早期夹具将 Redis 镜像误假设为必须声明 /data 卷，实际无声明；修正为仅允许无声明或 /data，运行时始终显式挂载 tmpfs，失败日志保留。

## 范围与收尾

本批只证明日常环境及上述路径可用，认证全扩展在 PostgreSQL 的完整生成应用、Windows／Linux 浏览器、全平台／数据库发布整合和真实外部供应商仍按原门禁推进。未搬迁旧数据库业务数据，未推送、改发布版本或部署。

日志及元数据保存在本批私密目录；[脱敏验收元数据](development-postgres-validation.json)记录日志 SHA。构建 target 已回收，逻辑 3,294,876 KiB，df 实际增加3,046,984 KiB，剩余91,289,040 KiB，高于停线。临时 PostgreSQL／Redis 容器均收尾，原有全部数据卷保留；源码、日志、共享缓存和开发库保留。其他协作者 Cargo／文档改动及未知 WIP 不纳入本批提交。
