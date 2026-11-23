# 品牌中性化审查（2026-11-23）

本轮授权：先审查清单，再直接处理不影响契约的展示文案和私有命名；公开契约调整等待确认。不暂存、不提交、不推送。起点为 `cfe41904a1b81e0ebe691d3f271749b8e3064cb4`；并行任务随后独立提交 `21daa52c8eacc962c25618d20207c0f8d2963a76`，不是本批提交。

## 范围和依据

读取根 AGENTS、项目指引、规范及文档指南；扫描 Git 跟踪和未忽略的新文件，覆盖生产源码、应用骨架、配置、模板、脚本、测试和三语文档。忽略 Git 对象、依赖安装目录、临时及构建输出。匹配大小写和空格、下划线、连字符变体，并检查 `orust` 线索；不把 `OR`、`ORM` 等通用术语认作品牌缩写。

沿用现有固定上游基线及“方案调研与复用”结论，本批不设计新运行机制、不引入依赖。搜索只提供线索，不能据此替换所有字符串；保留的项目身份、准确坐标和兼容边界不以零命中为目标。

原始扫描及最终逐位置线索：`target/tmp/neutral-brand-audit/{scan-before,scan-after}.json`。最终扫描工具为 `.agents/project/tools/audit_brand_neutrality.py`，输出每行文件、位置、原文、复核类别及理由；自动分类不等于该行已通过公开契约迁移审计。

## 修改前清单与裁决

以下位置按修改前源码列出；同类项目包含对应三语与真实消费者。更精确的改写片段见工作区差异，未改写旧审计证据。

| 文件和位置 | 当前名称或表述 | 中性替代／处理 | 公开契约 | 兼容性与理由 |
| --- | --- | --- | --- | --- |
| framework/src/console/application.rs:199 | OpenRust command-line interface | 应用命令行接口 | 否，帮助展示文案 | 保持 Artisan 名称、命令、参数、格式结构和退出码；自然语言帮助标题按要求调整 |
| framework/src/contracts/log/mod.rs:10、22、37；log/facade.rs:78；log/logger.rs:237；pail/src/handler.rs:40 | OpenRust structured/asynchronous … adapter | 结构化日志／异步请求日志适配接口 | 否，lint 原因 | 不改允许项、方法名、日志通道或行为 |
| translation-database/src/database_loader.rs:1 | Matches OpenRust's translation API | 保留既定翻译接口命名 | 否，lint 原因 | 只解释保留旧接口拼写的原因 |
| framework/src/database/schema/grammars/grammar.rs:797 | Earlier OpenRust migrations … | 早期迁移创建独立索引的兼容说明 | 否，注释 | SQL 完全不改；保留兼容原因 |
| framework/src/http/request.rs:463 | 这是 OpenRust 扩展 | 直接说明当前语言的获取方法 | 否，注释 | localeSelection 和 currentLocale 接口均不改 |
| framework/src/support/date.rs:13 | IANA timezone used by OpenRust … | 日期辅助方法使用的 IANA 时区 | 否，注释 | 不改时区和默认值 |
| framework/src/support/stringable.rs:18–20 | 字符串示例 OpenRust | 普通示例 Example | 否，文档测试样例 | 类型、导入和序列化实现不改 |
| package-discovery/src/lib.rs:186 | invalid OpenRust discovery declaration | 包 {} 的发现声明无效 | 否，人可读错误原因 | 保持 io 错误类型、触发条件、元数据键及构建失败状态 |
| docs/{en,zh-Hans,zh-Hant}/digging-deeper/{sms,localization}.md | 品牌扩展、品牌专有功能等归属修辞 | 具名驱动、语言偏好、应用路径选择等职责说明 | 否，正文 | 不改代码块、配置键、默认值、链接和章节锚点；三语同步 |
| docs/{en,zh-Hans,zh-Hant}/the-basics/responses.md:137 | 统一业务响应格式由 OpenRust 提供 | 单独提供统一业务响应格式 | 否，正文 | 保留异常／健康端点不使用该格式的限制 |
| translation-database/README.md:68 | spatie/the framework-translation-loader | 恢复 spatie/laravel-translation-loader | 必要外部名称 | 上一次文字替换破坏了准确包坐标，须纠正；保留 2.8.3、MIT 及许可引用 |
| AGENTS.md:13、conventions.md 品牌表述及骨架条款 | 以 OpenRust 为主体；上游品牌段替换为 OpenRust | 统一引用按职责表达、身份与契约例外规则 | 项目规范 | 不执行公开 API 迁移；保留并行任务的原生 Rust 命名和清单入口规则 |

上述 framework、pail 等路径均位于 `vendor/openrust/`；没有为去品牌重命名包目录或文件。

## 必须保留项

| 位置／标识 | 原因与影响 |
| --- | --- |
| Cargo.toml／Cargo.lock、vendor/openrust/*、`openrust::`、`openrust_*` crate、仓库 URL、README／站点标题、版本与发行说明 | 正确构建、分发、导入、定位文档及辨认项目所需的身份。不是用中性名称替换真实包名。 |
| 根 .env.example 的 APP_NAME、DB_DATABASE 示例；config/app.rs、数据库连接与邮件／健康页的项目名回退；docker 的镜像、容器和项目默认名 | 本轮禁止改变默认值；运行配置和依赖数据可能使用这些值。用户在途 .env.example 不改。 |
| `[package.metadata.openrust.discovery]`、`openrust.blade.paths`、`openrust_providers()`、生成模板中的真实导入 | 包发现和编译契约；根应用与生成项目必须一致，不改元数据格式。模板品牌命中主要是这些真实导入。 |
| Console／HTTP／Database／Redis／Sanctum／Log／Dump 等 `OpenRust\\…` 事件及 Provider 名 | 监听器、持久化记录和跨包集成按原名称消费；需要别名、版本边界和迁移验证才能改名。 |
| `openrust:queue:restart`、`openrust:reverb:restart`、Pulse restart/check/throttle/ingest、`openrust.routing.url.defaults`、`openrust.queue.background`、`openrust.broadcast.event` | 缓存、进程控制、载荷和请求上下文契约；不可清掉旧控制键或让已有任务失联。 |
| `laravel_unique_job`、任务节流／SQS 指针、SQL 别名和隐藏键、发布标签、Horizon 前缀 | 已有互操作／数据／发布契约。先前逐位置裁决保留在 branding-2026-11-23.json，本轮不重写历史。 |
| `laravel-echo`、`laravel-precognition`、`@laravel/passkeys` 等准确外部包名 | 安装及集成需要真实名称，不能写成虚假的项目包名。 |
| LICENSE、NOTICE、THIRD_PARTY_NOTICES、SOURCES、依赖 README.openrust.md 等来源声明 | 法律、来源与本地补丁归属，不为减少命中删除、搬走或改写事实。工作区原有 Unicode／Zend 两份许可删除的暂存状态由他人保留，本批未操作，不代表本批已审定许可删除。 |
| tests 的 OPENRUST_TEST_*、OPENRUST_INIT_*，验收目录／容器标签、样例数据及相应断言 | 已有脚本和隔离命名契约；保持数据覆盖和并行隔离，不进行无收益的测试数据全文重写。 |
| 其他功能正文中的项目身份或 Rust／外部实现边界、准确 SDK User-Agent 示例 | 用于明确实现所属、版本和互操作边界；没有把所有主语强行换成“框架”。普通品牌扩展修辞本批已按上表清理。 |
| .agents/project/ 下的历史计划、来源和验收记录 | 反映当时事实，不以本次规范追溯改写。新裁决单独新增记录。 |

## 待确认的可观察契约

以下仅列迁移建议，未执行。未获确认不表示本轮非契约清理阻塞。

| 位置 | 现有值／候选职责表达 | 迁移影响与建议 |
| --- | --- | --- |
| foundation/http/{server,prerender}.rs:76／156、grpc.rs:165 | OpenRust … listening → HTTP／gRPC 服务监听提示 | tests/grpc.rs、middleware_lifecycle/shutdown.rs、包发现验收等按前缀解析地址。若确认更改，统一更新所有解析器并提供明确版本边界；保留地址、协议及启动时机，实测 HTTP/gRPC 启停。 |
| foundation/dev_commands.rs:144、231 | OpenRust Foundation → 框架内置命令 | source 是 Serialize 的字段，不能当纯注释；确认后迁移所有结构化输出消费者并验证 dev 命令。 |
| log/logger.rs、log/log_manager.rs | openrust 日志通道 → 应用通道名称 | 已有格式断言、日志检索和告警规则可能使用该字段。先确认名称、默认值与旧日志检索安排，不在此批改默认通道。 |
| Horizon 通知主题／Slack username、Socialite User-Agent | 当前品牌主题和产品标识 → 按服务职责命名 | 属于默认可观察值或外部请求标识，可能被过滤规则或服务端统计使用；须确认后更新对应消费者与真实发送验收。 |
| 事件名、缓存键、测试环境变量等 | 现有品牌命名 → 需要逐族确定名称 | 不建议为追求零命中迁移。若确需实施，须单列旧／新对应、双读或别名期限、数据迁移和回滚路径，取得确认后另批实施。 |

## 规则冲突处理

1. 原“以 OpenRust 为主体”及“上游品牌替换为 OpenRust”已被本次职责中性化要求替代；骨架条款改为引用统一规则。
2. 固定上游的机制、目录、默认行为和兼容契约继续验收；不因品牌清理设计新 Facade、命名空间或生命周期。
3. 并行确认的 Rust 原生命名迁移计划不属于本批，保留其差异；当前公开 camelCase 接口不在此批改名。
4. 通用 AGENTS 仅引用品牌表述章节，例外与验收要求集中维护在 conventions.md。

## 实际验收

- 隔离 HEAD 基线加本批修改：Console 7 项通过；补充包发现文案后再跑 7 项通过。没有用其他协作者未提交的构建脚本作为本批前置。
- `support::stringable` 定向文档测试 1 项通过；过滤的其他文档测试没有计入通过。
- 实际二进制：根 `--help` 展示“应用命令行接口”，命令帮助、JSON 命令清单正常；未知命令仍退出 1，两个帮助和生成操作退出 0。
- 实际 `make:class`、`make:job` 生成样本正常，Job 使用真实 `openrust::` 导入。所有生成模板与 HEAD 一致；探测文件在保留样本后从隔离源码删除。
- 隔离元数据 `version=0` 实际构建：仍报 `InvalidData`、退出 101，最终错误为“包 \"openrust\" 的发现声明无效”；恢复原 Cargo.toml 后 Console 回归再次通过。仅人可读原因改变。
- 最终三语文档 `npm --prefix docs run build` 通过，三语各 109 个导航章节和 111 个 Markdown 文件内容检查通过。已有构建 chunk 体积提示不属于本批新增行为。
- Playwright 实际访问三个改动章节 × 三语 × 根／子目录部署，共 18 项通过；检验最终正文、真实 API、语言目录说明和无页面异常。
- 本批 11 份 Rust 文件格式检查通过，源码 SHA256 与独立快照一致。包发现错误字符串缩短后的格式调整不改变词法语义。`git diff --check` 通过。
- 来源品牌守卫通过：117 处必要运行／分发标识；新恢复的真实包名使用独立补充裁决，不改旧 JSON。扩展扫描另外包含项目品牌、分隔符变体和文件名，不要求零命中。
- 原暂存区两份许可删除保持相同路径，HEAD 中对应对象与起点一致；本批未操作暂存、提交、推送。并行的 21daa52c8 提交归原负责人，本批保留其工作。

日志、生成样本、逐位置扫描和隔离源码均放在 `target/tmp/neutral-brand-audit/`。`preservation.json` 记录本批 Rust hash、原暂存删除和模板一致性。构建采用独占缓存与 build_guard，未触及 32 GB 停线；保留可复用增量缓存和验收日志，没有运行中的本批服务或容器。没有重跑数据库、队列、跨平台或完整上游验收，因为本批未调整这些机制；本批结果不代表这些模块重新验收完成。


## 本地提交核定

用户随后授权将已完成范围提交本地 Git，不推送。仅收本记录的非契约品牌清理、三语文档、扫描工具与规则；conventions.md 按分块仅纳入品牌章节和骨架条款中的品牌句，不纳入其他协作者的 Rust 原生命名计划。已列待确认的可观察契约仍保留，不把提交授权解释为其更名许可。根环境文件、其他交接和计划、原暂存的两份许可删除均排除。
