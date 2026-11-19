# Laravel 核心功能闭合对账

基线为 2026-11-13 本地提交 `10a19965`。以[固定 Laravel 基线](../plans/laravel-rebuild-baseline.md)、[交付计划](../plan.md)和[章节对照](../plans/laravel-docs-delivery-map.md)核对实现和验收。

每个编号单元权重相同。已实现并通过适用验收、单元内没有已知核心缺口，才记为闭合。Rust 等价做法需记录语言约束及可观察行为证据。部分验收只记录已通过范围。阶段内的核心单元全部闭合后才记为阶段完成。

**2026-11-19 第一代范围已扩大：Passport／OAuth2 与相关 API 扩展列为必须交付，当前尚未达到 v0.1.0 用户验收／发布条件。** 具体缺口和证据见下方 [v0.1.0 验收门禁](#v010-acceptance)；66/66 仅指下述历史核心范围，不代表全部公开契约、API 扩展或目标环境已经通过。

当前核心单元 **66/66（100%）**，原路线图九个阶段全部有范围验收记录，核心范围闭合 **9/9**。核心分母含 00–07 的 61 项、框架广播 08-01 和测试能力 T 的 4 项。

| 阶段 | 闭合数 | 已闭合单元 | 未闭合单元及主要缺口 |
| --- | ---: | --- | --- |
| 00 启动与配置 | 3/3 | 00-01 [启动骨架和配置](00-01/configuration-cache.md)；00-02 默认 HTTP、静态资源与错误格式；00-03 Artisan 基础入口 | — |
| 01 容器、日志与异常 | 7/7 | 01-01 基础容器与 Provider；01-02 [容器与延迟 Provider](01-02/README.md)；01-03 基础日志；01-04 [轮转、自定义和系统日志](01-04/windows-syslog.md)；01-05–07 Context、异常报告和渲染 | — |
| 02 数据库与集合 | 9/9 | 02-01 [数据库配置、连接和基本 SQL](02-01/README.md#命名绑定与整单元验收)；02-02 [多连接、读写与事务整单元验收](02-02/README.md#连接接口与-02-02-整单元验收)；02-03 基础查询；02-04 [高级查询与写入](02-04/README.md#mysql-和-mariadb-影响行数与整单元验收)；02-05 [Schema 表、列、索引和外键](02-05/README.md#postgresql-全文搜索向量列与整单元验收)；02-06 [迁移命令与批次账本](02-06/README.md#禁止破坏性命令与整单元验收)；02-07 Seeder；02-08 [通用集合](02-08/README.md#整单元验收)；02-09 [惰性集合](02-09/README.md#02-09-整单元验收) | — |
| 03 应用基础服务 | 9/9 | 03-01 [本地文件系统与公共磁盘](03-01/README.md#03-01-整单元验收)；03-03 S3；03-04 加密和密钥轮换；03-05 [密码哈希和重哈希](03-05/README.md#03-05-整单元验收)；03-06 [缓存、过期与原子锁](03-06/README.md#03-06-整单元验收)；03-07–08 [视图、Blade 与组件](03-08/blade-parity.md)；03-09 [Redis 连接、缓存与锁](03-09/README.md#03-09-整单元验收)；03-10 [国际化、回退和复数](03-10/README.md#标签回调对象格式化与整单元验收) | — |
| 04 HTTP 请求链 | 9/9 | 04-01 [路由参数、分组及诊断](04-01/README.md#跨参数反向引用与整单元验收)；04-02 [请求、响应和控制器](04-02/README.md#流式响应与整单元验收)；04-03 [中间件顺序、别名与 CORS](04-03/README.md#cors-条件跳过与整单元验收)；04-04 [Cookie、Session 与 CSRF](04-04/README.md#自定义驱动与整单元验收)；04-05 [Validator、API 422 与 FormRequest](04-05/README.md#04-05-整单元验收)；04-06 [Web 表单回传](04-06/README.md#上一页-url-回传复验)；04-07 [命名限流与 HTTP 节流](04-07/README.md#redis-cluster-多额度与-04-07-整单元验收)；04-08 异常报告限流；04-09 上传、下载与普通对象 URL | — |
| 05 Eloquent、认证与 API | 9/9 | 05-01 模型读写和作用域；05-02 关系、加载与集合；05-03 属性转换与序列化；05-04 工厂与 Seeder；05-05 路由模型绑定；05-06 数据库分页；05-07 [API Resources](05-07/README.md#05-07-整单元验收)；05-08 [Guard 和会话认证](05-08/README.md#登录限流事件与整单元验收)；05-09 [Gate 和 Policy](05-09/README.md) | — |
| 06 URL、后台任务与通知 | 8/8 | 06-01 [URL、重定向和签名](06-01/README.md#06-01-整单元验收)；06-02 事件与监听器；06-03–04 任务派发和 Worker；06-05 调度；06-06 [邮件与通知](06-06/README.md#06-06-整单元验收)；06-07 密码重置和邮箱验证；06-08 Artisan 帮助及交互本地化 | — |
| 07 框架专题 | 7/7 | 07-01 [Helpers／Strings](07-01/README.md#整单元验收)；07-02 [HTTP Client](07-02/README.md#整单元验收)；07-03 [Processes](07-03/README.md#整单元验收)；07-04 [Concurrency](07-04/README.md#整单元验收)；07-05 [Images](07-05/README.md#07-05-整单元验收)；07-06 [Search](07-06/README.md#07-06-整单元验收)；07-07 [Package Development](07-07/README.md#07-07-整单元验收) | — |
| 08-01 框架广播 | 1/1 | 08-01 [频道授权、事件分发和唯一任务](08-01/README.md#08-01-整单元验收) | — |
| T 测试能力 | 4/4 | T-01 [测试入口、应用与环境](T-01/README.md#测试环境与整单元复核)；T-02 HTTP／Artisan 测试入口；T-03 [数据库测试指南与整单元验收](T-03/README.md#测试清表与-t-03-整单元验收)；T-04 容器与事件替身 | — |

03-09 的 Redis Cluster 标签保留[主线复验](03-09/README.md#redis-cluster-标签主线复验)记录。已补验[标签清空中断后的过期索引清理](03-09/README.md#标签清空中断后的过期索引清理)、[显式删除后的空标签索引回收](03-09/README.md#显式删除后的空标签索引回收)、[集群标签索引的无效引用清理](03-09/README.md#集群标签索引的无效引用清理)、[集群标签索引登记容错](03-09/README.md#集群标签索引登记容错)和[按标签清空时清理无效引用](03-09/README.md#按标签清空时清理无效引用)。[指定 Redis 标签的 `flushStale()`](03-09/README.md#按指定标签清理过期引用)也已通过。[集群标签待回收集合容错](03-09/README.md#集群标签待回收集合容错)已通过。[当前主线复验](03-09/README.md#redis-cluster-标签与过期清理复验)再次确认真实集群和单机 Redis、双进程 HTTP 和独立 Artisan。[槽迁移期间的标签清理及重试](03-09/README.md#redis-cluster-槽迁移期间的过期标签清理)已通过真实 Redis、HTTP 与 Artisan 验收。[标签登记丢失后的恢复](03-09/README.md#redis-cluster-标签登记丢失后的恢复)已验证部分重建时找回未登记的索引。[索引丢失后的空代际回收](03-09/README.md#redis-cluster-索引丢失后的空代际回收)已验证索引和登记同时丢失时回收空记录，并保留仍有引用的旧代际。[清理无效代际标识](03-09/README.md#redis-cluster-过期清理回收无效代际标识)已验证存储级修剪移除无效二进制及文本标识，并保留有值的旧代际。[Redis Cluster 分批回收旧标签代际](03-09/README.md#redis-cluster-分批回收旧标签代际)已通过真实 Redis、HTTP 与 Artisan，验证清空可跨多页回收旧代际。[Redis Cluster 存储级清理分批回收空代际](03-09/README.md#redis-cluster-存储级清理分批回收空代际)已验证同槽批量检查空代际，以及索引类型异常后的重试。[按到期分数清理](03-09/README.md#redis-标签按到期分数清理)已通过真实 Redis 单机和集群、双进程 HTTP 及独立 Artisan。03-09 维持闭合。跨单元的 [memo 标签装饰器与 `flushStale()`](03-06/memoization.md#标签仓储的记忆化与过期清理)已通过真实 Redis 单机和集群验收。

03-09 在当前主线再次通过[过期标签清理的存储选择与主线复验](03-09/README.md#过期标签清理的存储选择与主线复验)。显式非 Redis 存储的空操作与无效存储的失败均保留默认 Redis 的索引。

03-09 [补齐索引登记丢失时的错型错误传播](03-09/README.md#索引登记丢失后的-redis-键类型错误)。真实 Redis Cluster 验证按标签 HTTP 清理和 Artisan 存储级清理报告失败，恢复正确键类型后可重试。

03-09 [补齐指定标签跨代际的集群过期清理](03-09/README.md#redis-cluster-指定标签跨代际清理)。真实 Redis Cluster、HTTP 和 Artisan 已核对代际及登记丢失、同槽扫描重定向和重试。

03-09 [Redis Cluster 指定标签的空代际回收](03-09/README.md#redis-cluster-指定标签的空代际回收)。真实 Redis Cluster、HTTP 和 Artisan 已核对指定范围的空代际回收、活跃引用保留和可重试性。

03-09 [Redis Cluster 指定标签的悬空索引登记回收](03-09/README.md#redis-cluster-指定标签的悬空索引登记回收)通过真实 Redis、HTTP 与 Artisan 验收。指定标签只回收自身无索引的登记，存储级清理回收其余悬空登记。

00-03 的 [Provider 注册与 stub 发布整合回归](00-03/README.md#provider-注册与-stub-发布的整合回归)已通过实际 Artisan、HTTP 和启动工作区回归。

## 01-02 容器与延迟 Provider

固定 Laravel 的 18 个容器属性、11 个容器测试文件和 Provider 仓库测试已按行为复核。条件绑定、请求和 Artisan 作用域、延迟 Provider 服务键及事件、回调、重绑定和并发隔离通过整单元回归。以编译目录表达 Provider 元数据，保留服务键和事件的不同启动顺序。 [验收](01-02/README.md)

## 01-04 日志轮转、自定义和系统日志

轮转、临时渠道、工厂错误、格式化器、processor、tap、配置型 handler、缓冲、Unix syslog 与 JSON 异常路径有实际验收。Windows ARM64 原生完成 Application 事件日志、双应用 HTTP 和 Artisan 回归，关闭 01-04。 [验收](01-04/windows-syslog.md)

## 05-01 模型生命周期

SQLite 和 MySQL 8.4 已验证模型持久化、读取和软删除事件，以及静默和取消。显式事务写入、提交和回滚已用 SQLite／MySQL 验收，为事务后 Observer 提供执行边界。事件映射已用 SQLite 和 MySQL 8.4 验证取消写入、普通监听器回退、事务提交与回滚。模型复制及静默复制已通过 SQLite 和 MySQL 8.4 验证。自动 UUIDv7、ULID 及复制和路由绑定已在 SQLite 和 MySQL 8.4 验收。数据库生成列刷新已在 SQLite 和 MySQL 8.4 验证。单模型写连接回读、原位刷新、命名关系重载和事务锁定回读已通过双库验收。MySQL 行锁在提交和回滚时的释放已实测。05-01 核心闭合。 [验收](05-01/README.md#模型生命周期派发)

## 05-02 关系身份筛选

四个关系身份 OR 变体、多态 Pivot 身份查询、中间表过滤、插入默认值、排序及时间戳已通过 SQLite 和 MySQL 8.4 验收。自定义 Pivot 的类型化水合、转换、写入生命周期与多态隔离已专项验收。Pivot 反向关系自动水合已通过双库验收，MySQL 核对无额外查询。多对多切换及声明端触碰已专项验收。模型保存时的 `BelongsTo` 和 `MorphTo` 所属关系触碰及递归传播已通过双库验收。Pivot 变更时的相关端触碰已通过双库验收。常规模型保存时可声明普通和多态多对多触碰，已通过双库验收。Pivot 写入可从模型正反向声明推断触碰。应用作用域的动态关系登记、解析及加载已通过双库验收。动态关系的存在、缺失和计数查询已通过双库验收。组合模型可显式登记动态关系回退。点分关系路径的批量预加载已通过双库验收。已登记单目标关系支持点分路径存在、缺失及末级计数。已登记的多目标 MorphTo 路径加载、存在、缺失和计数已通过双库验收。多态路径可从数据库发现实际目标。普通和多态路径可批量补齐缺失后代。从集合借用单模型时，可按自动预加载策略批量加载根集合的关系。已加载集合支持单键、批量和模型主键查找及缺键错误，也支持按模型身份和谓词判断包含关系。集合可批量合并可见性设置及替换、清除追加字段。集合支持模型映射、Support 集合转换、带键映射及投影。带键模型映射可保留 Eloquent 接口，显式键随刷新、转换和 JSON 输出保留。借用单个关联模型时，首次访问下一层缺失关系可回到根集合批量加载并写回关系图。多态目标借用可按选定别名批量加载并回写根集合。借用可沿多层普通和多态关系回到根集合。移出集合的根模型可共享首层关系批量查询，通过异步关系访问取得的后代可继续共享批量查询。批量加载后的关系立即对同批模型的同步读取及序列化可见。集合支持按键分区、分组计数和重复项检测。集合支持位置配对、双向填充和模型扁平转换。位置配对支持多输入，标量键投影可与原集合键交换。集合支持读取队列元数据和共有关系路径。普通模型与集合可通过类型化 JSON 标识从真实 Worker 恢复，任务级缺失模型的默认失败、可选静默删除、锁释放及批次重放已验收，自定义普通与多态 Pivot 可从 Worker 恢复，并保持原表写入约束。集合公开主键字典。普通和多态关系支持类型化列筛选、缺失与 OR 入口，普通关系可共用筛选及预加载条件。已登记路径支持末级列条件、闭包、缺失、OR 与条件预加载。多态通配路径的末级筛选与预加载共用实际发现的目标，已通过双库验收。集合可在刷新时按多个已登记路径预加载，已通过双库验收。多态延后计数与聚合已写回父模型的关系缓存，通过双库验收。多态关系与登记路径的延后重载也已写回父模型缓存，通过双库验收。声明端可按目标叠加预加载、计数与约束，通过双库验收。多态目标的三个软删除加载选项、混合目标和约束顺序已通过双库验收。多态目标可按登记别名构造默认模型，并可显式选择空类型的默认别名，已通过双库验收。命名多态关联的赋值和解除可立即更新指定关系缓存，已通过双库验收。命名所属关系的模型赋值、外键赋值及解除关联已通过双库验收。单值关系的 `is`、`isNot`、普通比较零查询及穿透与多选一存在查询已通过 SQLite 和 MySQL 8.4 验收。Pivot 写入的 `OrFail` 入口已通过双库验收，包括自定义 Pivot 事件失败后的回滚。`BelongsToMany::createOrFirst` 已通过写库回查、并发唯一冲突和 Pivot 写入失败回滚验收。多对多关系的 `firstOrCreate` 与 `updateOrCreate` 属性映射入口已通过双库验收。多对多延迟属性和软删除恢复已通过双库验收。多对多 `incrementOrCreate` 的原子计数、模型事件与失败回滚已通过双库验收。直接、多态与穿透关系的递增入口已通过双库验收。关系计数器自定义递增转换已通过 SQLite 与 MySQL 验收。模型独立递增、递减及多列运算已通过双库验收。静默计数入口和 Eloquent Builder 批量计数已通过双库验收。直接和多态关系的批量保存与静默创建已通过双库验收。直接与多态关系的强制创建已通过双库验收。多对多模型保存、创建、批量及静默和强制创建变体已通过双库验收。直接、多态与穿透关系的查找、创建、更新和恢复变体已通过双库验收。直接和多态关系的 upsert 已通过双库验收。穿透和多对多关系的 upsert 转发已通过双库验收。普通及多态反向关系的 upsert 已通过双库验收，非空多态写入要求已注册目标。带作用域关系的 withAttributes 已通过双库验收。已加载关系的递归保存及静默保存已通过双库验收。直接与多态关系的 chaperone 反向水合及查询计数已通过双库验收。复用既有关系键的 through().has() 已通过双库验收。按路径预加载特定列已通过双库验收。应用级自定义多态类型映射已通过双库验收。单模型延后加载关系聚合已通过 MySQL 8.4 验收。查询集合工厂与关系未保存模型批量构造已通过 MySQL 8.4 验收。批量模型工厂与队列恢复的自定义集合键已通过 MySQL 8.4 和真实 Worker 验收。多值关系结果的自定义集合键及 JSON 已通过 MySQL 8.4 验收。PHP 对象共享与隐式类继承的 Rust 对应方式见整单元验收。 [验收](05-02/README.md#带键的-eloquent-集合)

按交付计划复验外键、预加载、查询计数、集合键和关系写入。MySQL 8.4.13 和 SQLite 通过整单元复验，05-02 核心闭合。运行时动态属性和集合子类采用 Rust 类型化关系访问、快照共享和集合扩展实现。 [整单元验收](05-02/README.md#05-02-整单元验收)

## 05-03 属性转换与模型序列化

访问器、修改器、各类 Cast、日期、隐藏和追加字段以及模型与集合 JSON 已通过双库和 HTTP 复验。应用级 JSON 编解码支持普通及加密属性，并验证跨应用隔离。05-01 前置满足，05-03 核心闭合。 [整单元验收](05-03/README.md#05-03-整单元验收)

## 05-04 模型工厂与 Seeder

编译期工厂绑定、状态、回调、批次、延迟创建、工厂关系和 Seeder 集成通过整单元复验。固定 Factory／Seeder 用例矩阵和三语工厂、Seeder 正文已核对。MySQL 8.4.13 实跑默认及文档 Seeder，双库工厂、地区 Faker 和八个默认扩展的验收记录可追溯。05-04 核心闭合。 [验收](05-04/README.md#05-04-整单元验收)

## 05-06 数据库分页

普通、简易、游标三类分页通过双库实际 HTTP、JSON、Blade 和模板发布复验。同请求中的不同具名分页独立导航。02-03 与 05-02 依赖满足，05-06 核心闭合。命名路由链接和 Resource 由 06-01、05-07 验收。 [整单元验收](05-06/README.md#05-06-整单元验收)

## 05-08 Session 验收遗留

05-02 验收时曾触发记住登录测试的密文子串断言。共享验收现在使用 Cookie 解密、名称绑定和 HTTP 会话恢复验证，MySQL 分支已通过。SQLite 分支按发布矩阵复验。 [验收](05-02/README.md#eloquent-集合的模型身份与谓词)

## 06-01 URL、重定向和签名

已接入链式闪存与命名错误包，并通过默认 Web 中间件、加密会话和实际表单验收。已接入控制器动作 URL 和重定向，实测覆盖、域名优先与应用隔离。具名和位置模型参数已按自定义绑定列生成 URL，并用 SQLite／MySQL 实测。本地临时链接回调和常用 S3 签名选项已验收。S3 独立签名基地址和指定对象版本已通过 RustFS 实测。高级 S3 SDK 请求构建器已用 RustFS 验收 Range 下载与条件上传。模型访问器与类型转换已通过 SQLite／MySQL 8.4 HTTP 验收。可信代理归于 04-03。S3 独立 ACL 已由 [03-03](03-03/README.md) 的 SeaweedFS／BucketOwnerEnforced 场景验收，不再重复列待办；托管 AWS／STS 等目标仍按发布矩阵跟踪。 [URL 验收](06-01/README.md#重定向闪存)

实际 HTTP 和 Artisan 已验收请求级默认值、路径保留字符编码、完整 URL、上一会话 URL 与路由名、重定向入口及边界。三语文档示例通过编译并实际访问，06-01 核心闭合。 [整单元验收](06-01/README.md#06-01-整单元验收)

## 06-02 动态事件发现

编译监听器清单支持 `ShouldBeDiscovered`，通过两个应用的真实 HTTP 与 Artisan 验证条件过滤、隔离及注册失败行为。生产事件缓存通过独立 Artisan 进程和真实 HTTP 验收。Eloquent Observer 注册、写前取消和写后审计已用 SQLite 和 MySQL 8.4 验证。Observer 生成器已验收。事务后 Observer 已在 SQLite 和 MySQL 8.4 验证提交、回滚和保存点，接口监听器顺序已用真实 HTTP 验证，类监听器已通过 SQLite 和 MySQL 8.4 的真实 HTTP 验证所属应用的容器解析、传播停止及事务提交与回滚。06-02 核心闭合，队列监听器扩展归 06-04。 [事件验收](06-02/README.md#类监听器容器解析)

## 06-05 调度收尾

账本登记的四类缺口已验收。覆盖写、追加写、输出钩子、URL 探测与后台完成事件均通过真实 `schedule:run`，共享缓存的重叠锁在完成与失败后均可重新取得。输出邮件、清锁和手工测试命令随后通过实际 Artisan 验收，06-05 核心主线闭合。 [调度验收](06-05/scheduler-output-background.md)

## 07-01 Helpers 与 Strings

06 阶段复核为 8/8，06-06 的真实供应商账户和证书场景继续在邮件验收中跟踪。

前置 02-08 已闭合。07-01 末批完成 `Stringable::scan` 格式串与 `Number` 多语言整数、小数及序数拼读。按固定 Laravel 对照余项并经真实 HTTP、Artisan 与三语文档验收，核心单元闭合。 [07-01 整单元验收](07-01/README.md#整单元验收)

## 07-02 HTTP Client

前置 01-01 已闭合。并发池、批次、请求事件和 Digest Auth 已分批验收。末批通过 URI 模板、指定 URL 放行和可修改下一次请求的重试回调。真实 HTTP、Artisan、三语文档及定向回归通过，07-02 核心单元闭合。 [07-02 整单元验收](07-02/README.md#整单元验收)

## 07-03 Processes

前置 01-01 已闭合。控制终端、`waitUntil`、超时结果、模拟进程脚本与数值信号均经真实子进程、HTTP、Artisan 和三语文档验收。07-03 核心单元闭合。 [07-03 整单元验收](07-03/README.md#整单元验收)

## 07-04 Concurrency

前置 07-03 已闭合。已用真实 HTTP、Artisan 和 Redis worker 验证延后执行、主动释放、失败和硬超时隔离。驱动选择、命名结果、超时取消及进程回收也已复核，07-04 核心单元闭合。 [07-04 整单元验收](07-04/README.md#整单元验收)

## 07-05 Images

AVIF、HEIC／HEIF、JPEG XL 静态读写和纯 Rust WebP 编码已通过真实 HTTP、Artisan 和独立系统读取器验收。GIF、APNG、WebP 的帧合成、播放次数和时序已通过真实入口及独立读取器验收，JPEG XL 有损和无损质量分支已通过。AVIF、HEIC、JPEG XL 的多帧往返、延迟与播放次数已通过。变换后的 EXIF、ICC、XMP 已通过六种格式的真实入口验证。JPEG 同格式变换保留 IPTC 与注释。无封面的 AVIF 和 HEIC 主轨道元数据已用真实 HTTP 和 Artisan 复验。外部 AVIF 与 HEIC 逐帧 ICC、EXIF、XMP 经同格式与交叉往返复验，保持原帧关联、延迟及播放次数。07-05 核心单元闭合。 [整单元验收](07-05/README.md#07-05-整单元验收) [静态格式验收](07-05/README.md#静态格式补齐) [动画验收](07-05/README.md#帧变换与时序)

## 07-06 Search

MySQL 与 PostgreSQL 原生全文索引、模型排序和分页均通过真实查询入口。PostgreSQL 提交后的索引更新与删除、回滚、并发序列化和死锁重试均已验收。05-03、06-04、07-02 前置满足，07-06 核心单元闭合。 [整单元验收](07-06/README.md#07-06-整单元验收)

## 07-07 Package Development

前置 01-02、03-08、06-08 已闭合。资源发布的筛选、覆盖、迁移与路径边界已验收。当前 Cargo 目标和特性的包自动发现、按名与全部排除、清单重建与清除、包翻译和视图组件均已用实际 HTTP 和 Artisan 验收。07-07 核心单元闭合。 [整单元验收](07-07/README.md#07-07-整单元验收)

## 08-01 广播频道与事件分发

前置 05-09、06-02、06-04 已闭合。频道授权和模型参数、公共／私有／在线频道投递、匿名广播、来源套接字、事务、队列与唯一锁均通过真实 HTTP 和 Artisan 验收。08-01 核心单元闭合。 [整单元验收](08-01/README.md#08-01-整单元验收)

## 本轮已验收的增量

以下增量已提交到本地。所属单元仍有表中列出的其他缺口，闭合数以顶部账本为准。

| 单元 | 实际验收 | 提交 |
| --- | --- | --- |
| 00-02 | 健康页 HTML／JSON、诊断失败和维护模式例外 | `132dc081`、`0294c666` |
| 02-04、02-05 | MySQL 8.4 JSON 查询与列修饰符 | `3fc79e0d`、`ac1e44d4` |
| 03-06、03-08、03-10 | Redis 请求及任务内缓存、Blade 条件子视图、Artisan 发布语言文件 | `733ad42b`、`8fcc9724`、`f6719659` |
| 04-04、04-07 | MySQL Session 与 Redis 锁、Redis 跨进程数字节流 | `55766215`、`917903f2` |
| 06-02、06-03 | MySQL 事件延迟派发、自定义队列连接及 MySQL／Redis Worker 复验 | `81541c47`、`daff67a0` |
| 06-05 | 任务输出邮件、清除重叠锁和手工运行 | `36bd30ee`、`e7ff17e5` |

## 扩展与发布

| 范围 | 基线状态 | 待验收或确认 |
| --- | --- | --- |
| [08-02 Reverb](08-02/README.md) | [按模拟验收口径闭合](08-02/README.md#08-02-模拟验收与闭合)。缓存频道、统计与批量发布、Redis 扩展、Artisan 和 Echo 均已实测。具名 Pusher 驱动通过隔离模拟服务验收 | 托管 Pusher TLS 和部署配置实机验证 |
| [08-03 gRPC](08-03/README.md) | [已闭合](08-03/README.md#08-03-整单元验收)。监听、命名客户端、mTLS 凭据及双端拦截器通过真实 Tonic、HTTP 和 Artisan 验收 | — |
| Sanctum 与 API 安装 | Sanctum-01 至 03 在计划范围内闭合。 | `install:api --passport` 归入 Passport。 |
| `vendor:publish` | 命令范围闭合，包含发布、交互选择、迁移时间戳及控制台提示。 | 应用内源路径、目标及迁移目录的符号链接边界已复验。包发现与清单缓存已随 07-07 验收。 |
| [第一代 API 扩展](../plan.md#first-generation-api) | Passport／OAuth2、Fortify、Socialite、Precognition、OIDC RP／双应用 SSO 已纳入第一代，15 个单元中 Passport-01 已验收，剩余 14 个待正式实现／验收。 | 固定版本、依赖、接口和验收见计划；OIDC 与 Passport 分开。 |
| [其他生态包](../plans/laravel-docs-delivery-map.md#packages) | Scout、Horizon、Dusk 等按各自范围独立核对。 | 未因本次 API 授权自动全部纳入第一代。 |
| [08-04 发布](08-04/README.md) | [按模拟正式环境验收闭合](08-04/README.md#08-04-模拟正式环境验收与闭合)。干净检出安装、[原生 Ubuntu x86_64 发布复验](08-04/docker.md#原生-ubuntu-x86_64-发布复验)（`07307c6e1`）、Linux amd64 兼容环境镜像、Windows ARM64 独立部署、SQLite 3.51.3／MySQL 5.7.44 Oracle 通用归档和 Oracle Linux 7 ECDHE 派生镜像、8.0.46／8.4.13／MariaDB 10.3.39 和 11.8.9／PostgreSQL 10–18 实测版本／SQL Server 2017 和 2022 TDS，以及[本地 TLS 证书链与轮换、SQS 标准／FIFO SigV4 模拟补测](08-04/README.md#模拟验收补测)通过 | Windows Docker Desktop、生产 TLS 和托管 AWS SQS 标准／FIFO 的发布实机验证，见[环境与权限复查](08-04/README.md#剩余实机环境复查) |

08-04 [主线独立发布包复验](08-04/README.md#主线独立发布包与交付对照复验)已验证 `2d1f83dfd` 的离线锁定归档、MySQL 和 Redis、HTTP、Artisan、搬移与关停。三语映射的 104 个本地路径完整，范围待确认的生态条目继续在交付计划中跟踪。

### Sanctum、API 安装与资源发布复核（2026-11-15）

复核快照 `c8b60fb8`。对照固定 Laravel 13.40.0 和 Sanctum 4.4.0，复用以下实际验收。Windows 首装见 `f6cfc23e`，截至 `a12a274f` 的重建与迁移失败行为见 [Windows 重建复验](sanctum-03/README.md#windows-重建复验)，Windows 发布交互见 `707ab390`。本轮只执行新增或受影响的验收。

| 范围 | 已验收 | 本轮差异验收 |
| --- | --- | --- |
| [Sanctum-01](sanctum-01/README.md) | `156702c5`。SQLite 和 MySQL 8.4 的首次、重复迁移、回滚和重建。 | 该单元闭合。 |
| [Sanctum-02](sanctum-02/parity-v4.4.0.md) | Bearer、显式 SPA、密码变更后会话失效、可替换中间件、清理命令、自定义模型事件与转换、整数／UUID／ULID、提取与验证回调、测试身份及令牌关系已有实测。 | 默认令牌使用 Eloquent，MySQL 8.4.13 实测签发、真实 HTTP 认证、最后使用时间写回和单条撤销的生命周期。默认序列化包含 `updated_at` 并隐藏摘要，自定义模型仍遵循其可见性。 |
| [Sanctum-03 / `install:api`](sanctum-03/README.md) | `4d0b8b03`、`b4d23daf`、`f6cfc23e`。首次生成、重复安装、覆盖与迁移确认通过。macOS 和 Windows CMD／PowerShell 有实测，生成路由已验证 Bearer 200／401。 | 实测生成模板返回已注册的自定义认证模型及其可见字段。无法自动接线时保留生成文件并警告。macOS 真实终端确认迁移，分别验证启用 Sanctum 与自动重建两条路径中的迁移失败提示、安装退出码 0 和脚手架保留，以及首次重建迁移成功。Windows CMD 和 PowerShell 也验证了改动后的重建及迁移成功和失败路径。构建错误仍作为安装错误返回。 |
| [`vendor:publish`](sanctum-02/parity-v4.4.0.md#vendorpublish-逐键交互) | `8e47fbf6`、`265f9646`、`707ab390`、`b4d23daf`。Provider、标签、全部、交集、目录复制、覆盖、事件和迁移日期已有实测。macOS 与 Windows CMD／PowerShell 逐键选择通过。 | 实际 Artisan 及定向测试验证逐标签提示、文件和目录复制状态、已存在和缺失目标两种跳过原因及发布事件。 |

保留已记录的 Rust 装配边界：`install:api` 通过 Cargo 功能和编译迁移注册实现安装，Composer 路径选项属于 PHP。已有迁移文件沿用原文件名以匹配已编译的迁移注册及账本。Windows 保留已验收的逐键搜索交互。无已注册模型序列化器的自定义身份只返回认证标识。Rust 1.94 格式、工作区全目标严格 Clippy（启用 Sanctum）及三语 VitePress 构建通过。

`install:api --passport` 单独归入 Passport 集成。待 Passport 就绪后验收选项、安装、受保护路由及相应守卫，不计入上述 Sanctum 验收。核心单元的闭合数仍沿用本账本口径。

## 额外队列驱动与载荷定稿

已接通 Null、Deferred、Background 和 Failover。Rust 任务载荷采用版本化 JSON 与编译注册。Beanstalkd 基础驱动、批次和链中批次已通过真实 Worker。SQS 标准／FIFO 与溢出载荷已通过独立 ElasticMQ、Redis 和真实 Artisan。跨连接转发已验收目标延迟及 SQS 拒绝后的重试。SQS 原生批量发送已验收按条数和字节分块、HTTP 200 逐项失败、恢复和追加。FIFO 已验收 broker 接收后响应失败、SQL 确认失败和并发恢复。标准 SQS 和 Beanstalkd 已实测 SQL 确认失败后的副本重投递与批次结算。并发副本已用两个独立 Worker 验证成功、失败及成功先于失败的结算。处理器仍需应用幂等。06-03／06-04 核心闭合，托管 AWS 实机归 08-04 发布验收。 [外部队列批次验收](06-03/external-batches.md) [Beanstalkd 验收](06-03/beanstalkd.md) · [SQS 验收](06-03/sqs.md)

Background 已在 macOS 实测发起进程退出后仍可完成。Deferred 与 Background 的共享唯一锁通过真实 HTTP 和 Artisan 专项。 [驱动验收与边界](06-03/lifecycle-failover-drivers.md)

## Bus 派发

已验收注册任务的 Bus 派发、同步、直接执行和 HTTP 终止阶段，提交 `d3bf3388`。 [Bus 派发验收](06-03/bus-dispatch.md)

## 任务中间件

已补齐固定 Laravel 中间件清单中缺少的 `FailOnException`，通过 MySQL 和 Redis 独立 Worker 验收。 [异常失败验收](06-04/fail-on-exception.md)

## 队列唯一任务与跨存储批次

MySQL 与 Redis 队列均通过唯一任务的独立 Worker 验收，提交 `641241a0`。独立 MySQL 连接的批次发布、追加、故障恢复和 Worker 消费已通过专项验收。 [唯一任务验收](06-03/unique-jobs.md) · [跨数据库批次验收](06-04/job-batch-cross-database.md)

## 本轮整体验收

macOS arm64、Rust 1.94.0。锁定离线工作区测试退出码为 0，日志合计 610 项通过、0 项失败、7 项夹具忽略。MySQL 8.4.13 和 Redis 8.10.3 的真实驱动路径另按上表专项验收。格式检查和三语站点构建通过。

全部测试结束后，用 `cargo clean --target-dir` 清理本轮专用临时目标目录 `/private/tmp/openrust-core.JBsAy8/target`，释放 11.1 GiB，磁盘复查剩余 84 GiB。保留验收日志、工作区和共享依赖。

## 当前主线整仓复验

以 `4355d5042` 的干净归档与本轮 T-04 补丁运行 Rust 1.94.0 工作区锁定离线串行回归，标准测试 1,326 项通过、0 项失败、9 项忽略，另有两个自定义测试程序通过。Provider 生成、stub 发布和事件替身三项既有整仓回归均已通过。真实 MySQL、文档、Clippy 与 Artisan 入口的专项证据见 [T-04 验收](T-04/README.md#事务生命周期事件与替身回归)。外部驱动仍以专项及发布矩阵为准。

T-04 的 [作用域内事件替身](T-04/README.md#作用域内的事件替身)通过同一进程的并发 TCP HTTP、SQLite 和真实 MySQL 事件回归、Artisan 以及三语示例。

T-04 的 [事件负载筛选与断言](T-04/README.md#按事件负载筛选与断言)通过真实 HTTP、SQLite 和 MySQL 事件回归、Artisan 以及三语示例。

T-04 的 [测试时间控制](T-04/README.md#控制测试时间)通过应用隔离、HTTP、Artisan、SQLite 模型时间戳、真实 MySQL 模型回归和三语示例。

T-04 的 [通用 Facade 调用记录](T-04/README.md#通用-facade-调用记录)通过 HTTP、Artisan、容器及隔离验收。以 `89d5e2565` 的干净归档和本轮补丁复验全部 169 个测试目标，标准测试 1,332 项通过、0 项失败、9 项忽略，自定义测试程序两项通过。

04-02 与 T-04 的 [请求日期测试时钟](04-02/README.md#请求日期读取应用测试时钟)通过固定 Laravel 与 Carbon 3.8.4 的 22 组实际 TCP 对照，涵盖相对输入、格式缺项与应用隔离。

06-05 与 T-04 的 [调度测试时钟](06-05/README.md#调度读取应用测试时钟)通过实际 Artisan、Redis 8.10.3 跨进程调度和 Carbon 秒级舍入对照。

06-05 的 [调度列表的环境筛选、排序与展示时区](06-05/README.md#调度列表的环境筛选排序与展示时区)通过真实 Artisan 与 Redis 回归。固定 Laravel 的 JSON 列表、显示时区下的 cron 表达式转换和详细终端输出继续跟踪。

06-05 的 [调度列表的 JSON 与 cron 展示时区](06-05/README.md#调度列表的-json-与-cron-展示时区)通过实际 Artisan、固定基线转换矩阵与 Redis 锁状态验收。[调度列表的终端排版与详细模式](06-05/README.md#调度列表的终端排版与详细模式)通过独立 Artisan、实际伪终端和 Redis 锁状态验收。

08-04 [复验当前主线整仓和独立发布](08-04/README.md#当前主线整仓回归与独立发布复验)。修复三个真实 MySQL／Redis 测试夹具边界，以 168 个工作区目标和实际发布二进制复验证据确认，核心统计维持 66/66。

06-06 的 [Markdown 邮件布局和主题发布](06-06/README.md#markdown-邮件布局和主题发布)通过源码与独立二进制发布、现有文件保留和覆盖、SMTP 投递及 Mailpit 的真实 HTTP 验收。

06-06 的 [Markdown 邮件的四类 Blade 组件](06-06/README.md#markdown-邮件的四类-blade-组件)通过真实邮件投递、Mailpit HTTP、Blade 隔离和独立二进制的 11 个资源发布。

06-06 的 [Markdown 邮件页眉、页脚与补充说明](06-06/README.md#markdown-邮件页眉页脚与补充说明)通过真实 SMTP 和 Mailpit HTTP、页脚语言与应用时区、模板覆盖以及独立二进制发布 17 个资源的验收。

06-06 的 [Markdown 邮件显式布局与响应式主题](06-06/README.md#markdown-邮件显式布局与响应式主题)通过真实 SMTP 和 Mailpit HTTP、根布局与四个槽位、响应式主题以及独立二进制发布 17 个资源的验收。

06-06 的 [Markdown 邮件扩展与安全编码](06-06/README.md#markdown-邮件扩展与安全编码)通过真实 SMTP 和 Mailpit HTTP、两个应用的解析配置隔离、字面插值与动态链接过滤以及独立二进制发布的验收。

06-06 的 [单封 Markdown 邮件主题](06-06/README.md#单封-markdown-邮件主题)通过真实 SMTP 和 Mailpit HTTP、直接发送、MySQL 和 Redis Artisan Worker、默认主题回退及实际 Mailpit HTTP的验收。

06-06 的 [邮件渲染与浏览器预览](06-06/README.md#邮件渲染与浏览器预览)通过真实 SMTP 和 Mailpit HTTP、实际 HTTP 预览、无传输渲染、语言与主题覆盖及真实投递回归的验收。

06-06 的 [邮件 MIME 内嵌图片](06-06/README.md#邮件 MIME 内嵌图片)通过真实 SMTP 和 Mailpit HTTP、SMTP 原始 MIME、数据库与 Redis Worker 的 CID 保持，以及独立 HTTP 预览的验收。

06-06 的 [供应商 API 的内嵌图片字段](06-06/README.md#供应商 API 的内嵌图片字段)通过真实 SMTP 和 Mailpit HTTP、Postmark、Resend、Cloudflare 与 Mailgun 表单的 CID 协议回归及 SMTP 内嵌图片复验的验收。

06-06 的 [Artisan 生成 Markdown 邮件与 Blade 视图](06-06/README.md#Artisan 生成 Markdown 邮件与 Blade 视图)通过真实 SMTP 和 Mailpit HTTP、实际 Artisan 生成三种邮件、配套测试以及编译后预览的验收。

06-06 的 [Artisan 生成通知与 Markdown 邮件模板](06-06/README.md#Artisan 生成通知与 Markdown 邮件模板)通过真实 SMTP 和 Mailpit HTTP、实际 Artisan 生成普通和 Markdown 通知、配套测试以及编译后真实邮件投递的验收。

06-06 的 [Artisan 邮件与通知生成器的缺参交互](06-06/README.md#artisan-邮件与通知生成器的缺参交互)已通过真实终端、无交互路径和三语文档验收。

06-06 的 [邮件附件的路径、磁盘与字节来源](06-06/README.md#邮件附件的路径磁盘与字节来源)通过真实 HTTP、SMTP、独立 MySQL 和 Redis Worker、S3 协议和三语文档验收。

06-06 的 [上传文件、默认云磁盘及邮件附件断言](06-06/README.md#上传文件默认云磁盘及邮件附件断言)通过 multipart HTTP、SMTP、独立 Worker、云磁盘和三语断言示例的验收。

06-06 的 [URL 附件与可附加对象](06-06/README.md#url-附件与可附加对象)通过 HTTP 下载、SMTP、独立 Worker 重试和三语文档验收。

06-06 的 [邮件标签和元数据](06-06/README.md#邮件标签和元数据)通过 MIME、独立 Worker、各供应商请求字段和三语文档验收。

06-06 的 [邮件消息定制回调](06-06/README.md#邮件消息定制回调)通过即时 MIME、供应商协议、跨进程 Worker、失败重试和应用隔离验收。

06-06 的 [邮件正文与地址断言](06-06/README.md#邮件正文与地址断言)通过真实 HTTP、语言上下文、转义、顺序、发送回归和三语文档验收。

06-06 的 [通知邮件的默认格式与操作按钮](06-06/README.md#通知邮件的默认格式与操作按钮)通过真实 HTTP 双正文、语言、按钮、Artisan 发布覆盖、独立 Worker 及三语文档验收。

06-06 的 [通知邮件的地址、附件与消息定制](06-06/README.md#通知邮件的地址、附件与消息定制)通过真实 HTTP、SMTP、MySQL 和 Redis 独立 Worker 的信封、附件与消息定制验收。

06-06 的 [通知邮件的模板、预览与优先级](06-06/README.md#通知邮件的模板、预览与优先级)通过真实 HTTP 预览、SMTP 和独立 Worker 的 MIME 优先级、Artisan 生成与三语验收。

06-06 的 [Vonage 短信通知渠道](06-06/README.md#Vonage 短信通知渠道)通过实际 HTTP 的 Vonage 协议、独立 Worker 重试和三语验收。

06-06 的 [Slack Bot API 通知与 Block Kit](06-06/README.md#Slack Bot API 通知与 Block Kit)通过实际 HTTP 的 Slack Bot API 协议、Block Kit、独立 Worker 与三语验收。

06-06 的 [Slack 传入 Webhook 路由与队列投递](06-06/README.md#slack-传入-webhook-路由与队列投递)通过真实 HTTP 协议、MySQL 和 Redis 加密队列载荷、独立 Artisan Worker 与三语验收。

06-06 的 [Slack 旧式附件 Webhook 投递](06-06/README.md#slack-旧式附件-webhook-投递)通过真实 HTTP 附件 JSON、独立 Artisan Worker、MySQL 和 Redis 加密队列验收。

06-06 的 [Slack Block Kit 选择器与文本约束](06-06/README.md#slack-block-kit-选择器与文本约束)通过真实 HTTP、MySQL 和 Redis 独立 Artisan Worker 与三语验收。

06-06 的 [Slack Block Kit 标题、分隔线与图片](06-06/README.md#slack-block-kit-标题分隔线与图片)通过真实 HTTP、MySQL 和 Redis 独立 Artisan Worker 与三语验收。

06-06 的 [Slack 元数据与线程回复](06-06/README.md#slack-元数据与线程回复)通过 HTTP、独立 Artisan Worker 及三语验收。

06-07 的 [密码重置与邮箱验证的默认邮件](06-07/README.md#default-auth-notification-mail)通过实际 HTTP、Mailpit SMTP、令牌仓储和 Artisan 验收。

06-07 的 [认证邮件应用级链接与内容回调](06-07/README.md#auth-notification-mail-hooks)通过真实 HTTP、SMTP、自定义签名链接及应用隔离验收。

06-07 的 [换址与并发邮箱验证](06-07/README.md#verification-after-email-change)通过真实 HTTP、SQLite 与 MySQL 精确邮箱更新及并行事件验收。

03-09 [Redis Cluster 指定标签的异常索引登记清理](03-09/README.md#redis-cluster-指定标签的异常索引登记清理)通过真实 Redis、双进程 HTTP 和独立 Artisan 验收。按标签清理仅移除归属于所选标签的异常索引登记，存储级清理处理其余登记。核心账本维持 66/66。

06-08 [Artisan 完整纯文本帮助](06-08/README.md#artisan-完整纯文本帮助)已用三语实际 Artisan 与注册命令回归验收。核心账本维持 66/66。

08-04 [独立发布的配置缓存与 Artisan 帮助](08-04/README.md#独立发布的配置缓存与-artisan-帮助)使用当前主线独立发布产物验证。三语帮助、配置缓存与搬移、真实 MySQL／Redis Worker 和 HTTP 通过。发布实机项保持待目标环境。

06-08 [Artisan 文本列表的全局选项](06-08/README.md#artisan-文本列表的全局选项)通过三语实际 Artisan、定向回归和三语站点验收。

08-04 [Artisan 服务重载](08-04/README.md#artisan-服务重载)已补齐，真实 Redis／MySQL、独立 Worker、调度和 Reverb 的 Artisan 重载通过。统一启动的内嵌服务继续由主进程的生命周期管理。

08-04 [Artisan 部署优化与缓存清理](08-04/README.md#artisan-部署优化与缓存清理)通过真实 Artisan、HTTP 和 Redis 验收。持久化路由缓存由路由单元继续跟踪。

04-01 [持久化路由匹配索引](04-01/README.md#持久化路由匹配索引)和 08-04 部署命令联动通过实际 HTTP、Artisan、MySQL、Redis 和三语文档验收。

04-01／T-01 [路由缓存路径与测试夹具隔离](04-01/README.md#路由缓存路径与测试夹具隔离)通过真实 HTTP、Artisan、子进程隔离和三语文档验收。

06-02／T-01 [事件缓存路径与测试隔离](06-02/README.md#事件缓存路径与测试隔离)通过真实 HTTP、Artisan、子进程隔离和三语文档验收。

07-07／08-04 [独立部署的缓存路径与包清单清理](08-04/README.md#独立部署的缓存路径与包清单清理)通过实际独立二进制、HTTP、Artisan 及三语文档验收。

06-04 [启动前维护页面的请求目标匹配](06-04/maintenance-prerender.md#启动前维护页面的请求目标匹配)通过真实 TCP、Artisan、Redis 和三语文档复验。

04-02 [HTTP 请求内容协商](04-02/README.md#HTTP 请求内容协商)通过固定上游对照、真实 HTTP、Artisan 和三语文档验收。

04-02 [请求头路径与当前URL](04-02/README.md#请求头路径与当前url)通过固定 Symfony 对照、真实 HTTP、Artisan 和三语文档验收。

04-02 [输入存在性与可变输入](04-02/README.md#输入存在性与可变输入)通过固定 Laravel 对照、真实 HTTP、Artisan 及三语文档验收。

04-02／04-03 [输入修剪与空字符串规范化](04-02/README.md#输入修剪与空字符串规范化)通过固定 Laravel 对照、真实 HTTP、Artisan 及三语文档验收。

04-02／04-03 [路径编码和请求正文边界](04-02/README.md#路径编码和请求正文边界)通过固定 Laravel 对照、真实 HTTP、Artisan、Redis 维护复验和三语文档验收。

04-02／04-04 [请求旧输入与会话闪存](04-02/README.md#请求旧输入与会话闪存)通过真实 Array／MySQL 会话、Redis 阻塞、固定 Laravel 对照、Artisan 和三语文档验收。

04-02 [请求 Cookie 与上传文件读取](04-02/README.md#请求-cookie-与上传文件读取)通过实际 HTTP、固定 Laravel 对照、MySQL 8.4.13 验证与认证回归、Artisan 和三语文档验收。

04-02 [请求全部输入与文件合并](04-02/README.md#请求全部输入与文件合并)通过真实 HTTP、固定 Laravel 对照、Artisan 和三语文档验收。

04-02 [类型化请求回调与字段筛选](04-02/README.md#类型化请求回调与字段筛选)通过真实 HTTP、固定 Laravel 对照、MySQL 8.4.13 会话验证、Artisan 和三语文档验收。

04-02 [请求正文参数与 Fluent](04-02/README.md#请求正文参数与-fluent)通过真实 HTTP、固定 Laravel、MySQL 8.4.13 Fluent 转换、Artisan 和三语文档验收。

04-02 [请求动态字段和类型化 Fluent 子作用域](04-02/README.md#请求动态字段和类型化-fluent-子作用域)通过真实 HTTP、固定 Laravel、Artisan 和三语文档验收。

04-02 [原生 HTTP 请求转换](04-02/README.md#原生-http-请求转换)通过真实 TCP 和应用二进制、固定 Laravel 元数据对照、Artisan 及三语文档验收。

04-02 [原生 HTTP 响应转换](04-02/README.md#原生-http-响应转换)通过真实 TCP、Laravel PSR-7 桥接对照、实际二进制、Artisan 与三语文档验收。

04-02 [路由字符串与结构化响应](04-02/README.md#路由字符串与结构化响应)通过真实 HTTP、固定 Laravel 对照、实际二进制、Artisan 和三语文档验收。

04-02 [Responsable 请求上下文响应](04-02/README.md#responsable-请求上下文响应)通过实际 HTTP、固定 Laravel 对照、Artisan 和三语文档验收。

04-02／05-07 [JSON Resource 与 Responsable 响应生命周期](05-07/README.md#json-resource-与-responsable-响应生命周期)通过固定 Laravel、真实 HTTP、MySQL、Artisan 和三语验收。核心账本维持 66/66。

04-02 [处理器直接返回重定向响应](04-02/README.md#处理器直接返回重定向响应)通过真实 HTTP、固定 Laravel、Artisan 和三语文档验收。核心账本维持 66/66。

04-02 [重定向 URL 片段](04-02/README.md#重定向-url-片段)通过真实 HTTP、固定 Laravel、Artisan 和三语文档验收。核心账本维持 66/66。

04-02 [重定向同源限制](04-02/README.md#重定向同源限制)通过真实 HTTP、固定 Laravel、Artisan 和三语文档验收。核心账本维持 66/66。

04-02 [响应 Cookie 链式设置与删除](04-02/README.md#响应-cookie-链式设置与删除)通过真实 HTTP、固定 Laravel、Artisan 和三语文档验收。核心账本维持 66/66。

04-02 [响应异常附加与提前返回](04-02/README.md#响应异常附加与提前返回)通过真实 HTTP、固定 Laravel、Artisan 和三语文档验收。核心账本维持 66/66。

04-02 [响应型 abort 与请求捕获边界](04-02/README.md#响应型-abort-与请求捕获边界)通过真实 HTTP、固定 Laravel、Artisan 和三语文档验收。核心账本维持 66/66。

04-02／08-04 [HTTP 章节与 2702d577b 发布快照复验](08-04/README.md#http-章节与-2702d577b-发布快照复验)通过整仓、实际独立发布及三语站点复验。核心账本维持 66/66。

00-03／03-06／08-04 [Windows ARM64 文件锁与 Artisan 源码入口复验](08-04/README.md#windows-arm64-文件锁与-artisan-源码入口复验)通过来宾原生 ARM64 MSVC、实机 Artisan 与 HTTP。托管 DynamoDB 和 Windows Docker Desktop 仍按各自边界跟踪。

06-04 [Windows ARM64 Worker 强制终止与异常次数](06-04/queue-crash-exception-count.md#windows-arm64-worker-强制终止与异常次数)通过 Windows ARM64 SQLite／文件缓存强制终止、真实 HTTP、Artisan 和 macOS MySQL／Redis 复验。核心账本维持 66/66。

06-04 [Windows ARM64 MySQL 与 Redis Worker 崩溃恢复](06-04/queue-crash-exception-count.md#windows-arm64-mysql-与-redis-worker-崩溃恢复)通过 MySQL／文件缓存、Redis／Redis 缓存的真实 HTTP、Artisan 和三语文档复验。核心账本维持 66/66。

06-05 [Windows ARM64 后台调度与 Redis 互斥](06-05/scheduler-output-background.md#windows-arm64-后台调度与-redis-互斥)通过原生 Windows 的真实 Artisan、Redis 以及后台进程的三种退出路径。核心账本维持 66/66。

06-05 [Windows 后台调度关闭父输出管道](06-05/scheduler-output-background.md#windows-后台调度关闭父输出管道)通过 Windows ARM64 原生 EOF 回归、真实 Artisan、Redis、文件缓存和 macOS 复验。核心账本维持 66/66。

07-04 [Windows ARM64 Concurrency 延后执行与管道](07-04/README.md#windows-arm64-concurrency-延后执行与管道)通过原生 Windows 的实际 HTTP 和 Artisan、macOS 回归及三语验收。06-03 Background 的 Windows 管道差异另记。核心账本维持 66/66。

06-03 [Windows ARM64 Background 输入与父输出分离](06-03/lifecycle-failover-drivers.md#windows-arm64-background-输入与父输出分离)通过原生 Windows 真 Artisan、stdin 与双管道回归及 macOS 复验。核心账本维持 66/66。

07-04 [process defer 在终止阶段脱离运行](07-04/README.md#process-defer-在终止阶段脱离运行)通过真实 HTTP、Artisan、Redis 8.10.3 Worker、公开接口进程回归和三语验收。核心账本维持 66/66。

07-04 [defer 在执行边界捕获 Context](07-04/README.md#defer-在执行边界捕获-context)通过真实 HTTP、Artisan、Redis 普通／隔离 Worker 和三语文档验收。核心账本维持 66/66。

06-03 [生命周期连接在终止回调捕获 Context](06-03/lifecycle-failover-drivers.md#生命周期连接在终止回调捕获-context)通过真实 HTTP、Artisan、MySQL 8.4.13、Redis 8.10.3 和三语验收。核心账本维持 66/66。

08-04 [Docker 非特权运行的包清单路径](08-04/README.md#docker-非特权运行的包清单路径)通过真实 Artisan、HTTP 和 Linux UID 10001 挂载权限验收。核心账本维持 66/66。

08-04 [原生 Linux 完整镜像包清单复验](08-04/docker.md#原生-linux-完整镜像包清单复验)通过原生 Ubuntu x86_64 完整镜像、只读容器的真实 Artisan、HTTP、MySQL 8.4.13 和 Redis Worker。实机发布环境检查维持原范围，核心账本维持 66/66。

08-04 [ARM64 完整镜像包清单复验](08-04/docker.md#arm64-完整镜像包清单复验)通过完整 arm64 镜像的交叉构建及原生运行，真实 Artisan、HTTP、MySQL 8.4.13 和 Redis 8.10.3 Worker 通过。核心账本维持 66/66。

01-02 [三语容器文档同步已交付行为](01-02/README.md#三语容器文档同步已交付行为)已修正解析、回调和构造器章节的早期待办描述，三语页面和导航通过。核心账本维持 66/66。

06-08 [三语 Artisan 入口与 Windows 验收对齐](06-08/README.md#三语-artisan-入口与-windows-验收对齐)已同步 Runner 与独立 Artisan 的入口职责，并对齐 Windows ARM64 源码启动器验收。核心账本维持 66/66。

06-08 [Artisan 门面程序化调用](06-08/README.md#artisan-门面程序化调用)通过双应用真实 HTTP、嵌套命令、独立 Artisan 与三语文档验收。核心账本维持 66/66。

06-08／06-03 [Artisan 命令队列](06-08/README.md#artisan-命令队列)通过真实 HTTP、MySQL 8.4.13／Redis 8.10.3 独立 Worker、重试、失败记录及后续消费。核心账本维持 66/66。

00-03／06-08 [Artisan 开发 HTTP 服务](06-08/README.md#artisan-开发-http-服务)通过真实 HTTP、独立 Artisan、地址占用回退、环境文件重载及 Linux 关停验收。Windows 新增关停路径待实机复验。核心账本维持 66/66。

06-08 [Artisan 开发进程注册与列表](06-08/README.md#artisan-开发进程注册与列表)通过真实 Artisan、两应用注册隔离、筛选与终端格式及三语文档验收。下一批为 `dev` 并发执行。核心账本维持 66/66。

06-08 [Artisan dev 并发执行与关停](06-08/README.md#artisan-dev-并发执行与关停)通过真实 HTTP、SQLite 队列监听、重试、退出码及 Unix 信号验收。交互终端、Windows 进程树回收与实机验收与 Pail 分别跟踪。核心账本维持 66/66。

06-08 [Artisan dev 标签与流式终端](06-08/README.md#artisan-dev-标签与流式终端)通过 Ubuntu 实际伪终端、交互操作、日志回放及恢复验收。Windows 进程树与 Pail 继续跟踪。核心账本维持 66/66。

06-08 [Artisan dev Windows 进程树与 Ctrl+C](06-08/README.md#artisan-dev-windows-进程树与-ctrlc)通过 Windows ARM64 真实 Artisan、控制台信号及作业回收。Windows 队列夹具默认栈、Faker 编译门禁及 Pail 分别跟踪。核心账本维持 66/66。

06-04／06-08 [Windows ARM64 Debug 作用域与堆上状态](06-04/worker-hard-timeout.md#windows-arm64-debug-作用域与堆上状态)通过默认 1 MiB 栈下的真实 Worker、强制超时、失败回调与记录、后续消费、HTTP 和关停验收。Windows 与 Ubuntu 全目标严格 Clippy 及三语文档检查通过。Faker 编译门禁已提交于 `6796629d9`，Process 与文件系统测试导入同批限定平台。核心账本维持 66/66，Pail 单列后续。

01-04／06-08 [同步日志监听](01-04/log-listeners.md)通过真实 HTTP、独立 Artisan、22 项日志／Context 回归、严格 Clippy 和三语文档检查。Pail 固定对照与交付顺序已登记，后续接通捕获和命令。核心账本维持 66/66。

06-08 [Pail 会话捕获](06-08/pail-capture.md)通过真实双应用 HTTP、独立 Artisan、MySQL Worker、并发来源隔离、跨进程文件与根二进制包发现验收。三语正文同步。已解析认证身份可捕获，PHP 自动解析差异、Pail 命令及平台发布矩阵继续跟踪。核心账本维持 66/66，不将该扩展范围计作整体 Pail 已闭合。

06-08 [Pail 消息对象与筛选](06-08/pail-options.md)通过固定 PHP 语料、真实 HTTP→Artisan 筛选、6 项消息／筛选与文件回归、严格 Clippy 和三语文档检查。[认证边界](06-08/pail-auth-boundary.md)仍未闭合；Provider 环境门禁、打印器、会话命令与 dev 注册继续推进。核心账本维持 66/66。

00-02／06-08 [Pail Provider 环境门禁与延迟解析](06-08/pail-provider.md)通过实际 HTTP／Artisan、配置缓存与环境覆盖、MySQL Worker、根发现与启停、严格 Clippy 和三语文档检查。runningUnitTests 在未绑定时无解析，已绑定时精确匹配并传播解析错误。核心账本维持 66/66；额外发现 07-01 路径 Helpers 的 storagePath／useStoragePath 入口尚缺，已列为下一依赖，Pail 整体未闭合。


00-02／07-01／06-08 [Application 存储路径与 Pail 目录](07-01/storage-path.md)通过真实 HTTP、独立 Artisan、MySQL 8.4.13 Worker、失败回调与记录、隔离来源、严格 Clippy 和三语文档检查。路径 API 与 Pail 依赖闭合，其他存储消费者迁移、认证自动解析、打印器和命令继续跟踪。核心账本维持 66/66。


00-02／07-01／06-08 [存储路径消费者与配置上下文](07-01/storage-consumers.md)通过 55 项定向测试函数、全目标严格 Clippy、真实 HTTP／Artisan 文件读写与链接、维护模式、缓存前后路径及三语文档验收。存储路径消费者依赖收口，Pail 打印器、实时命令和认证自动解析继续跟踪；Blade 标量算术表达式亦单列后续，不能据路径测试宣称任意 PHP 表达式已支持。核心账本维持 66/66 的既有范围。


06-08 [Pail CLI 打印器](06-08/pail-printer.md)通过 432 组固定 PHP 对照、真实 HTTP／独立 Artisan、PTY、管道失败、23 项根回归、严格 Clippy 与三语文档检查。打印器 API 已验收，实时命令、认证自动解析及发布矩阵继续跟踪；核心账本维持 66/66，不将该扩展范围计作 Pail 整体闭合。


06-08 [Pail 实时命令](06-08/pail-command.md)通过真实 HTTP／独立 Artisan、文件跟随与并发隔离、超时／信号及取消清理、根二进制 PTY／键盘 Ctrl+C、23 项根回归、严格 Clippy 与三语文档验收。Pail 命令实现闭合，dev 默认注册、认证自动解析和 Windows 发布矩阵继续跟踪；核心账本保持既有 66/66。


06-08 [Pail 默认开发进程](06-08/pail-dev.md)通过已加载 Provider 身份与生命周期、默认项优先级及隔离、MySQL 8.4.13 实际 HTTP／Artisan／TTY、两种构建的通用 PTY、50 项及默认构建单项、严格 Clippy 和三语文档检查。Pail 命令与默认 dev 进程已验收；认证自动解析、Windows 发布矩阵及已记录的 Blade 算术继续跟踪。核心账本保持既有 66/66，不外推为整个框架完成。


## 2026-11-19 Blade 标量算术增补

03-07／07-01 [Blade 标量算术](07-01/blade-arithmetic.md)通过 962 组固定 PHP 与真实 HTTP、33 项回归、根二进制／Artisan、严格 Clippy 和三语文档验收。补齐存储路径消费者验收发现的标量算术缺口。核心 66/66 保持原有边界，不代表任意 PHP 或所有 Blade 文本形式已兼容；JSON 非有限数参数边界及指令词边界／空白规则单列后续，Pail 认证与 Windows 发布矩阵仍未闭合。


## 2026-11-19 Blade 指令边界与引擎空白增补

03-07 [指令边界与引擎空白](03-07/blade-statements.md)按固定 PHP 完成 143 组语料、2 组覆盖、5 组片段、962 组算术回归、39 项最终测试、实际根 HTTP／Artisan、严格 Clippy 和三语文档检查。片段 fallback 与原缓冲区的空白边界已明确，未通过修改错误预期掩盖差异。核心 66/66 仅保持既有范围；完整源码预处理顺序及式内注释仍独立推进，不能将本批当作所有 PHP／Blade 行为完成。Pail 认证与发布矩阵仍跟踪。


### 2026-11-19 Blade 全源预处理增量

03-07 [Blade 全源注释与原样块预处理](03-07/blade-preparation.md)完成原样块保护→全源注释消除→组件／指令／回显解析，补齐字符串／指令名称参数／组件属性中的注释与原样内容恢复，缓存版本 32。198 组固定 PHP 对照（184 成功／14 错误）、962 组算术回归、39 项测试、实际根 HTTP／Artisan、严格 Clippy、格式、三语站点和 12＋6 项浏览器检查全部通过。8 份改动源码 SHA 一致，32 份启动／配置／路由／Cargo 基线不变。专属远程产物约 5.45 GB、本地 77.12 MB 与新临时 Composer 镜像已直接清理，保留全部数据卷及共享依赖；项目 Available 80,680,420 KiB（82.62 GB）。核定 15 文件本地提交，不推送。JSON 非有限数跨 ViewData、其他 PHP 语法边界、Pail 身份与 Windows 矩阵仍跟踪，不将核心 66/66 当作全框架完成。下一步按最新授权将 API 扩展与 v0.1.0 发布前缺口纳入现有计划和账本，正式发布须等用户验收及明确授权。


<a id="v010-acceptance"></a>
## v0.1.0 用户验收与发布门禁

2026-11-19 按最新真人指令核定的已知缺口及发布前待复核范围。此表沿用现有 plan／progress／单元审计，不另建平行账本。状态区分“已验收范围”“待实现／核对”“待目标环境”；环境和凭据缺失不是实机通过，也不等于立即阻断其他独立开发。候选清单不是已证明没有遗漏，后续发现按同一表追加。

| 编号 | 当前状态／证据入口 | 关闭条件与剩余工作 |
| --- | --- | --- |
| V01 Blade 全源预处理 | **本批闭合**：[03-07](03-07/blade-preparation.md)，提交 `976b0f908`，198 组固定 PHP／HTTP、39 项测试、根 HTTP／Artisan、严格 Clippy 和三语验收通过。 | 原样块保护与式内／名称参数间注释已补；无新增影响不重复本批验收。不能扩展为整个 PHP 编译器闭合。 |
| V02 非有限数 ViewData 边界 | **本批闭合**：[原生值与 JSON 边界](03-07/view-data.md)。有序原生容器、非有限数／负零传递、递归编码错误、嵌套比较和缓存 33 已验收。 | 108 组固定 PHP、22 个视图／Blade 测试、邮件视图与真实 MySQL 分页、双应用 HTTP／缓存、根 HTTP／Artisan、严格 Clippy、三语构建和 12＋6 浏览器检查通过；具体限制见记录，不扩展为任意 PHP 或平台矩阵闭合。 |
| V03 其他 Blade 表达式契约 | **本批闭合**：[表达式契约](03-07/expression-contracts.md)。原生数组展开、include 接线、缓存 34、PHP 语法拒绝和明确 Rust 映射。 | 96 组固定 PHP 两次一致、27 项定向测试、编译／渲染生命周期、双应用 HTTP、根 HTTP／Artisan、全目标严格 Clippy、三语构建和 12＋6 浏览器检查通过。函数／引用／对象重载通过明确 Rust 接口适配，不承诺任意 PHP 解释器。 |
| V04 Pail 自动身份与时序 | **本批闭合（Rust 显式 await 适配）**：[请求日志时序](06-08/pail-auth-timing.md)。请求日志入口及内置 HTTP 异常报告等待 guard，既有同步日志读取身份快照。 | 47 项定向函数及 Pail harness、登录／登出／切换、异常／取消、跨请求及多应用、实际 MySQL Worker、根 HTTP→Pail／Artisan、严格 Clippy 和三语 12＋6 浏览器通过。无阻塞或递归认证；普通同步 Log::info 不能等待异步 guard，边界已明确。 |
| V05 Pail Windows 原生终端 | **待目标验收**：[Pail 命令](06-08/pail-command.md) 的 Linux PTY 不算 Windows 证据。 | 现有 Windows 虚拟机复查工具链；原生终端／文件跟随／轮换／截断／UTF-8、超时、Ctrl+C／管道关闭、进程树／会话／输出状态清理，ARM64 与承诺的 x64 目标分开。 |
| V06 Passport OAuth2 服务端 | **部分验收**：[Passport-01](passport-01/README.md) 配置／RSA 密钥／五表发布已过；[Passport-02 首批](passport-02/README.md) 真实模型仓储查询／创建／更新／秘密轮换和 scoped 缓存已过 24 项测试、MySQL／HTTP／Artisan 与三语检查；[模型连接回退](passport-02/connection-fallback.md)的直接查询／保存、事务连接校验和已绑定连接保留已通过真实 MySQL／HTTP／Artisan；[scope 与显式追加](passport-02/scopes-and-appends.md)已过固定 PHP 312 例、29 个 Rust 测试函数与三语检查；[客户端工厂](passport-02/client-factory.md)及[命令／关系／UUID／撤销链](passport-02/client-management.md)已过；[PHP 契约与 HTTP 管理](passport-02/contracts.md)已过 2699 条固定 PHP 对照、48 个 Rust 测试函数及真实认证 HTTP／MySQL，Passport-02 计划范围完成验收；Passport-03 至 07 仍待。 | 客户端、密钥／五表、授权码＋PKCE、设备授权、访问／刷新、撤销／过期／清理、scopes、Guard 和协议错误；Password／Implicit 保持默认关闭，旧代码须适配并真实 MySQL 8.4／HTTP／Artisan 验收。 |
| V07 install:api --passport | **待实现**：[Passport-08](../plan.md#unit-passport-08)。当前 install_routes_command 未定义 passport 选项，已验收的默认 Sanctum 安装不覆盖它。 | 包装配、配置／密钥／迁移／路由、选项和提示；干净生成应用，首次／重复／强制／失败与多级前缀；API 仍按需生成。 |
| V08 认证扩展整合 | **待新增影响路径验收**：[Passport-09](../plan.md#unit-passport-09)；既有 [Sanctum-03](sanctum-03/README.md) 验收保留。 | Sanctum／Passport 共存、模型与 UserProvider／连接定制、令牌归属、API/Web、跨应用和真实第三方 HTTP 客户端。未受影响的既有 Sanctum 路径不无故重跑。 |
| V09 相关 API 扩展 | **纳入第一代，待实现／验收**：[API-F01／02、S01、P01、O01／02](../plan.md#unit-api-f01)。包源码版本待各单元开工固定；旧版 Fortify／Precognition／OIDC RP 可提取。 | 无界面认证、2FA／Passkeys、第三方 Socialite、Precognition 和 OIDC RP／双应用 SSO；每项有独立协议、依赖和实际失败路径。Passport 不等于 OIDC；完整 OP 不是现有交付，不对外宣称。第三方账号／回调域名不足时先完成协议服务，列精确缺失条件。 |
| V10 Windows Docker Desktop | **目标前置待复查**：[08-04 实机条件](08-04/README.md#剩余实机环境复查)。旧来宾检查未具备 Desktop／WSL 虚拟化，本次未冒充重新实机通过。 | 可登录 Windows 主机、真实 Linux 容器后端、Docker／Compose／Buildx、隔离挂载权限；构建、路径／权限、迁移、HTTP／Artisan／Worker、退出和数据保留。已有原生 Windows 和 macOS Docker 结果不能替代。 |
| V11 生产 TLS 部署 | **待真实目标**：[08-04](08-04/README.md#剩余实机环境复查)。本地 CA／TLS 1.2／1.3 和可信代理已过。 | 授权测试域名／HTTPS URL、受信任证书链、反代目标和对应实例只读 Artisan；核对证书、转发／签名、HTTP 与运行发布版本。未经授权不部署生产。 |
| V12 托管 AWS SQS | **待真实目标**：[SQS 模拟补测与权限](08-04/README.md#剩余实机环境复查)。标准／FIFO SigV4 协议模拟已过。 | 两条隔离队列 URL、区域、可用 IAM／临时会话凭据、所需 SQS／KMS 权限和 MySQL 账本；实际发送／消费／可见性／重试／失败与 FIFO 顺序／去重。此前凭据缺失记录需在目标提供时再核实。 |
| V13 托管 Pusher TLS | **待真实目标**：[08-02](08-02/README.md)。受控 Pusher、Reverb 和 Redis 扩展已过。 | 授权 Pusher 应用／集群／TLS 配置，真实发布／订阅、认证／重连、错误与部署，不能以协议模拟替代托管验证。 |
| V14 真实邮件／短信及通知供应商 | **待目标投递**：[06-06](06-06/README.md)。SMTP／Mailpit、队列、供应商协议、Slack 载荷均有验收。 | 对承诺驱动逐个列测试账号、API key／发件域名／SMTP 证书、受控收件箱／号码／Slack 工作区和权限；实际回执、最终收信、TLS、失败重试与退订等适用行为。无账号先补自动化，不能称已送达供应商最终用户。 |
| V15 其他数据库／云驱动目标 | **继续对照既有记录复查**：[08-04](08-04/README.md)、[S3](03-03/README.md)。SQLite、MySQL 5.7／8.0／8.4、MariaDB、PostgreSQL 10–18、SQL Server 2017／2022、原生 Linux／Windows 的已过范围保留。 | 独立 ACL 的 SeaweedFS／BucketOwnerEnforced **已过**，本次纠正旧账本残留待办；不重复列缺口。AWS 公网／真实角色与 STS、S3 厂商及大对象边界仍需对应目标。Passport-01 的 MySQL 8.0.46 CLI→8.4.13 dump 组合失败，匹配 8.4.13 原生客户端回归已过，前一组合仍待核；Oracle MySQL 发行版不代表 Oracle Database 驱动已支持；商业端点／额外驱动先核对实际承诺范围。 |
| V16 三语正文与 API 文档 | **待发布范围全量核对**：[逐页映射](../plans/laravel-docs-delivery-map.md)。本批扫描每语 106 个 Markdown，其中 30 个纯空文件；含 index／生态占位，数量不等于 30 个功能缺失。 | Passport 三语 installation／deploying／configuration 已随 Passport-01 闭合，其余 OAuth 小节待后续；Fortify／Socialite／Precognition 三页仍为空，须随实现闭合；其他空页按导航／已实现功能逐项确定正文、Rust 映射或不支持范围，不能空页冒充正式功能。安装／升级／部署和 API 参考与发布快照一致，构建、链接、示例与浏览器全过。 |
| V17 最终干净快照整合 | **待全部实现闭合后执行**：[08-04 发布回归](08-04/README.md#当前主线整仓回归与独立发布复验)。历史发布快照不覆盖后续 API 新增。 | 锁定干净提交的根应用＋生成应用安装／打包／迁移、HTTP／Artisan／Worker／调度、支持 OS 与数据库目标矩阵、必要整体失败／恢复回归；每个实际环境注明版本和证据，清理不删数据。 |
| V18 Git／依赖／版本和最终批准 | **待发布审计及用户验收**：既有 `976b0f908`、`ef35334c8` 及后续单元均仅本地，未推送。原始历史补丁、core/、translation-database/ 继续保留并分离。 | Passport-01 引入 rsa 0.9.10 涉及 RUSTSEC-2023-0071，本批仅密钥生成／解析，无在线私钥签名解密；未加入 Cargo audit 忽略，发布前仍核受修复版本／可达性。审核 WIP 归属、依赖／许可证／敏感信息、CHANGELOG 与版本元数据变更计划；按单元给提交号和验收证据，交用户逐项验收。用户确认无问题并明确授权后才改发布版本／打 tag／创建 Release／推送／生产部署。 |

### 当前复核结论与下一步

这次只关闭 V01，并纠正 V15 的已过 ACL 残留；没有把旧记录里的模拟环境算作真实供应商，也没有因历史 66/66 把 V02–18 勾为通过。API 扩展按计划分为 9 个 Passport 单元与 6 个相关 API 单元，Passport-01 已通过实际 MySQL／HTTP／Artisan／三语验收，其余 14 个单元待闭合；Sanctum-01 至 03 保持已验收范围。下一代码单元为 Passport-02；ViewData、Pail 和发布目标条件继续在本表独立跟踪。

本次计划增量只核对固定源码、旧版和既有日志、页面空占位与链接，不声称已经完成全契约或全云厂商审计，也未重新扫描／暴露凭据。每批结束核对本地提交、实际验证及资源清理；目标缺失仍能推进无关单元，直到已授权范围完整收尾，再交用户验收。


### 05-03 forceFill 转换职责与 Passport-02 前置

2026-11-19 05-03 [forceFill 保留属性转换](05-03/force-fill.md)作为 Passport-02 必要前置完成。固定 Laravel forceFill 只绕过保护、仍执行 fill；主线原始属性直写已修正为 Result，同步转换失败传播，并提供任务隔离的 forceFillAsync 走既有应用哈希线程池。旧版 mass_assignment 的嵌套／错误／pending／取消规则已复用。30 个不同测试函数通过：最终 4 项专项（含真实 TCP→Kernel→MySQL 哈希落库及三语示例）、MySQL 8.4.13 三项模型／哈希／关系回归、根 23 项 Console／Feature／Artisan。关系完整执行 525.79 秒通过；最低 Rust 1.94 格式／features pail,passport 全目标严格 Clippy、三语站点及 12＋6 浏览器检查通过，./artisan inspire 在 1.94 与默认 1.99.1 均退出 0。根测试首轮读取 Nov18 既有配置缓存导致未注册模型 500；已保留缓存，以本批专属缓存路径复验通过，原 SHA／mtime 不变。1.99 Debug unwind 表链接警告如实记载，不改共享构建 flags。9 份源码／文档 SHA、56 份入口／配置／路由／Cargo／Passport 基线核对通过。核定 13 文件本地提交，不推送；协作者新增 install:full 的 plan／context 改动保留并排除。已直接回收本地逻辑 25.170 GB（df 增量 21.909 GB）、远程 24 KiB 包装目录与本批 MySQL 镜像，全部卷、共享缓存和原有 WIP 保留；本地可用约 82.45 GB。Passport-01 提交 9f086981252fde12c756afb54fd6e9c71cd9b7a8 已核实。Passport-02 仍待 Client／ClientRepository／passport:client，不将前置修复当作整单元完成；继续 UUID 查询门禁、临时明文／哈希、模型生命周期与 request-once／应用隔离。按最新授权取消人为额度停线，正式额度工具不可用，禁用 credits，Goal active。
