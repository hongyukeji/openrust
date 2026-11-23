# Rust 原生命名迁移验收

2026-11-23 用户授权：按 N-01 至 N-06 全仓实施、验收后分批本地提交，不推送。起点为 `8ae49738324ae7fcbae0196296d3bb3e137e7fe1`。保留用户环境配置、其他模型验收记录及原暂存的 Unicode／Zend 许可删除，不纳入命名提交。

| 单元 | 状态 | 实际范围及证据 |
| --- | --- | --- |
| N-01 | 验收通过 | 语法树盘点、workspace 清单、调用位置、字符串边界及碰撞检查；无运行代码改动 |
| N-02 | 验收通过 | 177 个基础方法族及跨 crate 消费者；20 项应用、59 项框架、独立生成测试、实际 HTTP／CLI、workspace 与可选 feature 严格 Clippy |
| N-03 | 验收通过 | HTTP／路由／认证／命令及同名调度接口完成命名迁移；隔离构建、实际HTTP/CLI、MySQL、Redis调度、生成器与三语消费验证通过 |
| N-04 | 验收通过 | 数据/基础服务1814组拼写、真实MySQL58项与Artisan/Redis、生成源码、组合Request、三语及workspace/可选features严格Clippy |
| N-05 | 待实施 | 异步服务、协议和全部扩展 |
| N-06 | 待实施 | 全仓复核、三语、Rustdoc、CI、防回归及最终真实消费者 |

## N-01：盘点与边界

- `cargo metadata --no-deps --format-version 1` 确认实际 workspace、目标与 feature；清单位于 `target/tmp/rust-naming/workspace.json`。不凭目录中嵌套夹具的 Cargo.toml 数量判断 workspace 数量。
- `rust-analyzer parse --json` 读取 1,939 份正式应用、框架、扩展、工具及测试 Rust 文件，不改第三方补丁或历史源码。语法节点区分声明、标识符、字符串和注释，UTF-8 字节位置用于核对，不能按全文字符替换。
- 发现 6,416 处待核对声明、3,546 个待迁移拼写、31,286 处标识符位置、1,718 处字符串／注释桥接候选；这是候选计数，不是迁移完成率。定义及消费者存入 `registry.json`，原始 AST 摘要保留在 `target/tmp/rust-naming/inventory.json`。
- 按依赖及同名方法族分配 N-02 至 N-05。一个方法族跨 crate 的定义、trait、实现和消费者必须同批接线，不能留下同名 trait 不匹配。其余服务不借机改业务。
- 单列项目类型 `DB → Db`、`URL → Url`、`OAuthError → OauthError`、`OAuthParameters → OauthParameters`；外部导入不随同改写。`XProvider` 的单字母词不是连续缩略词，不改语义。
- 同作用域的既有 snake_case 与 camelCase 方法存在 51 处潜在碰撞，已记录文件、语法作用域、旧名和目标名。每批必须区分重复别名、私有实现和参数不同的操作，按职责合并或命名，不能批量制造重复定义。不同作用域的 Faker `VAT` 与 `vat` 不强行合并。
- Windows 的 AttachConsole、SetConsoleCtrlHandler、GenerateConsoleCtrlEvent 等 extern 声明保持链接符号，必要时限定局部 lint 例外，不能改坏 FFI。`None`、常量匹配、类型路径不会当作小写变量处理。
- Facade 关联常量 `cached` 单独迁移为 `CACHED`；数据库查询辅助方法 `cached()` 不受此常量迁移影响。

### 字符串与实际消费者边界

| 类别 | 保留内容 | 每批验收入口 |
| --- | --- | --- |
| Serde／资源输出／数据库 | 原 JSON 字段、枚举、数据库列和载荷键；字段改名核对显式 rename | 序列化往返与实际 HTTP／数据库／Worker |
| Blade／动态格式化 | currentLocale、getLocale、DB、URL 等既有模板表达式；仅后端 Rust 接线改名 | 真实视图编译、模板运行、语言与请求对象消费者 |
| 宏／生成器 | 宏参数中的真实 Rust 方法迁移；属性选项、字符串协议保持；Rust 源码字符串单独核对 | derive／trait／Facade 消费者、新生成项目及源码实际编译 |
| CLI／事件／控制键 | 命令、参数、退出码、事件字符串、缓存和容器服务键 | 实际 Artisan、HTTP、Worker、调度及协议消费者 |
| 测试／三语／Rustdoc | Rust 示例迁移；PHP／Blade／JS 等代码块按各自契约保留 | 定向测试、文档构建、真实浏览器与导出检查 |

### 工具核对

使用现有 Rust 解析器和编译器，不引入生产依赖；盘点脚本为 `.agents/project/tools/rust_naming_inventory.py`，不会自动编辑源码。迁移只使用核定的符号与语法位置；字符串候选需要单独裁决。完整计划见 `plans/rust-naming-unification.md`，历史记录保持原貌。

N-01 尚未开始服务代码迁移，不表示任何运行模块重新验收通过。后续批次将追加实际命令、结果、源组合、数据格式对照、异常及剩余项。临时产物均在 `target/tmp/rust-naming/`，按构建守卫管理；不删除依赖下载、数据卷、源码或原索引。


## N-02：基础装配与消费者

N-01 已提交 `f4c755e962d50a607c02d2751fa25b3654b9f6a3`，无推送。本批按 registry 的 177 个拼写族逐项接线；同名跨 crate 定义与实现同步迁移。使用 AST 标识符位置编辑，不替换字符串或注释；三语 Rust 示例及明确的 API 引用单独处理，Blade／PHP／JS 表达式保留。

- Application、Container、Contracts、Provider、Config、Facade 及根 bootstrap 改为原生命名。关联常量 `Facade::CACHED` 与实现、消费者同步；普通 `cached()` 查询辅助方法不改。
- 7 个涉及字段均为无 Serde 的内部状态（应用与路径、Pail、Pulse），不改变外部字段。AST 字符串对照仅出现核定的生成源码／安装锚点、异步 API 提示和一处测试配置；事件、序列化、缓存、环境、CLI 名称与 Blade 表达式未做批量替换。
- 实际编译暴露跨文件 trait／私有入口同名遮蔽：仅将原 `Container::get_async(ServiceKey)` 私有入口改为 `get_async_by_key`，公开 trait 使用 `get_async`，其委托接线同步。不是增加两套公开兼容别名。
- 包清单生成字符串使用 `with_native_views`；安装器、测试锚点、显式编译的 Rust PROBE、两个 Python 内嵌 Rust 消费者及生成测试模板同步。`newInstance` 动态构造键、Blade 的 currentLocale／getLocale／makeWith 等字符串按原契约保留，Rust 后端使用原生命名。
- 移除 220 处不再需要的 non_snake_case 豁免；尚为 N-03／N-05 子模块继承的 crate 级例外保留到所属单元迁完，不提前制造大量告警，也不新增全局豁免。最初过早移除的原有 crate 属性已恢复；失败日志未计为通过。

### 验收与原有缺陷

1. 应用 Console／Container 生命周期／Configuration／Storage Paths／Resolving 共 20 项通过。
2. 框架 locale／Config Facade／Spy／Blade／属性注入／Offsets／管理器／请求注入／SelfBuilding 等 13 组共 59 项通过。
3. 首轮 Blade 测试的一项 None 状态预期在原提交隔离源码上同样失败：自动语言识别已内置，而夹具意图是不启用识别。测试夹具显式设置 `supported_locales: []` 后保持原预期，生产默认值不改；原提交失败原文与 SHA 保留，不通过修改预期掩盖迁移问题。
4. 实际最新二进制验证 GET /、/up、JSON／HTML 404、SIGINT 优雅退出，Artisan inspire、JSON list、install:grpc 及新的 with_grpc 接线均通过。测试运行时使用独立根路径、file Session／array Cache、显式关闭其他 Runner，未改生产配置或访问用户数据表。
5. 最新二进制实际 make:test；在独立 Cargo 项目显式接入项目依赖补丁和测试目标，生成的 with_routing 测试实际编译运行 1 项通过。第一次夹具未带补丁或未声明目标的构建不计通过。
6. `cargo clippy --workspace --all-targets -- -D warnings` 通过。根应用进一步显式组合 sanctum、reverb、pail、passport、fortify、socialite、oidc、settings、pay、cashier、activitylog、horizon、pennant、pulse、scout、telescope 的 all-targets 严格 Clippy 通过；未盲用 all-features，standalone 另属发布场景。
7. 三语站点构建、内容与入口检查通过；Worker Control 三语在 / 与 /docs/ 的真实浏览器检查 6 组通过。Rustdoc API 引用同步，未声称全仓 Rustdoc／所有文档测试已在此批闭合。
8. 受影响 Rust 文件格式化、git diff --check 与 117 处必要品牌标识守卫通过；后续批次继续按实际消费者复验，不以本批编译代替模块验收。

证据目录 `target/tmp/rust-naming/N-02/`：root-tests-final.log、framework-tests-final.log、workspace-clippy-final.log、optional-features-clippy.log、http-cli-results.json、generated-consumer-final.log、docs-build.log、docs-worker-control.log、literal-differences.json、source-hashes.json。独立基线副本已在退出后清理，保留 Git 基线、文件 SHA 和原失败日志；生成消费者与构建缓存保留供后续复用，未删数据库、日志或下载。收尾可用约 40 GiB，大于 32 GB 停线。原暂存两项许可删除和无关改动排除。


## N-03：请求、路由、认证、命令与生成消费者

N-02 已提交 `27c8802cdd94e8d48b6365ba51abccace132bba6`，未推送。本批按 registry 的方法族迁移，不按文本全文替换。Console 下调度方法及 Validation 与 Query 共用的方法族需同时接线，因此相应 spelling 提前纳入本批，N-05 仍需收尾异步服务剩余范围。

### 公开契约与冲突裁决

- 重复别名 hasUser/setUser、StatefulGuard 与路由 scoped-binding 包装合并到既有原生接口，不长期保留两套大小写。Output 与 MessageBag 的重复原生包装合并，保留相同输出/判空行为。
- SessionGuard 原异步 setUser 改为 `set_user_async`，原同步 Guard::set_user 保持同步。直接转成同名会让已有同步调用创建未轮询 Future，已在全 workspace 警告与真实认证测试中核对，不能用丢弃 Future 掩盖。
- AuthManager 私有请求缓存入口改为 resolved_user_from_request，ValidationException 私有包名读取为 selected_error_bag；公开设置/读取接口不互相遮蔽。HTTP 客户端请求级内部脱敏改名 redact_request_payload_only，公开请求/响应同时脱敏保持原语义，SMS 内部调用按原请求级语义迁移。
- CommandFinished、调度输出、RequestHandled、Response、Route、TestApp、Validator 涉及的9个字段均非直接Serde序列化字段；字段访问同步，事件名、JSON键、SQL、环境、CLI、Cookie、URL和模板表达式字符串独立保留。
- 宏实际展开暴露首轮 AST NAME 声明盘点未覆盖的方法。补登记状态断言、尺寸和数据库条件宏；同步定义与消费者。盘点工具新增宏体 FN_KW/IDENT 检测和前导下划线绑定入口，并用真实 Rust 解析器做正反边界检查。N-04 未迁移的关系/Faker宏仅保留原有精确属性，不新增全局关闭。
- Faker user_agent 的 Unique 键保持显式 `userAgent`，格式化器名保留；未来 Faker宏的 stringify 稳定键列为 N-04 专项核对。Google prettyPrint、Fortify confirmPassword、Node exitCode、Playwright setContent/maxRedirects 等外部标识不因Rust方法同名而改写。
- Rust生成模板、Passport安装PROBE、Fortify及模型验收脚本内明确的Rust源代码、安装提示API及三语断言同步。原生代码块与Blade/PHP/JS块分开处理；后者协议不机械改名。

### 真实验收与版本边界

- 工作区请求/路由/认证/Session/验证/观测共16组128项通过，含并行IP单元的11项；6个IP文件与冻结SHA一致。这是组合交集，不代表本批拥有或提交该新增功能。
- 独立命名候选（排除并行Request/IP新增Provider、上下文、路由借用与门面方法）重新构建：应用9组87项通过、1个gRPC子进程夹具ignore不当额外通过；测试中未提供环境的Postgres/MySQL早退不能当真实后端通过。
- 实际临时MySQL 8.4.13：强制环境入口运行Session认证、数据库Session、认证模型、内置迁移共4个MySQL消费者通过；默认单元中SQLite内存分支只是回归，不宣称发布矩阵完成。临时容器及匿名资源已清理，未使用用户数据卷。
- 独立命名候选的HTTP客户端/选项/流式sink、命令观测、消息包、时钟共6组44项通过；真实本地HTTP、事件及输出断言保持。
- 临时Redis 8下真实schedule:run、schedule:list、输出邮件、覆盖/追加、钩子、事件、锁、暂停/维护、应用隔离通过；scheduling 17项和clock 5项，2个自启动夹具ignore由父进程场景消费，不计额外独立通过。临时Redis已清理。
- 独立候选 `cargo clippy --workspace --all-targets -- -D warnings` 通过；不是只用工作区并行功能组合的结果。根实际二进制帮助、inspire、未知命令退出码0/1核对，API/CORS及配置前缀优先级TCP测试属于startup真实场景。
- 独立三语站点构建/内容检查通过，根与/docs/桌面/移动浏览器12组通过；文档原生API更新，必要Blade调用保留。更多feature和全仓Rustdoc仍由N-05/N-06复核。

### 修复发现的测试夹具遗留

request_languages 的请求语言算法测试原意不启用自动识别，现显式 supported_locales=[]，保持原en断言；这是与N-02已复现默认语言中间件同因的夹具条件。Schema注释夹具改为读取现已内置框架的三语资源，不假设应用lang仍附带schema.json；目录与正式资源一致。邮件生成器并发时间戳目录发生一次冲突，加入原子计数器保证进程内隔离；未改生产生成逻辑。三者均实际复验通过，不靠更改预期值掩盖行为变化。

### 并行与磁盘

Request门面负责人在本批期间另获授权新增IP及完整门面。其新增源码、Provider、代理状态、上下文和路由调用借用均排除本批；共享源文件从HEAD只应用核定标识符迁移构造候选，三语新IP段落也保留为其工作区在途差异。工作区与索引不重置、不覆盖。目录 `candidate-overrides/` 是本地提交候选，不是回写他人工作。

HTTP代码生成曾触32GB停线，守卫安全终止且持锁清理全部项目编译profile，释放72,809,451,520字节；停止的一轮不计通过。源码/下载/数据/日志与原索引保留，空间恢复后完整重试。证据均在 `target/tmp/rust-naming/N-03/`：http-tests-combination.log、ip-intersection-final-sha.json、isolated-root-tests-final.log、isolated-fixture-regressions.log、mysql-acceptance.log、mysql-consumer-*.log、client-console-tests.log、scheduling-redis.log、isolated-clippy-final.log、docs-isolated-build.log、docs-browser.log。下一批沿用热缓存，退出后清理重复源码副本和临时容器；历史失败日志保留。

## N-04 数据与基础服务（验收通过）

本批按清单迁移1814组Rust拼写；包括Db/Url、数据库/Schema/Eloquent/工厂、分页/Resources、缓存/Redis、翻译/视图、日志/异常及相应trait、Facade、调用方、生成模板和三语文档。命名不改变SQL、事件名、序列化键、CLI或Blade表达式。

### 冲突与字符串边界

- Factory原生执行辅助方法和公开On入口分开；查询内部布尔/联接参数、关系pivot时间戳、集合字典查找、分页窗口读取、视图上下文渲染、Loader共享命名空间写入均按实际签名核定私有职责，不靠重载或同名递归。
- 台湾统一编号原VAT与既有vat(Option<bool>)冲突，前者采用taiwan_vat，保持数值返回与独立动态VAT/vat分发；后者保持原签名。Faker宏的89个表项与word显式携带原格式化器/唯一历史键，8个时区宏保持原history字符串。
- HashInfo是本批字段中直接Serialize的类型，algo_name显式映射algoName；其他本批字段非Serde模型，手动JSON/事件字符串不随字段拼写更改。新增4项消费者覆盖实际序列化、原动态格式化器、区域VAT与唯一历史。
- 缓存公开get_store拥有Arc返回，原Repository trait借用保持不变；借用敏感的内部调用显式走trait。事件has_listeners包装显式委派trait，避免遮蔽后递归。Stringable转HTTP响应按原拥有值消费into_string，不误选新增同名借用方法。
- Rust生成模板与模型字段识别同步；model:fields的syn方法匹配改为get_table。Passport/Fortify与模型验收脚本中明确的Rust片段同步，协议/能力名viewAny/forceDelete及Blade Lang::hasForLocale等既有表达式保留。

### 已实际通过的验收

- 隔离命名源码：基础服务41组369项通过，含集合、模型转换、分页、Resources、Faker、Hash、翻译、视图/Blade、日志/异常与字符串。数据库环境未提供的早退不计真实后端验证。
- 临时MySQL 8.4.13：8组58项真实查询、写入、Schema、迁移、模型、工厂、关系、分页通过；另真实Artisan迁移和Seeder各1项通过。
- 临时Redis 8配合MySQL：10批36项通过，覆盖memo、独立HTTP/Artisan命名连接与锁、数据库缓存、清锁、Redis值/锁、MySQL故障回退恢复、标签物理清理与过期索引、HTTP标签、驱动/事件及18语资源实际消费者。临时服务及匿名资源已清理，未使用用户数据卷。
- 三语站点构建/内容及根与/docs/桌面/移动浏览器12组通过；8个时区宏和VAT冲突裁决后的文档已重新构建。视图/日志与翻译数据库专项另12组浏览器通过；日志断言原先仍期待旧Windows产物调用，现按正式文档的artisan.bat入口修正，不修改命令行为或声称Windows实机已验。

### 失败、修复与待验边界

- Unix socket日志夹具在规定的项目内TMPDIR下超出SUN_LEN；缩短唯一夹具名后，真实日志HTTP/Artisan/Unix datagram完整复验通过，未改变产品日志实现。
- 大型关系夹具在迁移前N-03源码强制重编译后同样默认Debug线程栈溢出；本批使用RUST_MIN_STACK=16777216完成真实MySQL复验，应用运行时设置未变。首次跨快照缓存未重编译的baseline尝试不计证据；同缓存切换源码须检查实际编译路径/时间戳。
- 最终Clippy到达32GB磁盘停线，守卫已持锁清理全部项目编译profile并释放71,958,274,048字节，保留源码、下载、数据、日志、独立交付文件及索引。该中断不计通过；model:fields专项随后因二进制已清理未完成，不计通过，重建后补验。
- Request门面在另一已授权单元开发，其未提交新文件/功能不属于本单元；候选对共享文件只应用命名差异，保持原有两份许可删除暂存及用户WIP。其功能已独立提交225699817，本单元基于该HEAD重新组合共享源文件，只应用本批标识符；组合Request28项与命名4项、Faker116项通过，MySQL/Redis/Artisan再次全过。

证据：target/tmp/rust-naming/N-04/，其中accepted-groups.json、data-tests-*.log、mysql-framework.log、migrations-mysql.log、seeding-mysql.log、redis-consumer-*.log、docs-build.log、docs-browser.log保留实际结果。最终workspace/all-targets严格Clippy通过24.22s，新增周期/时区测试后复核0.55s；根全部兼容features严格Clippy通过41.63s。实际make:model/model:fields在临时MySQL生成、保留自定义get_table、幂等检查与生成源码严格编译通过；通知迁移生成器及生成源码严格编译通过。8个时区宏通过Faker116项及新增周期边界/时区消费覆盖，根具名日期/限定日期测试另2项通过。候选源码、Rust模板、脚本与测试均逐项匹配隔离快照SHA，Git核定范围排除并行功能和用户在途差异，本地提交另记实际哈希，不推送。
