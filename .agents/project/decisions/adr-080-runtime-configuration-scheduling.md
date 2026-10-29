# ADR-080 Laravel 配置扩展与数据库动态调度

- 状态：Accepted（用户授权的目标契约；未实现、未真实验收）
- 日期：2026-10-29
- 依据：[ADR-070](adr-070-laravel-first-rebuild.md)、[ADR-003](adr-003-database-queue-default.md)、[ADR-037](adr-037-queue-worker-lifecycle.md)、[ADR-046](adr-046-redis-queue-driver.md)
- 具体事实、固定来源、排期与验收：[专门计划](../plans/runtime-configuration-scheduling.md)。

## 背景

现有 ConfigRepository 有进程内版本和启动校验，但 Provider 启动末尾 freeze 后不能更新；现有 Scheduler 使用内存注册目录，DB只持久化触发槽和队列任务。两者不满足管理员运行时管理、跨节点限时一致或仅应用与数据库迁移恢复的完整目标。

用户要求安全热更新、默认DB持久化队列和动态计划，同时明确配置机制必须遵循 Laravel，简洁、易用、灵活扩展。不能把实现一个完整配置平台作为普通应用的新负担，也不能以简洁为由省掉白名单、校验、权限或持久一致性。

## 决策

1. `.env` 启动来源、`config/` 领域配置与现有 ConfigRepository/ServiceProvider 是唯一体系，保留普通仓储/类型读取体验。V1必须实现typed分组DB业务设置、受权限保护的管理服务/API与动态计划；宿主是否启用Provider扩展可选。这些属于授权增强，不宣称 Laravel 核心原生1:1能力；不用 Java/Go API 或第二套配置客户端。
2. 默认所有配置仅启动可设；运行时只开放代码注册的安全键、类型/范围/跨字段校验和读写权限。基础设施DB/Redis连接驱动凭据、APP_KEY/加密keyring部署密钥、编译feature、路由、任务实现及监听地址端口仍需部署重启；邮件/SMS/支付/存储业务凭据按显式支持的driver受控重建，不能全部排除，也不能把改仓储等同客户端已替换。
3. DB保存运行时配置集合、单调revision/schema版本、CAS编辑和审计，事务内完整验证/提交；节点完整验证后原子发布，内部封装快照与版本。启动冻结保持，不解冻整个配置树；审计白名单消费者避免长期旧值。普通开发者不必操作revision/snapshot协调协议。
4. 内部DB轮询负责版本追平与丢失补偿，通知仅可选加速；Redis通知/短缓存不是权威源。具体有限轮询、生效期限、陈旧期限和默认数值由实施Spec定稿并实测；DB写失败显错，启动失败不Ready，运行超过陈旧期限后受影响读取/新触发拒绝，不静默退本地默认。
5. 默认数据库队列保持，connection/queue/Job代码注册复用现有入口；普通派发、同库事务派发及Laravel after_commit分别对照。after_commit回调不冒充可靠outbox；Redis显式队列不承诺与SQL原子事务或仅app+DB恢复，外部副作用仍至少一次。
6. 动态计划复用同一 Scheduler/Queue/JobRegistry/Provider。管理员只能编辑已注册任务允许的频率/IANA时区/schema参数/目标队列/执行策略；无任意代码、命令、shell或反射。DB保存计划版本/审计/必要状态，触发事务核对当前启用版本并原子提交slot+DBjob。
7. `onOneServer` 与 `withoutOverlapping` 独立表达且保留上游默认对应；当前全局slot去重不是本目标完成证据。暂停/删除后不产生旧版本新触发，已提交任务保留原版本参数继续；取消在途是独立授权。worker降并发不强杀现有尝试，复用lease/token/重试与受管关闭。
8. 常规恢复迁移应用构建/必要部署配置与一致DB备份，即恢复管理员配置/计划及DBjobs；Redis缓存通知可丢。显式Redis queue的任务由Redis自身持久化/备份/迁移承担，不能包含在DB恢复承诺中。滚动升级须明确schema/任务版本兼容及旧节点隔离，不能只比较版本数字接受不兼容快照。
9. V1必须实现typed分组DB设置与权限保护管理API/CLI，开发者注册schema/defaults、管理员保存，不要求其写Rust/.env；V2后台表单复用同一API，区分DB已保存与节点应用状态，不伪报全节点瞬时成功。公共方法名、配置格式、表字段及允许范围先以最短使用场景Spec收敛，不预造通用DSL/控制面/第二总线/新daemon或全资源热换机制。

配置、Queue、Scheduler和动态计划的“管理API”统一指现领域Rust公开服务方法/必要traits及Gate/Policy授权接入，不是框架HTTP endpoint。框架不自动注册admin/settings路由，不固定URL/响应/权限标识；开发者在自定controller/路由显式传入认证授权上下文调用服务，复用schema校验、脱敏和审计。modules/example在模块自己的routes/controller演示，V2与显式注册CLI复用同一服务且不绕授权，不暴露无鉴权入口。Job代码注册、派发/事务、计划启停修改、重试/查询沿现Job/Queue/Scheduler/Provider扩展，不另造API家族或固定后台模型；worker/schedule runner由CLI显式运行，serve不暗启worker，CLI不是HTTP路由，框架不含示例业务特判。

## 业务设置与secret

邮件from、SMS模板/通道、支付sandbox账号、存储bucket/endpoint属于后台管理员真实需求。采用Laravel config加Spatie型typed分组Settings理念，保持唯一Repository/Provider；V1底层与管理API不能无限延期，只有宿主启用与V2界面后置。支付限配置基础和现有/扩展driver，不启动新支付业务。

secret复用现Encryptor进行DB加密和轮换，APP_KEY/实际部署密钥在数据库之外，迁移必须保留解密key；细分读/改/替换权限，不回显/记录secret，防止脱敏占位回写，复用安全机制约束endpoint与SSRF。只支持声明安全刷新的driver，候选实例验证成功后配置与资源代次原子激活供新操作使用；不支持则明确pending_restart，构建失败不谎报生效。

在途保留原实例至完成；queued任务不持久化secret，保留业务payload，每次attempt开始选有效渠道代次并持有；重试目标标识/幂等与历史storage对象不能因换bucket/账户被错误路由。DB保存和各节点应用分别可观测；旧key撤销可能影响在途。文件/对象与外部资源仍需另迁，不包含在仅app+DB恢复承诺。

## 方案取舍与 Rust 适配

采用现有内核的可选扩展：比独立配置中心/独立调度服务更少生命周期与运维依赖，能沿用Laravel知识；比无权限的任意键覆盖或只有Redis通知的方案满足持久性/安全与通知丢失要求。仅静态配置无法满足用户目标，因此不作为最终替代。

Rust使用类型化声明、校验后不可变视图发布及async受管任务，不复制PHP运行时可变对象、Spring代理反射或Go全局Facades。固定来源对照见专门计划：Spatie Settings的typed分组/DB/save/encrypted/refresh、Filament官方表单、RuoYi DB后cache仅供机制比较；Laravel MailManager缓存/purge证明config set不自动重建客户端。不得将这些上游等同本项目CAS/多节点保证。DB实现依赖现有事务/权限审计，Redis仅可选加速；不为数据库队列强加Redis必需依赖。

## 实施与验收

当前HTTP簇不抢占；R2先收敛配置契约，持久实现依赖R4/R7及适用R8，再R10完成队列事务/每任务策略和动态计划，R14真实集群/迁移验收，R15才做UI。以唯一todo领取，各簇Spec→Implement→Verify→Commit。

必须在PG18与MySQL8.4分别验证多API/worker/scheduler、Redis通知中断/丢失补偿、DB故障期限、CAS/权限/无效/未注册拒绝、计划变更竞争与在途、任务崩溃恢复、重启滚动迁移和空Redis缓存恢复。同步唯一modules/example、生成项目、README/config与公开测试；旧专项或本轮文档审计不替代新目标验收。

本ADR接受目标和边界，不表示运行代码、动态管理API、默认数值或新数据库表已经存在；未定公共格式须实施前Spec，安全与持久化要求不得删减。
