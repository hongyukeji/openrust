# 其余模型与服务的静态分层复核

2026-11-24。延续 `63e6d328a` 的分层整理，按[项目分层规范](../../conventions.md#模型数据库访问与业务服务分层)核对生产模型、模型能力 trait、模型／Record 适配器及直接服务消费者。本轮只修改规范与审查记录，不重构下列代码，也不把上一轮 HTTP／Artisan 结果当作本轮行为验收。

## 范围与裁决方法

- 以 `app/`、`vendor/openrust/*/src/` 中的生产声明建立候选，随后阅读实现并追踪消费者；排除测试夹具和安装生成的应用侧残留。ServiceProvider 负责装配，不因名字含 Service 就认定是业务服务。
- 核对的实际 Eloquent 模型包括 User、Activity、DefaultPersonalAccessToken、Passkey，以及 Passport 的 Client／内部 ClientAttributes、Token、RefreshToken、AuthCode、DeviceCode；同时检查 Fortify、Sanctum、Scout、Pennant 的模型能力入口，通知记录和 Passkeys／Passport 的 Record 包装。
- 核对直接相关的 Fortify Actions／控制器、Passkeys Actions／Store、Passport Repository／Store／AuthorizationServer、Settings Manager／Repository，以及 Cashier 的 Billable／BillingStore／数据类型。此范围不等于对全部框架服务每个方法完成了行为审计。
- 判断的是实现职责与文件边界。模型自身的关系、属性、状态方法及 `save()` 委托可以保留；下面的问题是具体存储实现或成套流程仍混在定义／适配文件中，或服务职责反向进入存储层。静态证据不证明存在数据损坏、安全漏洞或运行失败。

固定基线参考：Framework [DatabaseNotification](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Notifications/DatabaseNotification.php) 保留模型已读状态方法；Fortify 1.43.0 的 [TwoFactorAuthenticatable](https://github.com/laravel/fortify/blob/3db382a3d5d6a0d537de32a190ce3b945e621713/src/TwoFactorAuthenticatable.php) 将恢复码替换作为模型能力。分层调整只委托内部实现，不能删除这些公开方法或改变顺序。

## 建议优先收敛的具体位置

### M01：Activitylog 模型文件仍包含事件处理与存储调度

- 文件：[models.rs](../../../../vendor/openrust/activitylog/src/models.rs)，重点为 `build_stored`（313 行）、`load_related`（352 行）、`save_after_commit`（372 行）和 `LogsActivityListener`（400 行）。
- 调用链：`LogsActivity::boot_logs_activity` 注册监听器 → `handle`／`handle_in_transaction` → 通过主键读取数据库当前行和关联 → 构造审计数据 → 缓冲、事务内保存或提交后保存。
- 问题：上轮只移走了自定义 ActivityModel 的存储适配；该文件中的 `tx.table(...)`／`get_connection().table(...)`、关联加载、事件处理和提交后调度仍同时存在。日志模型能力与监听／持久化编排尚未完全分开。
- 建议：保留 `models::concerns::LogsActivity` 及其覆写钩子；将监听器放到独立 Observer／listener 模块，将当前行与关联快照读取放到内部存储辅助模块，提交后缓冲／写入复用 Repository 和 Buffer。按现有调用层次拆，不为每个辅助函数新建服务类型。
- 后续验收：同连接事务、跨连接提交后写入、嵌套保存点回滚、缓冲去重、旧关联与当前关联、模型事件顺序；复跑三库 `activitylog` 和 HTTP／Artisan `provider` 消费者。

### M02：数据库通知记录混合行映射与具体数据库操作

- 文件：[database_notification.rs](../../../../vendor/openrust/framework/src/notifications/database_notification.rs)，`DatabaseRoute::table`（66 行）、`find`（72 行）、`DatabaseNotification::set_read_at`（154 行）、`insert`（172 行）。
- 调用链：`DatabaseChannel::send`／`send_queued` → 通知记录 `insert`；消费者通过 `DatabaseRoute::find` 读取记录，再调用 `mark_as_read`／`mark_as_unread` 更新状态。
- 问题：同一文件兼有身份路由、记录定义、行解码、连接选择、插入及条件更新。公开状态方法本身合理，具体查询与映射实现可以独立。
- 建议：在 notifications 内部抽出 database store／repository，承接查询、行映射、插入与已读状态持久化；保留 `DatabaseRoute::query/find`、`DatabaseNotification::schema`、标记方法及序列化契约。无需为了目录形式改造成另一套公开模型 API。
- 后续验收：具名连接、表前缀、整数／字符串多态主键、跨收件人隔离、重复标记、记录被删除、SQL 失败后的内存状态、即时／队列通道；覆盖三库与 HTTP／Artisan 消费者。

### M03：Fortify 密码重置完成流程进入模型注册适配器

- 文件：[password_reset_model.rs](../../../../vendor/openrust/fortify/src/password_reset_model.rs)，`ModelAdapter<T>::complete`（23 行）；消费者为 [actions/mod.rs](../../../../vendor/openrust/fortify/src/actions/mod.rs) 的 `CompletePasswordReset::handle`（225 行）。
- 调用链：业务 Action 查注册表 → 适配器完成类型转换 → 生成并设置 remember token → `save()` → 派发 `PasswordReset`。
- 问题：业务 Action 主要做注册解析，真正的完成流程却在适配文件中。它不是应用模型定义，但存在适配职责与业务流程混合。
- 建议：让现有 `CompletePasswordReset` 承担 token 生成与事件顺序；适配器只负责把输入应用到具体模型并持久化，避免新增平行的重置服务体系。
- 后续验收：自定义模型、实际连接和主键、应用重置 Action 前后顺序、token 轮换、观察器与事件、持久化失败、HTTP 重置入口及 Artisan 消费者；保留现有返回值处理语义，不顺带调整事件条件。

### M04：Fortify 邮箱验证适配器承担验证与事件编排

- 文件：[email_verification_model.rs](../../../../vendor/openrust/fortify/src/email_verification_model.rs)，`TypedAccount<T>::verify`（47 行）；消费者为 [email_verification.rs](../../../../vendor/openrust/fortify/src/http/controllers/email_verification.rs) 的控制器 `verify`（67 行）。
- 调用链：控制器取认证账户 → 注册适配器 → 检查验证状态 → `mark_email_as_verified` → 派发 `Verified` → 更新当前 Session 用户。
- 问题：类型擦除适配文件中实现了验证成功后的业务事件决策；这与模型本身的状态方法是两个职责。
- 建议：将“检查、调用状态方法、决定是否派发事件”放到 Fortify 的内部验证 Action／服务函数；适配器保留账户转接、主键、邮箱和模型操作的类型适配。不能直接替换为另一 HTTP 请求入口而重复校验或派发事件。
- 后续验收：重复链接、邮箱大小写或内容变化、状态更新失败、并发验证时事件次数、自定义模型、Session 用户更新及签名过期／错误的真实 HTTP 边界。

### M05：Passkeys Record 适配器中隐藏了认证校验流程

- 文件：[record.rs](../../../../vendor/openrust/passkeys/src/record.rs)，`Loaded<M>::verify`（108 行）；消费者为 [actions.rs](../../../../vendor/openrust/passkeys/src/actions.rs) 的 `VerifyPasskey::call`（118 行）。
- 调用链：Action 按 credential_id 查记录并开事务 → Record 锁住具体模型、`refresh_for_update` → 校验归属用户 → WebAuthn 验证并更新凭据 → 更新时间并在事务内保存。
- 问题：Record 本来承担实际模型实例的类型擦除与保留，但这里同时做认证决策、调用 WebAuthn 和持久化，Action 与存储适配的边界不清晰。
- 建议：保留 `PasskeyRecord` 的公开投影与实例身份；认证规则和 WebAuthn 调用归已有 VerifyPasskey Action／内部认证辅助函数，模型锁定、刷新及保存归 Store／持久化适配。拆分后仍须让校验与计数器更新处于原来的同一锁和事务保护范围，不能简单把校验移到锁外。
- 后续验收：凭据并发／重放、计数器、认证过程中删除或换归属、自定义 Passkey 模型、失败回滚、模型事件与响应中的实例身份、真实 HTTP 登录／确认及 Artisan 消费者。

### M06：Cashier 的存储层反向包含远端调用与业务状态机

- 文件：[store.rs](../../../../vendor/openrust/cashier/src/store.rs)，`BillingStore::execute`（95 行）、`sync_subscription`（242 行）、`process`（273 行）。
- 调用链：Billable 的修改流程 → `BillingStore::execute` → 选择 Stripe／Paddle 策略、占用幂等操作、调用远端 Client、裁决 done／rejected／unknown 并回写；Webhook 流程通过 Store 开事务、查询远端资源、同步订阅／账单并执行应用回调。
- 问题：属于服务与存储混合，未发现这些逻辑塞进 Subscription 模型。`BillingStore` 既实现 SQL，又编排支付渠道、远端请求、事件处理和幂等状态机，职责偏大。
- 建议：保留既有 BillingStore 公开入口并按需委托；由现有 billing／webhook 服务模块承接远端流程，Store 承接操作占用、状态落库和事务内数据写入。先保持幂等和事务语义完成结构拆分；HTTP 是否移出锁区、如何重试属于独立行为优化，不在同批机械改动。
- 后续验收：Stripe／Paddle 协议模拟 HTTP、同幂等键并发、远端成功但落库失败、unknown 状态、重复／冲突 Webhook、跨客户隔离、事务回滚和 Artisan 消费者；不能用当前静态结论代替真实支付平台验收。

## 可选的文件组织改善

- **S01 Scout**：[searchable.rs](../../../../vendor/openrust/scout/src/searchable.rs) 的 `import`（176 行）负责分页取模型、过滤并调用 EngineManager 同步，且由 Searchable 默认入口和 EngineManager 注册的导入闭包共同消费。可移到独立导入辅助模块，使该文件聚焦模型契约；保留 `make_all_searchable`、批量 hook、软删除、chunk 和失败传播。属于内部文件组织改善，不认定所有 Searchable 查询都是业务越层。
- **S02 Fortify 恢复码**：[two_factor_authenticatable.rs](../../../../vendor/openrust/fortify/src/two_factor_authenticatable.rs) 的 `replace_recovery_code`（28 行）包含生成、解密／替换／加密、保存与事件。固定上游本身把此方法放在模型 trait 中，因此保留是合规的；如需与包内双因素 Actions 统一，可将内部步骤委托给辅助实现。不能将它列为缺失 Service 的硬性缺陷，也不能删除方法。现有 Enable／Confirm／Disable／Generate 流程已经在 Actions。

## 本轮无需为分层强拆的实现

| 模块 | 已核对的职责边界 |
| --- | --- |
| 根 User | 没有直接数据库更新，认证状态方法委托框架辅助实现；字段生成区域仍在模型顶部。 |
| Sanctum | 模型与可覆写凭据查找入口保留，Store／事务锁／快照已经独立；HasApiTokens 主要委托 Store 与关系接口。 |
| Passport | Client 属性转换、关系、scope／grant 判断属于模型行为；Token／RefreshToken 的自身 revoke 通过普通模型生命周期保存。OAuth 授权流程在 AuthorizationServer，访问令牌／授权码等存储在 Repository／Store。ClientRecord 的模型投影与持久化适配属于适配职责，未见 Passkeys 那样的 WebAuthn 成套认证流程。 |
| Passkey 模型定义 | `passkey.rs` 聚焦映射、属性转换、认证器展示和关系；具体查询／创建已在 `store.rs`。M05 针对的是 Record 认证流程。 |
| Settings | `Settings<T>` 处理状态与验证并委托 Repository；Manager 负责协调，DatabaseSettingsRepository 实现 SQL。保留对象的 save 入口即可。 |
| Pennant | `HasFeatures::features` 转接 Feature／作用域，没有模型内具体数据库实现。 |
| ORM Searchable／Scout Observer | ORM SearchBuilder 的 SQL 构造属于数据库能力；Scout Observer 是独立监听与同步适配，模型入口委托 EngineManager，无需把所有入口改成应用 Service。 |
| Cashier／Pay 数据类型 | Subscription 的 active／valid 等是自身状态判断，字段解析是纯数据转换；Billable、支付 Manager 与渠道 Client 已承担服务职责，M06 单独指出 Store 中的逆向混合。 |

## 建议顺序与本轮检查

建议依次处理 M01／M02 → M03／M04 → M05 → M06；每组保持公开入口和行为，独立验收再提交。S01／S02 按维护收益决定，不作为前六项完成的前置条件。

本轮检查范围为规范、审查记录与链接：核对引用文件、方法位置、Markdown 本地链接／新增规范锚点及 `git diff --check`。没有修改 Rust、生成器、安装输出或面向开发者三语正文，没有执行新的 HTTP、Artisan 或 Rust 测试。前六项均为已定位、尚未实施的建议，不能记录为已经修复。

后续落实状态以[实施验收](implementation-2026-11-24.md)为准；本页保留静态复核时点与调整依据。
