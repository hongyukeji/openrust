# 六处分层整理的实施验收

固定行为与裁决依据见[静态复核](remaining-models-2026-11-24.md)和[项目规范](../../conventions.md#模型数据库访问与业务服务分层)。本轮仅调整实现归属，保留公开导入、覆写钩子和原有事务、事件、错误语义；不执行 PHP、不引入新的服务抽象体系。S01／S02 不在本轮六项范围内。

| 项目 | 状态 |
| --- | --- |
| M01 Activitylog | 已验收 |
| M02 数据库通知 | 已验收 |
| M03／M04 Fortify | 已验收 |
| M05 Passkeys | 待验收 |
| M06 Cashier | 待验收 |

## 第一批：Activitylog 与数据库通知

### 实现

- `models::concerns::LogsActivity` 的公开路径保持原位，定义移入同路径的子模块；模型选项、属性提取及业务钩子保留。`model_snapshot` 承接当前行与关联快照、审计数据准备，`model_observer` 承接监听、缓冲及提交后写入。原同连接／跨连接、保存点与事务处理顺序不变。
- 通知的 `DatabaseRoute::query/find`、`DatabaseNotification::schema`、序列化及已读状态方法保留。内部 `database_store` 承接查询、行映射、插入及更新；记录仅在受影响行大于零时同步 `read_at`，保留失败与记录已删除时的状态。
- 三语 Activitylog 与通知指南同步说明应用模型职责、事务边界及通过公开状态方法更新通知。

### 验收

Rust 1.94.0；隔离源码仅由已提交基线与本批文件组成，不纳入其他协作者的在途修改。项目内 TMPDIR 和独占 Cargo target 由 `tests/build_guard.py` 设置，沿用正常增量与并发。每个驱动分别运行：

```sh
cargo test --locked -p openrust-activitylog --test activitylog
cargo test --locked -p openrust-framework --test notification_store
```

- SQLite、隔离 MySQL 8.4.13、PostgreSQL 18（180007）：Activitylog 各 10 项通过；通知各 1 项通过。通知测试在真实 TCP／Reqwest HTTP 与正式 `Application::handle_command` Artisan 内，各覆盖字符串和整数主键、具名连接、独立表前缀、收件人隔离、即时及队列载荷投递、幂等标记、记录被删除及 SQL 错误后的内存状态。
- SQLite `activitylog --test provider` 7 项通过，覆盖自定义模型绑定、HTTP Kernel、清理命令及 Artisan 成功／非零退出／错误后的缓冲刷新。
- `mail_notifications` 精确运行 `database_notification_is_polymorphic_localized_and_cancellable`，SQLite 与 MySQL 分别通过；MySQL 显式映射 `OPENRUST_TEST_DELIVERY_MYSQL_URL`，未将缺服务的返回计为通过。
- 合计 42 项实际驱动／入口执行通过。`cargo fmt --all --check` 和 `cargo clippy --workspace --all-targets --locked -- -D warnings` 通过。三语内容、正式命令入口、VitePress 构建及 `/`、`/docs/` 的三语桌面／移动端浏览器检查通过。

新增通知夹具最初在同一连接上重建同名表并切换主键列类型，PostgreSQL 的预编译参数类型缓存导致 `bigint = text`；改为独立具名连接和表前缀后复验通过，没有更改驱动或降低通知 schema。一次全仓格式检查纳入了其他协作者未完成的 Vite 测试，随后改用仅包含基线和本批差异的隔离快照重新检查，未修改该在途文件。

磁盘曾触及32 GB停线，守卫停止验收后持锁清理本项目编译产物，释放 62,880,169,984 字节（约58.6 GiB），清理后可用 94,471,995,392 字节（约88.0 GiB）；保留锁文件 inode、源码、未提交修改、日志、依赖和数据卷。之后使用本批独占 target 复验。数据库夹具回收本批容器；文档产物和失效临时夹具在批次结束清理，可复用缓存与日志保留。

## 第二批：Fortify 重置与邮箱验证

- 现有 `CompletePasswordReset::handle` 负责 token 生成、完成步骤与 `PasswordReset` 事件；注册适配器只对实际模型设置输入并保存。保留原先保存返回值的处理与保存失败传播，不额外修改事件条件。
- 邮箱适配器提供实际 `MustVerifyEmail` 的可变投影；Fortify 内部验证动作负责状态检查、状态方法调用与 `Verified` 事件。控制器在动作完成后仍将同一更新后的账户放回 Session，签名／限流／CSRF 校验不变。
- Rust 1.94.0、MySQL 8.4.13 隔离实例：`password_reset_http` 6 项、`email_verification_http` 4 项、`provider` 3 项、`completion_artisan` 1 项，共14项通过。HTTP 覆盖自定义动作与响应、保存／事件失败、重复／错误／过期验证、并发 token claim 及 Session；新增 Artisan 用例通过真实默认 User 和公开 CompletePasswordReset 验证先落库再记录事件，保存失败不增加完成事件。
- Artisan 完成动作使用独立 SQLite；此批没有新 SQL 方言实现，未新增 PostgreSQL Fortify HTTP 验收结论。纯 Rust 实现，无 PHP 运行依赖。
- 隔离源码继续只同步本批文件；全仓格式与 workspace／all-targets 严格 Clippy 通过。三语 Fortify 指南说明自定义密码动作与完成步骤的分工、验证状态返回值与事件顺序；内容、命令入口、站点构建和两种挂载路径下的三语桌面／移动端检查通过。
- 本批数据库容器已回收，测试临时应用由夹具清理；文档产物在收尾回收，保留日志与后续批次复用的独占构建缓存。不推送。
