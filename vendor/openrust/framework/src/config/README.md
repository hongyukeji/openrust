# 配置仓储原始读写

## Rust 代码配置

应用的 `Application::config()` 返回显式 `ConfigManifest`：根 `config/mod.rs` 可用 `.section("app", app::config)` 逐项登记 `config/*.rs` 中已编译的普通 Rust 函数。启动先读取进程优先的部署变量及所选 dotenv，再把同次只读 `ConfigContext` 交给这些函数；`env`、`env_or`、`bool_or` 可读取字符串和严格布尔值，错误不输出部署值。`nullable_bool_or(key, default)` 单独对应 Laravel env 保留字及 PHP truthiness：null/(null) 返回 None，false/(false)/0 为 false，off/no 等普通非空值为 true；现有 `bool_or` 继续严格解析。根与生成数据库配置用该方法读取 DB_FOREIGN_KEYS，命名空间环境层仍优先。代码区段在框架默认值之后、部署变量覆盖之前求值。默认应用启动不扫描 TOML；需要旧文件来源时须在清单中显式 `.with_toml()`，原有 `ConfigLoader::sources()` 仍保留文件配置能力。Rust 源码和 `mod` 声明改变后必须重新编译，部署值改变后重启生效。

`.typed("example.runtime_policy", factory)` 可保存不可序列化 Rust 类型或工厂闭包，由 `ConfigRepository::typed::<T>(key)` 返回共享 `Arc<T>`；调用与资源关闭仍由 Provider/Container 负责。raw `all()` 只返回可序列化视图，不能代表 typed 值；同名及会互相遮蔽的父子路径写入被拒绝。模块 Provider 可在 boot 时调用 `BootContext::code_config(&manifest)`，沿用应用同一次环境快照并原子发布配置；缓存命中时跳过模块代码求值，含 typed 清单则明确拒绝缓存加载和创建。可序列化配置继续由现有 `config:cache` 写入最终值快照；失败不写半成品。

### 顺序依赖与求值前覆盖

`ConfigContext::get::<T>(key)` 返回 `Result<Option<T>>`；`require::<T>(key)` 要求路径存在。它们只读取同一次求值中已经完成的区段及捕获的最终覆盖层，不执行后面的函数。清单应先声明被依赖的区段：应用清单先 `server`，再 `app`，最后 `auth`、`security`。修改所属应用的端口默认值即可改变未显式配置的 `app.url`；JWT 与签名链接缺省身份再读取该应用值，独立覆写继续优先。这些身份继承是应用增强，不是 Laravel 核心的 JWT 配置。

自读当前区段、读取尚未完成的相交声明、跨线程读取或在函数返回后使用捕获的 context 均明确失败；不从框架回退值掩盖依赖错误。双向依赖在首个前向读取处被拒绝，不自动分析或执行未来回调。错误只保留可信声明/已知路径与类别，不回显部署值或任意输入键；`env` 快照本身仍可克隆使用。

缺失与显式 null 不相同：`get::<serde_json::Value>` 对存在的 null 返回 `Some(Value::Null)`，`get::<Option<String>>` 返回 `Some(None)`，`require::<Option<String>>` 返回 `None`；按 `String` 读取 null 返回类型错误。typed/工厂路径不能通过这两个原始读取接口序列化读取。

```rust
use openrust::config::{ConfigLoader, ConfigManifest};
use serde_json::json;
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let manifest = ConfigManifest::new()
    .section("server", |_| Ok(json!({"port": 3000})))
    .section("app", |ctx| Ok(json!({
        "url": format!("http://localhost:{}", ctx.require::<u16>("server.port")?),
    })));
let sources = ConfigLoader::application_code_sources_with_overrides(
    std::env::current_dir()?, "DOC_", None, &manifest,
    [("server", json!({"port": 8123}))],
)?;
assert_eq!(sources.figment().extract_inner::<String>("app.url")?, "http://localhost:8123");
# Ok(())
# }
```

求值前覆盖是最终高于环境的显式全局原始层；文件、环境和该层只解析/捕获一次，依赖候选与最后发布复用相同合并规则。已有 `merge_ordered_entries` 和运行期 `ConfigRepository::set` 仍为求值后的快照变更，不自动重算已执行函数、重建客户端或更改既有 token/签名。非空前置覆盖遇配置缓存明确拒绝，须先清缓存；空覆盖与原 loader 等价。

模块代码从本次 boot 仓储快照读取并按现有递归覆盖候选求值，校验及 revision/CAS 成功后一次发布；它不预读后续模块。应用文件的浅合并与模块 boot 覆盖是两个既有来源边界，不能互换。显式 Rust 清单替代 PHP 文件执行属于必要编译适配；有序依赖读取不构成第二仓储或独立 DI。

根与生成项目的配置文件显式列出已实现的可编辑默认策略及可选分支；字段可编辑不代表对应 driver 已编译、Provider 已激活或外部连接已可用。根 `config/view.rs` 的 `ENABLED=false` 只控制是否默认声明该代码区段；显式来源含 view 仍可启用原 OptionalView Provider，生成应用默认声明 view。queue/realtime 等模板保留实际编译门控；生成 Job 后同步接入已启用的 queue 配置，不把普通 new 改成默认启用所有能力。

可选 `[server.trusted_hosts]` 配置 Laravel 主机信任列表：缺省不启用；启用后 `at` 缺省由 `app.url` 主机生成 PCRE 模式，`subdomains` 默认 `true`。`at = []` 加 `subdomains = false` 表示空模式列表，不限制主机。应用或模块 Provider 可经 `BootContext::trust_hosts` / `trust_hosts_with` 覆盖静态配置。模式与物理/代理 Host 的校验顺序见 [HTTP 说明](../http/README.md#可信主机)。

## 模块激活与运行根

根应用和生成应用的 [`config/modules.rs`](../../../../../config/modules.rs) 显式提供以下策略；`modules` 是同一配置仓储内的区段，不是另一份状态仓储：

| 配置键 | 默认值 | 代码配置读取的部署变量 |
| --- | --- | --- |
| `modules.activator` | `file` | `MODULES_ACTIVATOR` |
| `modules.activators.file.statuses-file` | `modules_statuses.json` | `MODULES_STATUSES_FILE` |

`Configuration.application_root` 是应用运行根锚点，缺省为当前工作目录；模块资源仍从该根下的 `modules/<模块名>/` 解析。`ModuleManifest::from_config(&root, &config.modules)` 将状态文件的相对路径锚定到同一应用根，绝对路径保持原值。例如可将状态单独放在 `state/modules.json`，而不移动模块源码、语言或视图。不会自动搬迁旧状态文件；`APP_CONFIG_DIR` 也不会替应用选择资源根。生成的 `modules::manifest(root, &config.modules)` 使用同一入口。

默认 `file` 在实际激活器构造时读取一次，后续 `snapshot/has_status` 使用该实例缓存，外部进程写文件不会刷新它。自身 set/delete 先更新缓存再写；写失败缓存不回滚，后续成功写保留尚未持久化的自身变更。`file-live` 是显式实时读取扩展，可用 `MODULES_ACTIVATOR=file-live` 选择，复用同一状态路径；它不冒充上游默认。`FileModuleActivator::new(path)` 返回 `Result`，坏文件在构造处失败，手工恢复可显式调用 `FileModuleActivator::reset_file(path)`。

`file` 通过同一模块清单选择；自定义激活器须先用 `register_activator` 登记，未知名称启动失败。源码清单中的 `ModuleManifest::enable/disable` 只调整本次构建，持久写使用 [模块管理服务与命令](../core/README.md#编译模块与持久启停)。普通只读自定义激活器的写操作明确拒绝，不回退到默认文件。

状态文件只认模块声明名的精确键和 JSON 布尔 `true`；缺文件、缺键、null 或其他值不启用，损坏 JSON 不当作空文件覆写。`Module::is_disabled()` 是启用判断的否定；`has_status(false)` 则按精确布尔规则比较，缺失/null 视为 false，其他非布尔值不等于 false。未知键和值会保留，但不会成为可执行模块。模块加载是每次 App 的快照；激活器实例状态与磁盘持久化结果分别观察，写入 Err 不代表内存未变；它们不更改当前已加载的 Provider、路由或迁移历史。配置缓存可固定激活器及状态路径，**不缓存状态 JSON 内容**；改选路径后如已启用配置缓存，须显式重建或清除缓存。

默认缓存视图保持独立，文件写在协作式 sidecar 锁内重读，保留其他键，经同目录临时文件同步后原子替换；忙锁报 `module_state_busy`，由调用方决定重试。事件和用户回调不在文件锁内执行；替换是提交点，提交后的事件失败不回滚状态。缓存中 null/missing 的 delete 不写，文件已消失但缓存仍有记录时 delete 重建剩余内容；reset 即使文件已消失也清缓存。严格 IO 错误仍显式返回 Err，这是保留增强，区别于上游 reset 可能忽略底层 delete=false。空表和连续数字键写成 PHP 列表形状；外来非空列表或等价 numeric 对象拒绝，不能覆盖未知坏文件。只有同一实例对同一解析目标已成功提交且字节精确匹配的列表，才允许自身后续读写恢复。路径按符号链接目标协作；不同 hardlink 路径的等价锁定/替换语义及 Windows 原生文件系统行为仍待验收，不作已支持保证。与并发 `shutdown` 之间也不保证原子提交屏障；应用应协调管理动作与停机，而不是把一次生命周期检查当作事务锁。

`paths.modules` 默认 modules（获准目录约定），`scan.enabled` 默认 false、`scan.paths` 默认 vendor/*/*；主路径与状态文件独立。运行发现读取 module.json，按声明调用已编译 Provider/文件初始化绑定，不能执行未编译 Rust。MODULES_PATH 覆盖主路径；metadata读取在包register，仓储get_path动态读当前配置，不热挂卸资源。

## 应用环境身份

`app.env` 保存精确环境名称。未声明时按固定 Laravel `LoadConfiguration` 回调缺省为 `production`；Rust `config/app.rs` 函数或显式选择的 `config/app.toml` 可声明环境，外部进程 `APP_ENV` 优先于 `.env` 和代码/文件配置。根 `.env.example` 和 CLI 生成的 `.env.example` 显式选用 `development`，因此开发模板不会受到缺省值变化影响。`APP_ENV=local`、`development`、`dev`、`test`、`testing` 及自定义 `staging` 均保持不同身份，大小写和显式空字符串也保留。配置缓存命中时使用缓存内的 `app.env`。

`App::environment()` 和 `AppState::environment()` 返回启动时名称；`environment_matches(["local", "stag*"])` 按 Laravel `Str::is` 的大小写敏感 `*` 模式检查。`is_local()` 只匹配 `local`，`is_production()` 只匹配 `production`，`running_unit_tests()` 只匹配 `testing`。Rust 将 PHP 的返回值重载与可变参数拆成独立读取和模式匹配方法；不从 Rust 测试进程状态猜测 `running_unit_tests`。已运行应用的配置仓储可以被显式写入，但这些启动身份查询使用启动快照，不重建旧服务。

`ConfigLoader::environment_override()` 只报告显式指定的环境；`None` 表示最终身份由配置文件或默认值决定。最终名称从 `load_framework().app.env` 或已启动应用的 `environment()` 读取，避免把缺省提示误当作文件环境。`Environment` 按精确名称比较与哈希，即使使用 `Custom("production")` 构造也与 `Production` 表示同一值。

现有邮件、短信的 `log` driver 和示例模块的 OIDC loopback/本地捕获限制继续使用独立的 `enforces_production_safety()` 策略；它对 `prod`、大小写变体和首尾空白仍执行旧生产保护。该安全策略不改变上述 Laravel 精确环境查询结果。

应用二进制的控制台入口接受 `--env local` 或 `--env=local`，并尝试读取实际存在的 `.env.local`。找不到时再按外部 `APP_ENV`（自定义命名空间对应 `<PREFIX>ENV`）尝试专属文件，最后读取普通 `.env`；不会先从普通 `.env` 取环境名再反推专属文件。外部变量始终优先，最终应用身份仍以配置的 `app.env` 为准，控制台参数本身不强制改写身份。缓存路径由外部环境早命中时跳过 dotenv；由所选 dotenv 提供路径的晚命中已读取该文件；两种命中均不再执行配置函数。`config:cache` 的 fresh 引导沿用本次控制台文件选择。普通 `lifecycle::bootstrap` 不读取进程参数；嵌入式控制台可用 `lifecycle::bootstrap_for_console` 或 `run_with_args` 显式传参。

## 配置缓存

应用二进制执行 `config:cache` 时，框架先清除本次选定的旧缓存，再用同一 `Application` 类型重新 bootstrap，完成所有 Provider 的 register、boot、最终校验，写入唯一配置仓储的最终有序值。`config:clear` 只清除该文件。默认文件是应用根的 `bootstrap/cache/config.json`；`APP_CONFIG_CACHE` 或应用自定命名空间的 `<PREFIX>CONFIG_CACHE` 可选绝对路径或相对应用根的路径。全局 `openrust` CLI 将这两个命令转发给应用二进制。

启动先用进程已有环境选择缓存；命中时不读 `.env` 和 TOML，也不会用新的环境配置覆盖缓存值。未命中时将 `.env` 解析到本次引导的独立环境快照，外部环境优先，再重选缓存路径，因此 `.env` 内的 `APP_CONFIG_CACHE` 可以生效。`config:cache` 只清除本次已选路径，fresh App 用同一路径并重新读取最新 `.env`；不改写进程环境。缓存损坏会安全报错，须显式清除。缓存可能含秘密，文件仅允许所有者读取；不要提交、展示或记录内容。缓存和清除只影响下一次启动，不热更新已运行 App。`config show` 仍对展示值脱敏。

`ConfigRepository` 是各 Provider 共享的同一配置仓储；`section::<T>` 保留类型化读取。

| 方法 | 返回值（均为 `Result<…, ConfigError>`） | 行为 |
| --- | --- | --- |
| `get(key)` | `Option<Value>` | 缺失为 `None`；显式 null 为 `Some(Value::Null)` |
| `get_or(key, default)` | `Value` | 仅缺失使用默认值 |
| `string` / `integer` / `float` / `boolean` | `String` / `i64` / `f64` / `bool` | 按原类型严格读取；整数不得超出 `i64`，浮点不接受整数 |
| `array` / `collection` | `Value` / `Collection<Value>` | 仅接受数组或有序对象，集合保留数字、稀疏及混合键 |
| `has(key)` | `bool` | null 仍算存在 |
| `get_many(&[key])` | `Vec<(String, Value)>` | 缺失为 null |
| `get_many_with_defaults(&[(key, default)])` | `Vec<(String, Value)>` | 每键默认值；null 保留 |
| `all()` | `Value` | 已发布配置的独立快照 |
| `set(key, value)` | `()` | 按点路径替换尾项并发布一个版本 |
| `set_many([(key, value), …])` | `()` | 按迭代顺序处理，整批原子发布一个版本；空批次不变 |

六类严格读取各有 `method(key)`、`method_or(key, Value)` 与 `method_or_else(key, || Value)` 三种入口。仅缺失时使用默认值或调用一次闭包；显式 null、错误类型和非法默认值按相同期望类型返回 `ConfigError::Invalid`，已有键不会检查未使用的默认值。错误只包含键路径、期望和实际类型或 `i64` 范围，不包含配置值。原始来源读取错误原样传播。`array` 保留当前 `serde_json::Value` 列表或对象投影；`collection` 在严格数组检查后包装已有值，不施加请求输入 bag 的 1024 字段/64 层预算。集合后续 `to_value`、`map`、`filter`、`values` 等既有操作仍受其预算约束；空数组原形及 PHP 非 JSON 值不能从当前投影恢复。

查询优先匹配完整字面键，再以点逐段遍历。`a.b` 字面键可覆盖同名嵌套路径的读取；`a.b.c` 不会在中途把 `b.c` 当完整键。数组支持 `items.0.name`，`00`、`+0` 不作为索引零；对象的 `01` 仍是有效命名键。空字符串查询空键，全部配置使用 `all()`。

批量操作只读取同一已发布版本，按键首次出现的顺序返回；重复键取最后一次结果。原始值尚未发布时完整解析当前来源，解析失败整体返回脱敏错误；已有原始值时读取该版本的独立快照。读取不改变版本、默认值发布、overlay 或 freeze；注册期只读当前来源，Provider 默认值仍在原有发布时点生效。

写入遵循固定 Laravel `Repository::set` / `Arr::set`：即使根上已有字面 `a.b`，`set("a.b", value)` 仍写入嵌套 `a → b`，而 `get("a.b")` 继续优先读完整字面键。空键和连续点号也是可写的数组键；规范数字键可形成列表，稀疏数字键和数字、字符串混合键保留为有序对象。覆盖已有键不移动其位置，尾项整体替换；中途遇到 scalar 或 null 时创建数组。`set_many` 按输入迭代顺序执行，失败时原始值、Figment 类型化投影与版本均保持旧状态。

核心原始写入在 Register、Boot、Frozen 阶段均可调用。Register/Boot 写入仍受后续启动 schema 校验约束；Frozen 后的写入允许任意可表达的 `serde_json::Value`，后续类型化读取不匹配时返回安全错误。注册、boot overlay 和 freeze 的阶段限制保持不变。`App::config_repository()`、`AppState::config_repository()` 与 Provider 返回同一共享句柄；`App::section<T>` 读取最新仓储。`App::config()`、`AppState::config()`、`App::figment()` 和已经解析的服务保留启动快照，不因内存写入自动重建。`clone()` 共享服务句柄；`all()` 返回独立值快照。

运行期写入值与后续启动阶段的原始键序保存在同一仓储状态内；Figment 投影和来源元数据随同一版本原子发布。默认应用引导使用 `ConfigManifest` 构成的不可变 `ConfigSources`：默认值、已编译 Rust 配置函数和环境层在同一次加载中形成原始值与 Figment 投影。显式选择旧 TOML 来源时，`ConfigLoader::sources()` 仍从默认值、文件和环境层构建来源。可信应用代码也可用 `ConfigSources::from_ordered_entries([(key, value), …])?` 按输入顺序提供配置，重复根键覆盖原位置；`figment()` 借用对应类型化来源。`ConfigRepository::new`、应用 Provider 与 `TestApp` 可接收这个输入。代码来源可用 `ConfigSources::merge_ordered_entries([(key, value), …])?` 显式追加优先级更高的 Global 覆盖：提供路径对各 profile 统一生效，其他字段保持各 profile 原值；同时保留原 profile、来源元数据、同次环境快照及 typed 值；typed/raw 路径冲突或已编译缓存来源明确拒绝。该方法不会恢复此前经 Figment 丢失的声明顺序。`ConfigRepository::new(Figment)` 仍可用，但 Figment 的字典已排序，该转换有损，无法恢复原始声明顺序。应用启动的 typed 配置、`App::figment()` 与已解析服务仍是启动快照，后续 raw 写入只更新共享仓储。

有序入口沿现有 `php_entries` / `php_value` 规则表示规范数字键：连续 `0..n` 是列表，反序、稀疏或数字与字符串混合为有序对象，`01` 是字符串键；空根输入是空列表。Figment 要求根为字典，因此纯数字根列表在类型化投影中暂时展开为数字键字典；`ConfigRepository::all()` 保留列表形态。嵌套数组保持数组。原始值与投影值经同一来源验证，构造失败不发布半成品。固定 PHP 8.5.0 的值、键序、列表/混合/稀疏形态见 [golden fixture](../../tests/fixtures/config_runtime_set.json)；PHP 对象、Closure 和资源不属于 `serde_json::Value` 可表达范围，仍是公开差异。

环境命名与配置路径遵循当前 `SERVER_PORT → server.port`、`CACHE_STORE → cache.default`。监听端口只读中性 `SERVER_PORT`；`PORT`、`APP_PORT` 及自定义应用命名空间中的端口旧键均不再控制监听。缓存缺省选择具名 `redis` store，根配置的 `[stores.redis]` 使用 Redis driver；`REDIS_HOST`、`REDIS_PORT`、`REDIS_PASSWORD`、`REDIS_DB` 只决定连接，缓存和队列共用 `REDIS_DB`。`REDIS_DEFAULT_STORE` 与已撤回的 `CACHE_DEFAULT_STORE` 不选择 store。`cache.default` 必须引用已登记的 store，业务仍可显式登记并选择名为 `default` 的 memory 或其他 driver。根 `.env.example` 的 `APP_URL` 使用 `${SERVER_PORT}` 插值，进程环境仍优先。

`ConfigLoader` 递归发现 `config/**/*.toml`，`config/app.toml` 内直接写 `name = ...`，`config/cache.toml` 内写 `[stores.redis]`；文件相对目录的点路径与文件名形成根配置键，例如应用自有的 `config/shop/feature.toml` 形成可按 `shop.feature.enabled` 读取的嵌套配置。完整映射键按自然序装载，文件内表和数组保持声明顺序。隐藏项忽略；目录符号链接不递归，文件符号链接按入口扩展名筛选、以目标真实 basename 命名。相同映射键由实际目录遍历中最后的文件覆盖，遍历次序依文件系统而定，不保证跨平台胜者。精确文件区段与框架及 Provider 注册的 base 默认值在顶层浅合并；`cache.stores` 等 Laravel `mergeableOptions` 指定选项只再合并一层，数字键按 PHP `array_merge` 追加并重新编号。仅有 `config/cache/custom.toml` 不把 `cache` 视为精确文件区段；若 `cache` 的 base 根仍待发布，最终根写入会覆盖子文件值，随后加载的环境层继续获胜。普通程序来源仍沿既有 typed 默认补齐。Rust TOML 无法表示 PHP 对象、Closure、资源；缺失隐式目录允许使用默认值，显式目录缺失或坏文件安全失败。文件层重建 Figment 投影时按根键保留前序文件来源元数据，环境层再附加自身来源；文件值与未覆盖的来源保持各自元数据，文件层的精确命名空间边界随同一来源进入仓储。DB/Redis 分项与环境层仍按原优先级在单次环境捕获中构造，`.env` 由启动流程先加载。模块 `ModuleConfigFiles` 的 overlay 是独立模块契约，不使用此应用文件命名规则；配置缓存、完整 PHP 值表示仍未验收。

此写入仅供可信应用代码使用，保存在当前进程内；它不写文件或数据库、不跨节点传播，也不刷新 DB、Redis、邮件等已建资源。管理员 Settings、授权、密钥和待重启策略见 历史决策 080，不由此原始写入接口代替。

成功返回的值与 `snapshot()` 一样可能包含秘密。调用方在日志、CLI 或 HTTP 输出前须使用适用脱敏机制。此读取簇的原始来源错误不显示来源提供的路径；类型化来源错误仅保留按 schema 层级核对的字段，来源提供的规范数字数组段显示为 `<index>`，动态 Map 名称显示为 `<entry>`。schema 自行校验数组时生成的索引仍可用于可信诊断；未知键和动态 Map 键不出现在错误路径中。这里的脱敏保证限于上述路径与原始来源错误，不能代替调用方对成功值及其他业务错误的审查。

业务代码可按固定白名单键读取，例如 `repository.get_or("app.name", serde_json::json!("应用"))?`；返回值是原始 JSON，调用方仍须判断类型。独立公开 API 调用与边界测试位于 [config-repository-reading consumer](../../../../../tests/consumers/config-repository-reading/README.md)。生成器建立空 `modules/`，开发者可在自己模块的 Provider 中取得 `BootContext::config_repository()` 并把其克隆放入业务服务；当前生成器不会复制唯一示例模块。

当前接口与固定上游映射见 历史决策 086。唯一示例的固定业务入口见[模块路由](../../../../../modules/example/routes/api.rs)与[服务](../../../../../modules/example/services/config_reading.rs)；生成应用可显式安装上述 consumer 的 Provider。读取子簇的既有验收不代替本轮写入、应用接线及真实请求验证，也不代表完整 Laravel Config 已验收。
