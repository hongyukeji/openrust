# Laravel 核心功能闭合对账

基线为 2026-11-13 本地提交 `10a19965`。以[固定 Laravel 基线](../plans/laravel-rebuild-baseline.md)、[交付计划](../plan.md)和[章节对照](../plans/laravel-docs-delivery-map.md)核对实现和验收。

每个编号单元权重相同。已实现并通过适用验收、单元内没有已知核心缺口，才记为闭合。Rust 等价做法需记录语言约束及可观察行为证据。部分验收只记录已通过范围。阶段内的核心单元全部闭合后才记为阶段完成。

当前核心单元 **21/66（31.8%）**，原路线图九个阶段全部有范围验收记录，彻底闭合 **0/9**。核心分母含 00–07 的 61 项、框架广播 08-01 和测试能力 T 的 4 项。

| 阶段 | 闭合数 | 已闭合单元 | 未闭合单元及主要缺口 |
| --- | ---: | --- | --- |
| 00 启动与配置 | 2/3 | 00-02 默认 HTTP、静态资源与错误格式；00-03 Artisan 基础入口 | 00-01 配置缓存、环境文件加密 |
| 01 容器、日志与异常 | 5/7 | 01-01 基础容器与 Provider；01-03 基础日志；01-05–07 Context、异常报告和渲染 | 01-02 组件依赖型上下文属性和容器完整闭合复核；01-04 日志 formatter、processor、tap 等 Rust 对应行为 |
| 02 数据库与集合 | 2/9 | 02-03 基础查询；02-07 Seeder | 02-01–02 多结果集、连接与事务事件；02-04–06 高级谓词、列类型与约束、跨进程迁移隔离及 schema dump/load；02-08–09 集合方法和数据库游标接线 |
| 03 应用基础服务 | 2/9 | 03-03 S3；03-04 加密和密钥轮换 | 03-01 文件目录和元数据；03-05 自定义哈希驱动；03-06–09 缓存标签、故障切换、Redis Cluster、Blade 属性包和动态组件等；03-10 国际化扩展行为复核 |
| 04 HTTP 请求链 | 3/9 | 04-06 Web 表单回传；04-08 异常报告限流；04-09 上传、下载与普通对象 URL | 04-01–05 路由诊断、请求及控制器接口、可信代理与转发 HTTPS、Session 上一 URL 与额外驱动、文件和数据库验证规则；04-07 认证模型属性配额及额外 Redis 部署形态 |
| 05 Eloquent、认证与 API | 1/9 | 05-05 路由模型绑定 | 05-01–04 模型事件映射、复制及自动唯一 ID、部分关系和 Pivot 契约、Cast 和工厂多态；05-06–07 Blade 分页、部分 Resource 推断及条件接口；05-08–09 HTTP Basic、认证事件、其他设备失效、密码确认、Gate 事件及 Blade 授权指令 |
| 06 URL、后台任务与通知 | 3/8 | 06-05 调度；06-07 密码重置和邮箱验证；06-08 Artisan 帮助及交互本地化 | 06-01–02 URL／重定向边界、事务后 Observer；06-03–04 外部队列并发副本仍需应用幂等、SQS 托管 AWS 验收；06-06 部分邮件传输、通知和 Fake |
| 07 框架专题 | 0/7 | 各有范围验收 | 07-01–07 Helpers／Strings、HTTP Client 批处理及事件、Processes 的 TTY／waitUntil、Concurrency 非 HTTP 延迟执行、图像格式和动画、PostgreSQL 全文搜索、包资源发布和发现 |
| 08-01 框架广播 | 0/1 | 频道授权与分发有范围验收 | 模型频道参数绑定、匿名广播及广播唯一任务 |
| T 测试能力 | 3/4 | T-01 测试应用；T-02 HTTP／Artisan 测试入口；T-04 容器与事件替身 | T-03 RefreshDatabase 事务策略及其他驱动测试库管理 |

## 05-01 模型生命周期

SQLite 和 MySQL 8.4 已验证模型持久化、读取和软删除事件，以及静默和取消。事件映射和复制继续跟踪。 [验收](05-01/README.md#模型生命周期派发)

## 06-01 重定向闪存

已接入链式闪存与命名错误包，并通过默认 Web 中间件、加密会话和实际表单验收。已接入控制器动作 URL 和重定向，实测覆盖、域名优先与应用隔离。具名和位置模型参数已按自定义绑定列生成 URL，并用 SQLite／MySQL 实测。本地临时链接回调和常用 S3 签名选项已验收。S3 独立签名基地址和指定对象版本已通过 RustFS 实测。高级 S3 SDK 请求构建器已用 RustFS 验收 Range 下载与条件上传。模型访问器与类型转换已通过 SQLite／MySQL 8.4 HTTP 验收。可信代理归于 04-03，S3 独立 ACL 服务复验仍单独跟踪。 [URL 验收](06-01/README.md#重定向闪存)

## 06-02 动态事件发现

编译监听器清单支持 `ShouldBeDiscovered`，通过两个应用的真实 HTTP 与 Artisan 验证条件过滤、隔离及注册失败行为。生产事件缓存通过独立 Artisan 进程和真实 HTTP 验收。Eloquent Observer 注册、写前取消和写后审计已用 SQLite 和 MySQL 8.4 验证。Observer 生成器已验收。事务后 Observer 继续跟踪。 [事件验收](06-02/README.md#动态发现)

## 06-05 调度收尾

账本登记的四类缺口已验收。覆盖写、追加写、输出钩子、URL 探测与后台完成事件均通过真实 `schedule:run`，共享缓存的重叠锁在完成与失败后均可重新取得。输出邮件、清锁和手工测试命令随后通过实际 Artisan 验收，06-05 核心主线闭合。 [调度验收](06-05/scheduler-output-background.md)

## 本轮已验收的增量

以下增量已提交到本地。所属单元仍有表中列出的其他缺口，闭合数保持 21/66。

| 单元 | 实际验收 | 提交 |
| --- | --- | --- |
| 00-02 | 健康页 HTML／JSON、诊断失败和维护模式例外 | `132dc081`、`0294c666` |
| 02-04、02-05 | MySQL 8.4 JSON 查询与列修饰符 | `3fc79e0d`、`ac1e44d4` |
| 03-06、03-08、03-10 | Redis 请求及任务内缓存、Blade 条件子视图、Artisan 发布语言文件 | `733ad42b`、`8fcc9724`、`f6719659` |
| 04-04、04-07 | MySQL Session 与 Redis 锁、Redis 跨进程数字节流 | `55766215`、`917903f2` |
| 06-02、06-03 | MySQL 事件延迟派发、自定义队列连接及 MySQL／Redis Worker 复验 | `81541c47`、`daff67a0` |
| 07-06 | MySQL 全文索引、事务后更新和 HTTP 分页复验 | [Search 验收](07-06/README.md) |
| 06-05 | 任务输出邮件、清除重叠锁和手工运行 | `36bd30ee`、`e7ff17e5` |

## 扩展与发布

| 范围 | 基线状态 | 待验收或确认 |
| --- | --- | --- |
| [08-02 Reverb](08-02/README.md) | WebSocket 协议和 Redis 跨进程基础已有实测 | 全局在线成员、跨节点客户端事件及协议剩余接口 |
| [08-03 gRPC](08-03/README.md) | 服务端已用真实 Tonic 客户端验收 | 命名客户端、凭据及拦截器 |
| [其他生态包](../plans/laravel-docs-delivery-map.md#packages) | Fortify、Passport／OIDC SSO、Sanctum、Scout、Horizon、Dusk 等单独跟踪 | 各包范围和完整交付验收 |
| [08-04 发布](08-04/README.md) | macOS ARM64、Linux ARM64 和部分 Windows 运行路径已有实测 | Linux x86_64、Windows 完整部署、生产 TLS 与目标数据库版本矩阵 |

## 额外队列驱动与载荷定稿

已接通 Null、Deferred、Background 和 Failover。Rust 任务载荷采用版本化 JSON 与编译注册。Beanstalkd 基础驱动、批次和链中批次已通过真实 Worker。SQS 标准／FIFO 与溢出载荷已通过独立 ElasticMQ、Redis 和真实 Artisan。跨连接转发已验收目标延迟及 SQS 拒绝后的重试。SQS 原生批量发送已验收按条数和字节分块、HTTP 200 逐项失败、恢复和追加。FIFO 已验收 broker 接收后响应失败、SQL 确认失败和并发恢复。标准 SQS 和 Beanstalkd 已实测 SQL 确认失败后的副本重投递与批次结算。并发副本仍需应用幂等，托管 AWS 继续跟踪。 [外部队列批次验收](06-03/external-batches.md) [Beanstalkd 验收](06-03/beanstalkd.md) · [SQS 验收](06-03/sqs.md)

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
