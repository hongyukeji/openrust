# Rust 原生命名迁移验收

2026-11-23 用户授权：按 N-01 至 N-06 全仓实施、验收后分批本地提交，不推送。起点为 `8ae49738324ae7fcbae0196296d3bb3e137e7fe1`。保留用户环境配置、其他模型验收记录及原暂存的 Unicode／Zend 许可删除，不纳入命名提交。

| 单元 | 状态 | 实际范围及证据 |
| --- | --- | --- |
| N-01 | 验收通过 | 语法树盘点、workspace 清单、调用位置、字符串边界及碰撞检查；无运行代码改动 |
| N-02 | 验收通过 | 177 个基础方法族及跨 crate 消费者；20 项应用、59 项框架、独立生成测试、实际 HTTP／CLI、workspace 与可选 feature 严格 Clippy |
| N-03 | 待实施 | HTTP、路由、认证、中间件、Session、命令、安装与生成源码 |
| N-04 | 待实施 | 数据库、集合、缓存、Redis、翻译、视图、日志和异常 |
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
