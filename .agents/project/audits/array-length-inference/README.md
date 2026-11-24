# 注册列表长度推导

2026-11-24。按用户要求移除需要随列表增删而手动维护的数组长度；不做数字的全仓替换。Rust 1.94 支持局部数组长度 `_`，但不允许在 const/static 声明中这样推导，因此局部数组保留数组类型，常量／静态注册表改为只读切片。

## 修改范围

共27个Rust文件、33处声明：基础Provider、中间件栈、源码模板、批次回调阶段、REST动作、HTTP方法、查询绑定类别、扩展迁移登记、名言、Faker候选数据及对应测试列表。按值迭代显式使用`iter().copied()`；迁移／casts等返回切片的入口不再重复借用。查询绑定的Changes数组和HTTP方法返回数组从各自列表的`len()`推导，保留原来的栈数组表示。

列表的初始化内容及顺序逐项核对保持一致，没有改事件名、命令名、数据库标识、URL、默认值或随机选择范围，没有新增Vec堆分配和运行时目录扫描。公开方法的返回类型保持；例如迁移入口仍返回`&'static [MigrationEntry]`。

## 有意保留

- 密钥／nonce／十六进制与Base32／校验字母表的固定宽度：属于算法或协议约束。
- 月份12项、十进制数字10项、邮件优先级5个级别、终端颜色6个角色、Markov模型5个阶数槽位：已有数字索引或明确固定语义，长度同时提供编译期完整性约束。
- EUC-TW压缩映射表71项：编码数据表边界，不是可扩展注册列表。
- `UnionType::SUPPORTED: [&str; 6]`：公开常量的数组类型及标准类型集合保持，避免为省数字变成切片而破坏现有调用。

## 验证及边界

使用项目构建守卫、Rust 1.94.0、项目内独立临时目录。为避免与主对话争用或重写编译缓存，只读复用其已有依赖，按Cargo指纹精确选择依赖工件，在HEAD隔离源码上编译整个框架库并执行Clippy（`-D warnings`）。这属于实际核心库定向编译，不是全workspace Cargo Clippy。

- 隔离核心库编译、严格Clippy与改动文件格式检查通过。
- encryption、vite_facade、eloquent_strictness、lazy_collections四个实际测试crate编译通过；未将metadata编译计为测试执行。
- 从实际短信worker源码提取PROFILES，用正式Clap解析器运行：9个原有选项接受，未知选项拒绝。
- 33处初始化内容／顺序校验通过。没有执行数据库迁移、发送短信或重跑全仓HTTP／SQL测试；本批不改SQL或业务行为。

证据位于`target/tmp/provider-array-inference/`，包括独立源码、编译与Clippy日志、四个测试编译日志、CLI解析结果、列表内容校验及提交清单。一次缓存选择歧义和一次其他工作流磁盘清理导致的依赖工件消失均不计通过；重新按指纹读取已恢复依赖后完成验收。本批没有手动中止其他进程或执行额外缓存清理；期间项目32GB守卫清理的空间与工件变化以公共守卫记录为准。所有临时源文件与日志均在项目内；保留独立证据，不修改其他在途源码或索引分块。

## 实际消费者完整复验（2026-11-24）

针对列表改造提交 `ce75e29be167fe194b47219652a943123a8e9dd5`，以 `d73693e0ce67c3cbf2fbc637307d06f6619c5917` 创建完整独立源码快照，使用 Rust 1.94.0、构建守卫和独占 Cargo target。此次均实际运行测试和二进制，不用前次 metadata 编译结果代替执行。未混入其他协作者的在途改动。

### 注册内容与顺序

重新提取改造提交前后的33处初始化列表，内容与顺序逐项相同。列表改造后其他提交新增的 Provider／模板属于独立提交，不把这些正常新增误判为本批差异。HTTP 方法及 `any`／`match`／HEAD 重叠路由、资源与单例动作、Provider 的 register／boot 时机及幂等性、中间件进入／退出／终止次序均经实际消费者验证。查询绑定测试同时核对九个类别的顺序、扁平参数顺序、SQL参考结果及实际执行。

### 实际测试

| 日志 | 通过的测试执行数 | 验收范围 |
| --- | ---: | --- |
| framework.log | 223 | 路由方法、路由快捷入口、中间件生命周期与排除、查询绑定、加密、Vite、严格属性、LazyCollection、Faker身份／医疗／浏览器数据 |
| application.log | 39 | 根应用启动、真实二进制Artisan、模板发布和生成器、Provider顺序、静态挂载、API/CORS及骨架 |
| http_consumers.log | 35 | 真实HTTP输入与标量转换、资源／单例路由、组与命名路由及缓存 |
| sqlite_seeders.log | 1 | 独立Artisan进程在SQLite执行Seeder |
| faker_consumers.log | 42 | 根应用Faker公开消费者 |
| sqlite_batches.log | 1 | SQLite真实批次派发、Worker、各阶段回调及失败路径 |
| activitylog.log | 17 | 包Provider、HTTP／Artisan迁移与应用隔离 |
| telescope.log | 6 | 包Provider、初始化命令及HTTP观察消费者 |
| cashier.log | 3 | SQLite迁移、Artisan、HTTP回调与持久化；供应商使用隔离模拟服务 |
| pay.log | 2 | SQLite迁移、真实HTTP回调、通知事务与幂等；支付协议使用本地夹具 |
| mysql_bindings.log | 2 | MySQL 8.4.13上的绑定顺序和实际SQL执行 |
| mysql_seeder.log | 1 | MySQL真实Artisan Seeder |
| mysql_batches.log | 1 | MySQL 8.4.13＋独占Redis 8，真实Worker及批次回调、失败／重试路径 |
| postgres_bindings.log | 2 | PostgreSQL 18.7上的绑定顺序和实际SQL执行 |

合计375项测试执行通过，0失败、0忽略；相同用例在不同驱动上的重复执行分别计数，不代表375个独立功能。按名称筛选的其他测试未执行，不计入通过数量。MySQL／PostgreSQL使用项目正式隔离夹具，Redis使用本批专属容器，均未使用开发库或开发缓存。

### 实际应用二进制与Artisan

- `main artisan list --format=json`：134个命令，无重名；核对 `inspire`、`route:list`、`stub:publish`、`migrate`、`db:seed`、`queue:work`、`schedule:run`、`lang:publish` 均存在。帮助正常，未知命令仍退出1。
- 实跑12次 `inspire`，输出均属于原41条名言集合；另外直接执行独立快照的 `./artisan inspire --no-ansi`，退出0。连同清单、帮助和错误分支共19项CLI检查。
- 启动真实 `main`，依次请求首页GET／HEAD、`/up`、普通Web的HTML404、Accept JSON的404、API路径的JSON404，共6项HTTP检查通过。SIGINT正常退出0。此独立首页验收采用array Session／Cache并显式关闭其他Runner，避免连接开发服务；队列另由上述真实Worker测试覆盖，不计此首页验收为Runner全矩阵通过。
- 编译并运行真实短信Worker夹具：帮助中9个profile的内容和顺序一致；9个已知值均通过枚举解析、进入缺少必填参数分支，未知profile仍在枚举解析阶段被拒绝。本项未派发短信，不计作外部短信供应商验收。

### 检查、证据与收尾

- 完整快照 `cargo +1.94.0 fmt --all --check` 通过。
- 根应用、框架及activitylog／telescope／cashier／pay六个受影响crate的 `cargo +1.94.0 clippy --locked ... --all-targets -- -D warnings` 通过；未声称全workspace所有扩展均重跑Clippy。
- 完整命令、原始输出、列表初始化校验、CLI／HTTP结构化结果保留于 `target/tmp/array-runtime/`。本目录 [结构化验收摘要](runtime-2026-11-24.json) 保存测试基线、计数及38份关键源码／清单的SHA256。核心命令由 `run_more.py`、`database_acceptance.py`、`mysql_redis.py`、`binary_acceptance.py`、`sms_acceptance.py` 记录；均在项目内执行。
- 未发现本批列表改造引起的回归，无需修改生产源码或开发者三语用法文档。此次更新仅为验收记录，不扩大为全框架、全部数据库驱动或Windows发布验收。
- 本批HTTP进程和临时MySQL／PostgreSQL／Redis容器已全部退出并回收，无外部短信或真实支付调用。源码快照、日志和可复用独占增量缓存留在项目内；未清理其他协作者产物。收尾磁盘实测约55.5 GB可用，高于32 GB停线。仅本地提交，不推送。

## 全仓遗漏补查

后续检查及补齐见[全仓补查记录](followup-2026-11-24.md)，包含保留项、三语教程消费者和最终验收。

## Release性能与堆分配

[Release实测对照](release-benchmark/README.md)：分配计数相同、稳态基本不变；保留受控首次Faker构造约0.6微秒变慢、对启动上下文敏感的冷路径例外，不宣称所有场景绝对零开销。

## 路由注册分配热点优化

13700次分配的主要来源及后续优化见[路由注册热点记录](../route-registration-allocations/README.md)：93.43%来自路径编译；字面路径优化后128条GET注册为1028次alloc，Release完整注册／销毁耗时约减少90%。
