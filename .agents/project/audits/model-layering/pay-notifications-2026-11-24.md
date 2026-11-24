# Pay 通知流程与内部存储分离

2026-11-24。用户继续授权处理[静态复核中暂时保留的 Pay 边界](adjacent-services-2026-11-24.md#pay-暂时保留的事务边界)，遵循[项目分层规范](../../conventions.md#模型数据库访问与业务服务分层)。本批修改所属 `vendor/openrust/pay/` 包，不修改根应用接入或安装产物。

## 实现与契约

- `DatabaseNotifications::new/process`、回调参数及 Future／Result 契约、NotificationOutcome 与公开迁移类型保持原位。私有 `notification_store` 仅承接事务内的占用及加锁读取指纹、读取完成状态和写入完成标记；不自行获取连接、创建事务或调用应用处理器。
- 通知流程仍先匹配可信预期订单，再通过原 Connection 开启一次事务，保持原重试参数1。事务内依次生成 key／fingerprint → 无变化 upsert 占用 → `lock_for_update` 读取指纹 → 冲突判定 → 读取完成状态 → 重复判定 → 应用 SQL 回调 → 完成更新；没有合并查询、提前读取状态或缩短锁周期。
- 同身份异内容保留原 NotificationConflict 路径，重复保留 Duplicate，成功为 Processed；冲突仍在事务返回后转换为领域错误，未改成事务内抛错。事务内 SQL／回调失败仍回滚并映射 Error::Database，取消保持原事务清理。完成更新为零行的原返回语义保留，不新增成功条件。
- 复用前轮静态调研、当前通知实现与真实 HTTP／三库消费者；旧版 `openrust-v0.x` 未找到同包实现，不替换为无关的通用通知机制。纯 Rust，无新运行期依赖；新增 Artisan 消费者仅使用已有 clap 的开发依赖，锁文件不升级版本。

## 验收范围

- 既有 `notifications`：迁移、业务失败／取消回滚、8个并发同身份通知只执行一次、重复、金额不匹配与身份复用冲突。
- 既有真实 TCP `callback_http`：微信／支付宝签名通知篡改400、事务失败500、提交后各自原生确认、投递重试与重复，核对业务表和收件箱。
- 新增测试应用内登记的原生 Artisan `pay:notifications-probe` 消费者：包 Provider 迁移、微信／支付宝通知、订单不匹配不占用、回调先读 pending、完成标记写入失败时业务与收件箱一起回滚、失败后重试成功、完成后重复及指纹冲突不执行回调，以及完成更新零行的原返回值。完成失败由各驱动独占测试数据库中的触发器模拟，不修改生产 SQL 或驱动。
- 三语 Pay 指南说明订单匹配时机、同一事务及完成写入失败回滚。既有外部副作用由应用 outbox／提交后处理的契约保持不变。

## 实际结果

隔离源码由已提交基线 `70a076dab` 加本批文件组成，未纳入其他协作者的配置／验证器等在途修改。Rust 1.94.0，所有编排通过 `tests/build_guard.py` 使用项目内 TMPDIR 和复用的独占 Cargo 缓存，保持正常增量与并发。

```sh
cargo test --locked -p openrust-pay --tests
cargo test --locked -p openrust-pay --test artisan --test callback_http --test notifications
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- SQLite 3.54.1：完整包测试13项通过，包含协议、失败分类、真实 HTTP、Artisan 与事务；没有忽略项。
- 独占 MySQL 8.4.13、PostgreSQL 18（180007）：分别执行 `artisan/callback_http/notifications`，各3项通过，没有忽略项；由 `PAY_TEST_DRIVER` 选择实际驱动，项目 acceptance 脚本创建并回收独占容器，不访问开发库。
- 合计19项实际原生驱动／入口执行通过。各库均验证回调成功但完成写入失败时，两张表一起回滚，移除故障后重试成功；新命令同时验证微信和支付宝。原 HTTP 消费者验证原生确认响应只在提交后发送；原事务用例验证取消回滚和8并发同身份只执行一次。
- 三语内容、命令入口、VitePress 构建及 `/`、`/docs/` 下三语桌面／移动端浏览器检查通过；隔离快照的全仓格式和 workspace／all-targets 严格 Clippy 通过。本批实现、测试和三语正文与最终受检快照逐文件一致。
- 全部为纯 Rust／本地数据库及真实回环 HTTP。带 php_openssl 名称的用例读取已有固定签名／密文夹具，没有执行 PHP 或新增外部 SDK 依赖，也没有访问真实商户或资金接口。

本项已按范围实施并验收，取代前次“Pay 暂时保留”的当前状态；前次裁决保留作历史依据。不存在本次已识别但未处理的分层事项，不据此宣称全部框架服务重新完成行为审计。

验收日志位于 `target/tmp/model-layering-optional-20261124/pay-*.log`。两组隔离数据库容器均已回收，开发服务与数据卷保留；收尾清理本批源码快照、临时应用、文档产物和一次性编排脚本，保留日志与可复用构建缓存。只本地提交本批实现、测试、三语指南及记录，不推送。

## Pay 整理后的静态分层复核

2026-11-24，在实现提交 `9135ea0b4` 后，按当前源码复核生产模型、模型能力 trait、Record／注册适配器、内部存储操作及直接服务消费者。结论：**Pay 整理已完成并按前述范围验收；本轮未发现新增待整改项，也未发现已整理职责回流或遗漏。** 该结论限定于上述分层复核范围，不代表全部框架公开契约再次通过行为验收。

- [notifications.rs](../../../../vendor/openrust/pay/src/notifications.rs) 保留公开通知入口、订单匹配、事务编排、冲突／重复裁决及业务回调；[notification_store.rs](../../../../vendor/openrust/pay/src/notification_store.rs) 仅使用传入的事务执行占用、加锁读取、完成状态读取与更新，不自行开启事务或调用业务回调。去重、行锁、业务回调和完成标记仍在同一事务内。独立 Migration 实现属于迁移职责，不作为模型或通知流程越层处理。
- Cashier、Activitylog、Fortify、Passkeys、Passport、Scout 的既有委托边界保持完整；User、模型关系和自身状态方法，以及适配器中的类型转接、实例身份、锁定和持久化符合分层规范。Settings、Pennant 的模型消费者继续委托 Repository／Driver。当前没有据此需要继续拆分或新增转发层的具体文件。
- 独立扩展源码归所属 `vendor/openrust/<package>/` crate；当前 Sanctum 源码位于 framework 的可选模块。安装器发布到根应用的配置、迁移和模型接入属于消费者产物。应用接入中未发现本轮应处理的扩展安装残留，基础 API 配置和空路由入口不算残留。

本节记录的是**静态复核**；上文19项三库测试、HTTP／Artisan、严格 Clippy 和三语文档检查属于实施提交时的**既有行为验收**，不重复计作本轮结果。本轮未修改业务代码，未重新执行 Rust、HTTP、Artisan、数据库或文档站构建测试；仅核对记录内容、相对链接、章节锚点、来源提交及 `git diff --check`。本次只本地提交记录，不推送，保留其他协作者改动。
