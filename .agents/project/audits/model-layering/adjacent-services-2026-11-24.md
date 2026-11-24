# Cashier 订阅选择与 Activitylog 日志准备

2026-11-24。按[分层规范](../../conventions.md#模型数据库访问与业务服务分层)收敛相邻服务职责；承接[六项实施](implementation-2026-11-24.md)与[可选整理](optional-2026-11-24.md)。本批只处理用户选定的两处，Pay 的事务型通知实现保留。扩展仍归所属 vendor crate，不修改应用安装产物。

## 实现与保留契约

- Cashier：`BillingStore::subscription` 保留客户隔离、SQL 查询、逐项解码与原错误映射；解码完成后委托 `billing::select_subscription`。选择规则原样移动：排除 `canceled`／`cancelled`／`incomplete_expired` 后，多个候选报原错误，单个优先返回；全部为终止候选时按 `(created_at, id)` 取最大项，无候选为 `None`。不以 `Subscription::active()` 替代既有状态集合，不提前跳过坏载荷，也不改变返回的 Subscription／SubscriptionHandle 或公开方法。
- Activitylog：私有 `preparation` 模块承接 Redaction 和记录准备，Repository 保留配置及 `prepare/log/log_in`。原 Redaction 根导出继续通过 Repository 重导出，无公开入口迁移。结构校验 → 启用检查 → 可选白名单／递归脱敏 → 字段与默认时间准备 → 可选大小限制的顺序不变；SQL 时间转换与 PreparedActivity 的模型存储适配绑定继续留在 Repository，位于准备完成后。普通、显式事务及缓冲路径均经过原准备入口，不只在 Logger 路径应用策略。
- 沿用纯 Rust 与既有模块，不增加运行期依赖、平行服务体系或新业务行为。Activitylog 的 axum 仅为真实 TCP 消费者增加既有开发依赖，Cargo.lock 不升级依赖版本。
- 复用前轮固定基线调研与当前实现／消费者；旧版 `openrust-v0.x` 未找到这两个包的同目录实现，不引入无关旧行为。本次属于职责移动，当前公开契约和原执行顺序是回归依据，不新增上游对齐或托管支付验收结论。

## 验收范围

Rust 1.94.0；使用已提交基线加本批文件的隔离快照，其他协作者在途修改未纳入。临时文件与独占 Cargo 缓存统一位于项目 `target/tmp/model-layering-optional-20261124/`，命令经过 `tests/build_guard.py`；保留正常增量与并发。

- Cashier：扩展既有真实 TCP HTTP 与原生 Artisan 消费者，分别从公开 Store／Billable 入口验证空集、终止状态、同时间主键排序、past_due／paused 优先、同名冲突、坏载荷与缺客户；Stripe／Paddle 共用相同断言。保留协议模拟、Webhook 事务／重试／重复处理及持久化回归。
- Activitylog：新增同一原生应用的 Artisan 迁移、准备探针、真实 TCP HTTP Kernel 请求及回滚入口。探针验证直接 `prepare/log/log_in`、PreparedActivity 事务写入、白名单和嵌套秘密脱敏、无策略原值、显式时间、无效结构先于禁用检查、禁用不落库、大小限制、无效 SQL 时间及事务回滚。原模型事件、缓冲与 Provider 用例继续复跑。
- 三语指南说明可观察的选择规则和日志准备契约；计费指南中原落入 Rust 代码块的一段说明移回正文。

## 实际结果

```sh
cargo test --locked -p openrust-cashier --tests
cargo test --locked -p openrust-activitylog --tests
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- SQLite 3.54.1：Cashier 16项、Activitylog 25项通过，共41项；其中 Activitylog Provider 固定使用 SQLite。Cashier 原有1项要求外部固定 PHP SDK 生成新签名的历史测试继续显式 ignored，不计通过；其他 fixed_php 名称的用例只读取既有夹具，本批没有执行 PHP。
- 独占 MySQL 8.4.13、PostgreSQL 18（180007）：每库执行 Cashier `artisan/callback_http/persistence` 3项，Activitylog `activitylog` 10项及新增 `preparation` 1项，各14项通过。实际运行驱动通过 `CASHIER_TEST_DRIVER`／`ACTIVITYLOG_TEST_DRIVER` 选择；隔离实例由项目 mysql／postgres acceptance 脚本提供，不连接开发库。
- 合计69项实际原生驱动／入口执行通过；真实 HTTP、Artisan 和各自业务／存储边界均已覆盖，不将浏览器内部断言或 ignored 项累加为测试数。Stripe／Paddle 使用真实回环 HTTP 协议模拟，没有调用托管支付或真实资金接口。
- 最终隔离快照的全仓格式和 workspace／all-targets 严格 Clippy 通过；本批实现、测试与三语正文逐文件和受检快照一致。Cashier 原选择步骤另与改动前源码逐项核对一致。
- 三语内容、命令入口、VitePress 构建，以及 `/`、`/docs/` 下三语桌面／移动端浏览器检查全部通过。计费说明已移出代码块，公开用法保持原有 API。
- 新增夹具最初使用了模拟服务尚未支持的客户 ID，补充测试服务对应响应后复验；另一次给原生 `handle_command` 多传了程序名，修正夹具参数后复验。两次均未修改生产协议或命令契约。

验收日志保存在 `target/tmp/model-layering-optional-20261124/adjacent-*.log`。两组独占数据库容器已回收，开发服务和数据卷保留。本批结束清理隔离源码、临时应用、文档构建及一次性编排脚本，保留日志与可复用 Cargo 缓存。只提交本批实现、测试、三语指南及记录；其他协作者的在途修改保留，不推送。

## 本次静态复核结论

2026-11-24，在 `bf0e6b814` 整理后按当前源码再次复查根应用和 vendor 包的生产模型、模型能力 trait、Record／注册适配器及直接服务消费者。逐项追踪实际委托与调用链，不以关键词命中或目录名代替职责判断。本次未发现范围内新增需调整的分层问题，也未发现已整理职责回流；这不是对全部框架服务或全部公开契约重新完成行为验收的声明。

### 已完成项

| 项目 | 当前职责边界与实施依据 |
| --- | --- |
| User、Sanctum、自定义 Activity 存储适配 | User 保留映射与认证能力，条件更新委托框架；Sanctum 保留可覆写模型查找入口，存储、事务锁和快照独立；Activity 的模型存储适配独立并保留原导出。见[首轮验收](README.md)。 |
| Activitylog 监听与数据库通知 | 模型日志能力保留选项／钩子，监听、快照和提交后调度独立；通知记录保留公开状态方法，查询、行映射和条件更新委托内部 Store。见[六项实施验收](implementation-2026-11-24.md)，提交 `e61dc0279`。 |
| Fortify 密码重置与邮箱验证 | 完成步骤和业务事件由 Actions 负责；注册适配器保留模型投影、字段设置及持久化。见六项实施验收，提交 `125abdb72`。 |
| Passkeys 与 Cashier 业务／存储编排 | Passkeys 校验规则在 Actions，实际模型锁定、刷新及保存保留原事务保护；Cashier 远端调用和状态裁决归 billing／webhook，Store 负责锁、操作占用及读写。见六项实施验收，提交 `f3c15d1ad`。 |
| Scout 批量导入与 Fortify 恢复码 | 公开模型入口分别委托 importer 和 Actions，保留模型覆写、筛选／分批、加密／保存及事件顺序。见[可选整理验收](optional-2026-11-24.md)，提交 `353f3ca0b`、`0eeb12e3a`。 |
| Cashier 订阅选择与 Activitylog 日志准备 | 选择规则在 billing，Store 保留查询和解码；日志准备／脱敏在 preparation，Repository 的直接与间接入口均经过原准备路径。见本页实施与实际结果，提交 `bf0e6b814`。 |

复核还覆盖 Passport 的 Client（含私有 ClientAttributes）／Token／RefreshToken／AuthCode／DeviceCode、ClientRecord，以及 Passkey 模型、Settings 和 Pennant 的直接模型消费者。关系、属性转换、模型查询作用域及单实体状态保存属于允许的 Active Record 行为；Record 的类型转接、实例身份、锁定和持久化适配可以保留。未据此新增空 Service 或继续机械拆分。

### Pay 暂时保留的事务边界

后续状态：用户已继续授权[Pay 通知流程与内部存储整理](pay-notifications-2026-11-24.md)，实施与实际验收结果见该记录；以下保留静态复核时的裁决和事务要求。

唯一按用户决定暂时保留的组合实现是 [pay/src/notifications.rs](../../../../vendor/openrust/pay/src/notifications.rs) 的 `DatabaseNotifications::process`。它是事务型通知服务与数据库存储的组合，位于独立 vendor 包中，不是应用模型中的业务流程；本次不将其记作已经拆分，也不新增 Pay 验收结论。

- 已验签通知与可信订单的匹配校验在开启事务前执行。
- 去重记录占用、指纹检查、行锁、完成状态检查、应用 SQL 回调和 `done` 标记维持同一事务；指纹冲突或已完成的重复通知不调用业务回调。
- 回调或事务内 SQL 失败回滚本次事务，不能将回调或完成标记移到事务外而破坏原子性。
- 应用回调仅执行所提供事务中的 SQL；通知、发货等外部副作用由应用的事务 outbox／提交后路径处理。
- 后续如需整理，只在保留公开入口、上述顺序及错误语义的前提下抽取内部存储操作；当前建议保留现有实现。

### 源码归属与本轮检查

Cashier、Activitylog、Fortify、Passkeys、Passport、Scout 等实现属于各自 `vendor/openrust/<package>/` crate。当前 Sanctum 源码实际位于 `vendor/openrust/framework/src/sanctum/` 可选模块；其安装器发布到根应用的配置、迁移和 User 接入才是消费者产物，不能与包源码混同。当前 User、应用 Provider 及路由接入中未发现本轮应清理的扩展安装残留；基础 API 配置与空路由入口不作为残留处理。

本次只做静态源码复核及记录更新，没有修改业务代码，也没有重新执行 Rust、HTTP、Artisan 或数据库测试。上文69项等测试数仍属于各次实施时的验收，不计作本轮新增结果。本轮检查记录的相对链接、复核章节锚点与 `git diff --check`；只本地提交记录，不推送，其他协作者改动保留。
