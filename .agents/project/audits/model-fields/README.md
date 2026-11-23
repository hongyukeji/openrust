# 模型字段引用验收

日期：2026-11-23。设计边界及验收见本文，最新模型表名与列顺序修订见末节。前置代码按用户授权提交为 `f80096ea6`；之后的布局修订未授权再次暂存、提交、推送或发布。前面各版记录保留为历史，不能代替最新源码验收。

## 实现范围

- `database/eloquent/fields.rs` 提供声明宏，`eloquent/mod.rs` 重导出；一处可选声明生成普通模型关联字符串常量，无运行时注册、无新依赖。
- 应用 User 复用常量声明批量赋值/日期转换键与邮箱验证的筛选、NULL检查和写入；常量不改变这些策略的职责。
- 正式 `make:model` 模板提供可删除的默认主键声明，真实 Artisan 消费者验证其生成物；未增加平行 ORM、过程宏或数据库驱动生成服务。
- 三语 Eloquent 入门说明查询、映射、迁移、别名和真实编译期边界。
- 验收顺带修复 `test_response_metadata.rs` 两处对未跟踪 PHP 文件的依赖，改用已有 `json-schema.json` 作为下载夹具；测试仍验证真实文件响应与原始字节，不新增PHP文件。

## 源码与资源隔离

本轮最初在任务开始时的工作区组合快照中执行三库消费者、应用构建、生成器/rustc消费者及文档验收。全仓fmt与严格Clippy实际失败：其他在途文件存在格式差异、pagination/views.rs未使用ViewError导入、log/mod.rs未使用私有write方法。保留原始日志，不修改这些并行功能来让检查变绿。

随后固定 `6ff17cf0d`（任务开始时HEAD）加本任务改动建立独立组合。工作区其他改动及后续协作者提交不计入这个组合的通过结论；应用User基于该提交加入相同字段声明/引用，保留工作区另一任务的错误传播改动。基线记录 `target/tmp/model-fields-20261123/baseline.json`。独立组合还包含上述使测试不依赖未跟踪文件的夹具修复。

所有源码、调研、日志、数据库夹具子进程临时文件与独占Cargo缓存均在 `target/tmp/`；统一经build_guard，Rust1.94.0，保留增量与正常并发。本任务未执行清理、修改工具链、开发数据或数据卷。其他工作流运行期间磁盘空间由守卫监控；本轮初始约35.06GB可用，随后观察到其他工作流清理后约134.05GB，此清理不记为本任务执行。

## 已发现并修正的验收问题

- 初版测试错误地给Query::insert传入单条Attributes而非记录切片；已按真实签名修正。原失败见test-sqlite.log。
- SQLite双引号兼容规则可能将未限定未知列解析为字面量；缺列负例改用显式表前缀，三库都通过实际执行错误验证。原失败见test-sqlite-final.log。不因此改变既有SQLite语法器，也不宣称常量可检查数据库结构。
- 应用静态ModelMetadata内部不能使用外层Self；已改用User::EMAIL/NAME。固定组合全workspace检查发现并修复，原失败见clippy-baseline.log。
- 固定提交旧测试仍引用Git未跟踪的PHP下载夹具；改为既有JSON文件后重新运行严格检查及该测试。原失败见clippy-baseline-final.log。

## 命令与证据

全部路径以下相对于项目根。Cargo命令前缀为：

```sh
python3 -B tests/build_guard.py --workflow model-fields-20261123 --
```

独立源码的manifest为 `target/tmp/model-fields-20261123/source/Cargo.toml`。初始组合保留在同批 `integration-source/`，独占缓存为 `cargo-target/`，依次复用，未同时用两份源码构建。

```sh
cargo +1.94.0 fmt --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml --all --check
cargo +1.94.0 clippy --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo +1.94.0 test --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml -p openrust-framework --test model_fields --test query_bindings --test eloquent_key_values --test test_response_metadata --locked
cargo +1.94.0 test --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml -p openrust-framework --doc model_fields --locked
python3 -B tests/postgres_acceptance.py -- env OPENRUST_FIELD_DATABASE=postgres cargo +1.94.0 test --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml -p openrust-framework --test model_fields --locked
python3 -B tests/mysql_acceptance.py -- env OPENRUST_FIELD_DATABASE=mysql cargo +1.94.0 test --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml -p openrust-framework --test model_fields --locked
cargo +1.94.0 test --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml --test password_reset_http --locked -- --skip mysql_verification_requires_current_email_and_emits_once --skip http_authentication_links_from_real_smtp
cargo +1.94.0 build --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml --bin main --locked
python3 -B tests/model_fields_acceptance.py --source target/tmp/model-fields-20261123/source --work target/tmp/model-fields-20261123/consumer-final
```

框架消费者对比字符串/常量的SQL、绑定与查询结果；实际执行列映射、保留字、恶意值参数绑定、NULL、排序、投影、别名自联表、模型水合、更新、HasMany及动态属性与原始表达式。生成器使用真实Artisan，生成源码通过真实rustc元数据编译与查询接口类型检查；缺失和重命名后旧常量各以E0599拒绝，不以关键词出现替代失败状态。两项rustdoc另覆盖正确声明和不存在常量的编译失败。

文档在初始组合中通过 `npm ci`（缓存位于本批）、`npm run build`（包含命令入口规则与三语内容检查）、`check_browser.mjs` 和 `check_scope.mjs`，覆盖三语、桌面/移动、`/`及`/docs/`。Node24.20.0、VitePress2.0.0-alpha.21、本机Chrome及既有Playwright；路径通过DOCS_BROWSER_PATH/DOCS_PLAYWRIGHT_MODULE显式指定，日志docs-{install,build,browser,scope}.log。文档增强段落与最终工作区内容核对后沿用，不将文档构建算作全部Rust API导出。

## 支持边界与未验范围

保证的是关联常量存在性与可见性；没有模型归属、查询值类型或实际数据库结构的编译期保证。常量不自动限定表名、不自动导出数据库字段集合、不推断fillable/casts，不会自动察觉外部数据库重命名。SQL类型/值转换/自定义cast/NULL继续使用既有机制。本轮未引入值类型增强。

本批不代表所有未提交组合、完整workspace测试、all-features、性能、最终Windows/Docker/release或真实生产部署通过。普通构建不连接数据库；仅显式验收夹具创建独立实例。无本范围供应商凭据或生产数据依赖。

## 第一版历史结果

- 固定基线加本任务改动：fmt通过；全workspace/all-targets、锁定依赖、`-D warnings`严格Clippy通过。最后交付日志fmt-delivery.log/clippy-delivery.log；第三方既有vendor依赖仍可能输出Cargo cap-lints范围内警告，不宣称依赖日志零警告。
- 最终定向执行20项均通过：框架14项（新字段消费者1、Query绑定2、Eloquent主键/单值4、下载/响应元数据7）、rustdoc正反2项、PostgreSQL18.7和MySQL8.4.13各1项、应用实际HTTP/SQLite邮箱验证2项。没有把被筛除的MySQL认证或真实SMTP用例计为通过；三库共享字段行为由独立field消费者覆盖。
- 正式模型生成、模型字段查询消费者元数据编译、未知常量E0599、重命名后旧常量E0599均通过；最后为consumer-delivery/，包括完整rustc输出与Artisan输出。普通生成/编译命令未要求数据库服务。
- 应用追加NAME声明并在fillable复用后，重新通过fmt、严格Clippy、两项应用回归、应用构建及生成器三个正反编译阶段，详见delivery.log（各阶段退出码0）。框架与查询消费者源码未变，不重复扩大验收。
- 三语内容/站点构建、桌面/移动及根路径与/docs/浏览器、全导航均通过；对应docs-*.log。没有生成全feature Rustdoc API导出。
- 六处独立交付源码/测试/模板与最终固定组合逐字节一致；三语本增强段落一致。应用User的字段声明/引用已核对，固定组合保留提交中的错误映射，工作区另一个任务的错误传播调整保留，因此不将整个User文件错误映射视为本任务新验收。

本任务无剩余字段引用实现阻塞。当前全体并行WIP组合的fmt/严格Clippy失败仍保留在最初记录中，由对应任务处理，不将固定组合通过冒充所有WIP通过。未暂存、提交、推送或发布；未验范围继续以上节为准。


## 自动生成修订（2026-11-23）

用户要求不再自己逐字段定义。方案：数据库当前结构 → 开发阶段命令 → 保存在模型源码的关联常量 → 普通离线构建。基线仍为6ff17cf0d加本主题全部改动，未将其他并行WIP加入通过范围；保留其他Agent在User及console/mod中的变更。

实现：
- `foundation/console/model_fields_command.rs`：`model:fields` 读取既有Schema::getColumns，管理生成区域、拒绝标识冲突及非法/损坏边界，复用源码锁与原子替换。
- `model_make_command.rs`：`make:model --fields`、`--table`、`--database`；新模型元数据的表名使用Rust字符串转义，不允许表名注入源码。普通模板不再要求手写ID示例。
- 默认User从隔离SQLite中实际执行应用迁移，再通过实际model:fields生成8个字段；没有读取开发数据库。其他认证错误传播WIP保留，范围内User字段改动通过应用HTTP/SQLite认证回归。
- `tests/model_fields_acceptance.py`：真实Artisan、实际rustc和生成后的查询消费者，三库沿用现有独立实例夹具。测试数据库操作仅限本夹具表。
- 三语入门正文改用自动生成命令；完整说明自定义表/连接必须每次显式传入、不反射metadata、不自动感知外部结构、离线构建及重建要求、特殊名称拒绝及宏高级映射。

新增依赖为0，锁文件未因本任务变更；不复制第三方实现。已实际下载固定SeaORM CLI1.1.19、Diesel CLI2.3.3、Laravel IDE Helper3.6.0、固定Laravel ShowModelCommand及前三者MIT许可，借鉴结构反向生成机制。研究材料在本主题research/，详细来源和必要差异见方案自动生成修订节。

验收路径 `target/tmp/model-fields-20261123/`；本轮复用同一隔离source/，此前Cargo缓存已由其他工作流清理为空，本轮独占缓存改为 `target/tmp/model-fields-auto/cargo-target/`，没有复制完整缓存。守卫固定Rust1.94.0、项目内TMPDIR、正常并发及增量。旧auto-build.log受两次先后构建写同一文件影响，不用作最终证据；最终以auto-build-final、auto-validation及auto-final系列退出码为准。

第一阶段真实运行：
- `auto-validation.log`：fmt、workspace/all-targets严格Clippy、14项框架定向、2项宏rustdoc正反、2项应用HTTP/SQLite回归均0；认证中筛除MySQL认证及真实SMTP2项，未计为通过。
- `auto-sqlite.log`、`auto-postgres.log`、`auto-mysql.log`：三库Artisan自动生成、生成源码编译、真实查询、列增删/重命名后旧引用E0599、新字段成功、幂等更新、保留业务代码、嵌套模型/表/连接覆盖、缺表/连接/冲突/损坏标记失败保护通过。PostgreSQL18.7、MySQL8.4.13；对应隔离容器已回收。
- `auto-migrate.log` / `auto-user-fields.log`：实际应用迁移与User生成命令退出0。
- `auto-docs-{install,build,browser,scope}.log`：三语内容、构建、桌面/移动及根路径与/docs/全导航通过；每语言19个支持范围页，不将这些页面检查当作API功能验收。

补充检查发现自定义表名需要转义，已修复并增加带引号表名的真实生成/编译、非ASCII字段失败、重复及反向标记拒绝、不覆盖既有模型、--database须配合--fields的验证。最终三库复验全部通过：`auto-final.log` 的fmt、clippy、build、sqlite、postgres、mysql均为0，所有新增保护反例实际按预期失败且文件保持原样。日志位于auto-final-{sqlite,postgres,mysql}/各阶段及对应汇总.log；三个隔离消费者均执行真实SQL与生成模型查询。

可复现入口（所有命令前统一 `python3 -B tests/build_guard.py --workflow model-fields-auto --`）：

```sh
cargo +1.94.0 fmt --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml --all --check
cargo +1.94.0 clippy --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo +1.94.0 build --manifest-path target/tmp/model-fields-20261123/source/Cargo.toml --bin main --locked
python3 -B tests/model_fields_acceptance.py --source target/tmp/model-fields-20261123/source --work target/tmp/model-fields-20261123/auto-final-sqlite
python3 -B tests/postgres_acceptance.py -- env OPENRUST_FIELD_DATABASE=postgres python3 -B tests/model_fields_acceptance.py --source target/tmp/model-fields-20261123/source --work target/tmp/model-fields-20261123/auto-final-postgres
python3 -B tests/mysql_acceptance.py -- env OPENRUST_FIELD_DATABASE=mysql python3 -B tests/model_fields_acceptance.py --source target/tmp/model-fields-20261123/source --work target/tmp/model-fields-20261123/auto-final-mysql
```

不提供模型归属、值类型或离线真实数据库结构的编译期保证。未验完整workspace测试、all-features、Windows、生产部署、优化release及性能；当前全部并行WIP组合仍需所属任务独立验收。本任务不暂存、提交、推送或发布。


最终交付核对：两个命令实现及模板、字段宏、框架字段测试、Python消费者与最终隔离源码一致；console/mod仅新增模块/注册两处，与其他任务的错误传播改动分开核对。应用User保留其他任务错误映射改动，仅生成块与字段消费者变更归本主题；生成块与实际迁移库输出一致。三语本节与已构建/浏览器验收的快照一致。未触发本轮磁盘停线清理，最终可用空间以实时df为准；不新增PHP文件、不操作开发数据或卷、不执行Git写操作。


## 模型配置复用、批量与只读检查（2026-11-23）

本节为最新结论，覆盖前文每次手动指定自定义表/连接及“无静态类型登记”的旧边界。方案、实现、范围内验收均完成；未进行暂存、提交、推送或发布。

### 采用方案与调研

复用 Schema Builder::getColumns、生成区域、源码锁/原子替换及框架既有 inventory =0.3.25。自动生成区域携带模型类型入口，module_path! 与模型标识确定约定路径，调用时由空 ModelState 构造实例读取 getTable / resolveConnectionName。静态表仅保存字符串与函数入口，不保存应用、连接或可变模型，不增加管理器、过程宏或平行ORM。

实际HTTPS下载并阅读官方固定源码，材料位于 target/tmp/model-fields-sync-20261123/research；web检索工具返回空，改用官方GitHub raw及源码树API：

- 固定Laravel Framework v13.40.0 / 1449bf241017087fbd38c118ecaabef3f6797fdc 的 ShowModelCommand；IDE Helper v3.6.0 ModelsCommand 从真实模型 getTable/getConnection/getColumns读取结构并批量处理类型。采用模型配置复用，未仿造PHP反射；固定源码树未发现单独ShowModel命令测试，不宣称执行PHPUnit。
- SeaORM1.1.19 CLI及Diesel2.3.3 print_schema继续支持开发期读取结构、生成离线可构建源码的路线；Diesel同版本main.rs的LOCKED_SCHEMA比较生成结果，不同则SchemaWouldChange。借鉴检查语义，不照搬其创建目录副作用。
- inventory0.3.25已为认证类型入口使用；官方Cargo.toml声明最低Rust1.68、MIT OR Apache-2.0，并实际核对MIT许可。无新依赖、无第三方代码复制，Rust1.94.0实际通过。旧版v0.x没有当前字段自动同步实现可提取。

### 公开入口与支持边界

- `model:fields User`：已编译且带新版生成区域的约定模型复用配置；显式单模型参数优先。未登记旧模型首次按默认表/连接或显式参数同步，随后重新构建。修改表、连接或模块配置后先重建命令，避免使用旧编译配置；不解析Rust源码表达式。
- `model:fields User Admin/User`、`model:fields --all`：使用各模型配置。all仅约定app/models中已编译且启用自动生成的类型；未引用模块/未启用feature/任意包模型不保证发现，不能把该检查当作全数据库覆盖。空集合和歧义路径非零失败。多模型共用覆盖参数拒绝。
- `model:fields --all --check`：相同返回0，过期返回1；配置/数据库/标记错误非零并说明错误。不创建源码锁/源码临时文件，不改模型内容或mtime；应用启动日志/缓存沿用原行为。
- 批量全部预检成功才开始写入，每文件仍为并发内容检查及原子替换。跨文件不提供文件系统事务；写入期间IO失败可能部分完成，修复后幂等重跑。相同结果不重写。
- `make:model --fields --database=...` 同时保存模型connection元数据，修复此前只选择读取连接的差异。已发布自定义模板须包含{{connection}}占位符；缺失时明确失败且不创建模型，普通无连接选项的旧模板继续可用。
- 数据库列结构仍是字段集合唯一来源，自动类型入口不是另一份列清单。常量仍为未限定&str，只提供引用存在性检查；无模型归属、值类型或外部数据库变更的编译期保证。

### 验收组合与命令

本次在任务开始的HEAD归档叠加当时全部WIP建立隔离源码 `target/tmp/model-fields-sync-20261123/source/`，不再沿用旧6ff17组合。复用本工作流独占cargo-target、增量及正常并发，临时目录由守卫传入；Rust1.94.0 / Node24.20.0。最后核对本主题技术交付及三语正文与该组合逐字节一致；不把未覆盖的其他WIP行为计为通过。

以下命令都由 `python3 -B tests/build_guard.py --workflow model-fields-sync-20261123 --` 包裹；manifest指向上述source/Cargo.toml：

```sh
cargo +1.94.0 fmt --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml --all --check
cargo +1.94.0 clippy --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml --workspace --all-targets --locked -- -D warnings
cargo +1.94.0 build --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml --bin main --locked
cargo +1.94.0 test --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml -p openrust-framework --test query_bindings --test eloquent_key_values --test test_response_metadata --test console_observation --locked
cargo +1.94.0 test --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml -p openrust-framework --doc model_fields --locked
cargo +1.94.0 test --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml --test password_reset_http --locked -- --skip mysql_verification_requires_current_email_and_emits_once --skip http_authentication_links_from_real_smtp
cargo +1.94.0 test --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml --test console --test application_skeleton --locked
cargo +1.94.0 test --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml -p openrust-framework --test model_fields --locked
python3 -B tests/model_fields_acceptance.py --source target/tmp/model-fields-sync-20261123/source --work target/tmp/model-fields-sync-20261123/final-sqlite
```

最后两条另由 tests/postgres_acceptance.py / tests/mysql_acceptance.py 隔离实例运行，设置OPENRUST_FIELD_DATABASE及各自连接URL，work分别为final-postgres / final-mysql。最终阶段编排见同批final.py及final.log，各阶段全部退出0；每阶段原始日志final-*.log。三库分别为SQLite、PostgreSQL18.7、MySQL8.4.13，容器以独占标签回收，不访问开发数据。

实际30项Rust测试通过：框架14（查询/键值/响应/console事件）、宏rustdoc正反2、HTTP/SQLite认证2、应用console与骨架9、三库字段查询各1。真实命令消费者另覆盖自动生成/查询/编译正反例、列增删重命名、保留字/NULL/别名联表及动态模型；不将被筛除的MySQL认证或SMTP用例计为通过。

本次新增消费者明确验证：无效默认连接下仍使用各模型连接；自定义表无需重复传参；模型metadata变化重编译后换表；多模型与all；空发现集合拒绝；批量预检失败全部保留；过期返回1与两模型诊断；相同结果不改mtime；全新无源码锁目录执行check也不创建目录；显式覆盖失败保护；自定义模板保存连接及缺占位符失败保护。均经真实rustc生成独立消费者，再调用正式Application::handleCommand，非关键词静态证明。

三语文档在同一隔离快照中完成npm ci、npm run build（含内容及命令入口规则）、check_browser.mjs与check_scope.mjs。复用既有Playwright和本机Chrome，DOCS_PLAYWRIGHT_MODULE / DOCS_BROWSER_PATH显式传入；三语×桌面/手机×根路径和/docs/的12组合及6组全导航通过，日志docs-{install,build,browser,scope}.log。VitePress块大小及第三方vendor既有警告保留；严格Clippy通过不等于依赖日志无警告。

初次MySQL在自定义模板的“无半成品”断言失败：上一PG轮次成功生成的StubProbe文件残留于复用源码，并非失败命令生成。修正测试夹具保存/删除/恢复自身模型后，最终三库全量消费者重跑通过。原失败mysql-initial.log保留，不将原失败覆盖或计为通过。

本轮末可用约64,061,168KiB，高于31,250,000KiB停线，未触发清理。本修订无剩余实现阻塞；完整workspace测试、all-features、Windows、release、生产、性能及未覆盖WIP行为未验。不改Git授权，也不以本轮验证宣称整体最优或全仓完成。


## 数据库列描述生成文档注释（2026-11-23）

方案定稿、实现完成、本修订范围内验收通过。数据库列说明生成到字段常量的 `#[doc = "…"]`，等价于 Rust 文档注释；`dead_code` 的 reason 继续表达忽略警告原因，在生成 impl 上只保留一次。宏增加 impl 属性透传，兼容已有手写和旧版自动生成区域。

复用既有 getColumns / Column.comment，不新增依赖、ORM、配置清单或字段名说明词典。MySQL/PostgreSQL读取数据库列说明；缺失、空白及SQLite无原生列说明时不生成描述。只对说明是否空白做判断，非空内容按原值用Rust转义字符串输出，多行、回车、引号、反斜杠及伪造代码不会破坏源码；描述不由生成器翻译。说明变化也纳入现有 --check 的过期判断，删除说明后刷新会移除旧文档。

### 调研依据

web检索返回空结果，随后实际HTTPS读取以下官方固定源码/资料，保存在 `target/tmp/model-fields-sync-20261123/research/`，无第三方代码复制或新增依赖：

- [Rust1.94文档属性](https://doc.rust-lang.org/1.94.0/rustdoc/write-documentation/the-doc-attribute.html)及[注释语法](https://doc.rust-lang.org/1.94.0/reference/comments.html)：使用文档属性与转义字面量，避免数据库内容成为Rust语法。
- [IDE Helper v3.6.0 ModelsCommand](https://github.com/barryvdh/laravel-ide-helper/blob/v3.6.0/src/Console/ModelsCommand.php)：读取真实列的 comment 传入模型属性文档。采用数据库描述作为唯一说明来源的机制，不引入PHP反射或运行时依赖。
- [固定Laravel MySqlGrammar](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Schema/Grammars/MySqlGrammar.php)：compileColumns读取column_comment，与当前Schema已有接口一致。
- [SeaORM1.1.19 Column](https://github.com/SeaQL/sea-orm/blob/1.1.19/sea-orm-codegen/src/entity/column.rs)及[Writer](https://github.com/SeaQL/sea-orm/blob/1.1.19/sea-orm-codegen/src/entity/writer.rs)：核对结构生成及文档输出路径；此版本这两处未提供可直接复用的列说明生成逻辑，不将其声明为已支持本需求。无需引入SeaORM或代码生成依赖。现有Rust1.94.0兼容性以本次实际编译验证。

### 本次验收

继续复用隔离源码 `target/tmp/model-fields-sync-20261123/source/` 与独占cargo-target，无在途Cargo时同步本次修改，子进程TMPDIR及RUSTUP_TOOLCHAIN由build_guard设定；没有访问开发数据库。以下命令均加前缀 `python3 -B tests/build_guard.py --workflow model-fields-sync-20261123 --`：

```sh
python3 -B target/tmp/model-fields-sync-20261123/comments.py
python3 -B target/tmp/model-fields-sync-20261123/comments-docs.py
python3 -B tests/mysql_acceptance.py -- python3 -B target/tmp/model-fields-sync-20261123/user-comments.py
cargo +1.94.0 fmt --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml --all --check
cargo +1.94.0 clippy --manifest-path target/tmp/model-fields-sync-20261123/source/Cargo.toml --workspace --all-targets --locked -- -D warnings
```

comments.py实际依次执行fmt、workspace/all-targets严格Clippy、main构建、2项model_fields宏rustdoc正反例，再运行SQLite/PostgreSQL18.7/MySQL8.4.13的真实生成/刷新/查询/编译消费者；原有结构变更、业务代码保留、批量、模型配置与只读检查回归全部通过。新增验证包括：生成impl只有一个allow；缺说明不猜测；中文说明真正出现在rustdoc字段页面；仅改变说明使check返回1且内容/mtime不变；刷新后check通过且重复刷新不改mtime；复杂换行/回车/引号/反斜杠/伪造compile_error说明的生成模型在-D warnings下编译通过；空白/删除说明移除旧文档。三库消费者全部退出0，日志comments-{sqlite,postgres,mysql}.log及各对应目录。

user-comments.py在独立MySQL8.4.13实例以APP_LOCALE=zh-Hans执行真实应用迁移，再通过实际Artisan同步User，生成“邮箱”“密码哈希”等8个数据库描述；仅将生成区域更新回工作区，保留模型业务及其他协作者改动。随后最终fmt与严格Clippy再次通过；7个技术/正文文件与验收源码逐字节相同。日志comments-user*.log、comments-final-{fmt,clippy}.log。前面首次fmt检查发现一处排版差异，修正后各次最终检查退出0。

三语正文同步说明来源、驱动边界及刷新要求。文档build（含内容与命令入口检查）、12组真实浏览器页面及6组导航全部通过，见comments-docs*.log。第三方既有warning与VitePress体积提示不计为框架错误，未声称全部日志零warning。

最终可用61,046,076KiB，高于31,250,000KiB停线，继续保留可复用缓存。本次未新增SQL行为，未重跑完整查询矩阵Rust测试、完整workspace测试、all-features、Windows、release、生产或性能；其他WIP未被本修订全部验收。没有本修订剩余实现阻塞；未暂存、提交、推送或发布。

## 已完成计划归档（2026-11-23）

以数据库结构生成关联常量和列说明，复用模型表／连接配置与现有命令；只读检查、批量预检及逐文件原子写入边界不变。不提供字段归属和值类型的完整编译期保证。

原计划 `model-field-references.md` 已删除；历史设计、调研及执行前快照可从提交 `f80096ea6` 的 `.agents/project/plans/model-field-references.md` 查阅。本文保留实际验收与未验边界，不因归档补计测试。


## 模型顶部生成布局（2026-11-23）

### 方案定稿与实现

按用户最终裁决，自动字段仍保留在 `app/models/user.rs`，放在连续文件头导入之后、模型类型之前。使用与项目配置一致的分区块注释及 `openrust::model_fields! { User { NAME = "name"; } }`；不再输出 begin/end 机器标记、完整 eloquent 宏路径、`@model`、`impl`、`pub const` 或 allow 属性。宏内部生成已有的关联字符串常量与静态模型类型入口，集中处理未引用字段；普通说明使用 `///`，必须转义控制字符时保留安全 doc 属性。字段集合及描述继续以数据库结构为来源，不从 fillable/casts 推测。

不拆伴随文件、不引入 Repository／Service 层或平行 ORM。User 业务代码以及其他协作者的密码／令牌字段常量引用保留。旧标记生成区首次刷新时迁移到顶部；损坏、重复或有歧义的区域拒绝覆盖。解析使用 Rust AST 与真实字节 span，保留模型 doc/derive、模块 inner 属性、BOM/shebang 和业务原文；不会按大括号字符串猜测源码边界。表／连接仍从已编译模型读取；语法解析只负责安全编辑，并不反射尚未编译的业务配置。

### 调研、复用与差异

- 固定 [Laravel 应用配置](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/config/app.php)与[框架模型模板](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/stubs/model.stub)：采用分区说明的阅读风格，模型及现有 Eloquent 装配不变。PHP 上游模型没有本扩展的 Rust 字段常量宏，不声称这是 Laravel 原生 API。
- [IDE Helper v3.6.0 ModelsCommand](https://github.com/barryvdh/laravel-ide-helper/blob/v3.6.0/src/Console/ModelsCommand.php)：继续采用数据库列注释供 IDE 查看，不引入 PHP 运行依赖。SeaORM/Diesel 的结构生成研究沿用前文；不为输出美化引入另一套实体、列枚举或查询系统。
- [syn 2.0.119](https://github.com/dtolnay/syn/blob/2.0.119/Cargo.toml)与[proc-macro2 1.0.107](https://github.com/dtolnay/proc-macro2/blob/1.0.107/Cargo.toml)：复用已锁定版本，新增框架直接依赖边，不新增锁文件 package 或升级版本。仅启用解析、full AST 与字节位置能力，避免另写 Rust 词法器。二者声明 MSRV 1.71、MIT OR Apache-2.0；Rust 1.94.0 以实际构建验收。官方 HTTPS 调研材料保存在工作流 research/，均非 PHP 文件。

### 验收状态

方案定稿、实现完成、下列范围验收通过。源码快照、日志、TMPDIR 与独占缓存位于 `target/tmp/model-fields-sync-20261123/`，使用 Rust 1.94.0 与 build_guard；没有连接开发数据库。前置全量 WIP 已按用户要求提交为 `f80096ea6`；本修订没有暂存、提交或推送。并行归档提交 `25ec5d413` 保留；它未改变本修订技术文件。

所有命令均通过 `python3 -B tests/build_guard.py --workflow model-fields-sync-20261123 --` 执行。实际入口：

```sh
python3 -B target/tmp/model-fields-sync-20261123/layout-final.py
python3 -B target/tmp/model-fields-sync-20261123/post-layout.py
python3 -B tests/mysql_acceptance.py -- python3 -B target/tmp/model-fields-sync-20261123/user-check.py
python3 -B target/tmp/model-fields-sync-20261123/layout-docs.py
```

- `layout-final.py`：`cargo +1.94.0 fmt --all --check`、`clippy --workspace --all-targets --locked -- -D warnings`、`build --bin main --locked`、`test -p openrust-framework --doc model_fields --locked` 全部通过；Cargo 均显式指定隔离源码的 `--manifest-path`。2项宏 rustdoc 正反例通过。随后 `tests/model_fields_acceptance.py --source <source> --work <work>` 在 SQLite 3.54.1、PostgreSQL 18.7 与 MySQL 8.4.13 全部通过，后两者经对应 `tests/*_acceptance.py` 创建隔离实例。SQLite版本由同批实际链接的libsqlite3_sys查询，日志 `layout-sqlite-version.log`。
- 新增消费者实际验证：新建／刷新后位于顶部，普通输出无 allow／@model／机器标记，数据库说明可在 rustdoc 查看；旧区迁移、只读过期检测、刷新幂等，重复／损坏区域及非法Rust拒绝且原文不变；保留 doc/derive、inner 属性、BOM/shebang、原始字符串、同行导入、无导入模块和 CRLF 业务字节。原有真实查询、字段不存在／重命名后的编译失败、保留字、NULL、列映射、批量预检、模型表／连接复用、动态模型、普通生成器和自定义模板回归继续通过。日志 `layout-final-final-{sqlite,postgres,mysql}.log` 及其同名目录。
- `post-layout.py`：在隔离MySQL实际迁移并刷新根应用User，8个字段及真实描述放到顶部；逐字核对生成区外业务内容，保留密码／令牌常量的并行修改。生成物再次通过fmt、workspace/all-targets严格Clippy及main构建；2项应用骨架、7项Artisan、1项缓存配置下真实模型装配、1项真实TCP的密码重置及邮箱验证消费者全部通过。`layout-user-check.log` 额外验证重新构建后的新宏通过真实 `model:fields --all --check`，User内容和mtime不变。日志 `post-layout.log`、`layout-user-*`、`layout-console.log`、`layout-auth.log`、`layout-auth-http.log`。
- 三语 `getting-started.md` 同步顶部布局、旧区迁移、简洁宏、数据库说明及支持边界。`npm run build`（含内容和命令入口检查）、`node tests/check_browser.mjs` 与 `node tests/check_scope.mjs` 全通过，覆盖12组桌面／移动页面和6组三语导航，包含 `/` 与 `/docs/`。日志 `layout-docs{,-build,-browser,-scope}.log`。未将第三方既有warning或VitePress体积提示声明为零warning。

验收中先后发现严格Clippy的可合并条件及旧区移走后末尾多余空行，均修复并重跑；最终命令退出0。10项技术／正文文件与最终验收快照逐字节相同，`git diff --check` 通过。工作区原有归档与模型业务修改均保留，没有恢复已删除的历史计划。

本轮曾触及32GB停线，守卫安全终止构建，核对占用、持有全部Cargo锁后清理本项目编译产物；源码、日志、依赖下载、交付文件、数据库与数据卷保留。事件记录净释放93,007,200,256字节，锁inode保留；空间恢复后重跑并全部通过。收尾可用约88,336,404KiB，高于31,250,000KiB停线，继续保留当前可复用增量缓存。

本修订范围内无剩余实现阻塞。未重跑完整workspace测试、全部SQL／缓存驱动矩阵、all-features、Windows、release、实际SMTP或生产部署；这些不因局部通过计为完成。字段常量仍为未限定表名的`&'static str`，只提供引用存在性检查，不保证查询模型归属、值类型或外部数据库结构同步；这是可选增强，不是Laravel原生字段API，也不代表全框架完成对齐。


## 模型表名复用与数据库列顺序（2026-11-23）

### 方案定稿

沿用上一节的模型内顶部布局、配置式分区、简洁宏、AST 安全编辑与既有 Schema 查询。不增加旁路源码、管理器或查询框架。数据库结构是字段集合与列说明的唯一来源；表名配置继续来自模型或显式命令选项。

- `TABLE` 固定为第一项，其后严格保留 `getColumns()` 返回的数据库列顺序；名称索引只检查冲突，不参与排序。MySQL 按 ordinal_position、PostgreSQL 按 attnum、SQLite 按 cid 的现有结构读取无需修改。
- 标准模型静态元数据使用具体类型的 `User::TABLE`，表名字面量只保留在常量中。Rust 函数内 static 不允许外层 `Self`，实际生成物曾暴露 E0401，已改为具体模型名并重新验收。
- 首次刷新未编译模型时，可读取标准 `metadata()` 的 `ModelMetadata::new("table")` 字面量；否则使用既有推断。源码字面量与已编译模型不一致时拒绝隐式刷新，避免旧二进制静默选择另一张表。显式 `--table` 更新生成表名；标准模型重新构建后复用新表。
- 自定义 `getTable()`、动态表达式及非标准初始化保持原文，不强行重写业务配置。首次自定义连接仍须显式选择；后续复用已编译模型配置。
- `TABLE` 是保留常量，真实 `table` 列映射为 `COLUMN_TABLE`；与 `column_table` 冲突或模型手写 TABLE 时预检失败，不删除业务声明。
- 双行简短分区说明保留完整嵌套刷新路径。普通列说明输出 `///`，控制字符使用安全转义 doc 属性，缺少说明不猜测。警告抑制集中封装于宏内部，不出现在生成区。

### 调研与依赖

继续核对固定 [Model.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php) 的显式表名／默认推断与现有模型装配；常量是 OpenRust 可选增强，保留原有动态模型和字符串查询。对照 [SeaORM 1.1.19 列生成](https://github.com/SeaQL/sea-orm/blob/1.1.19/sea-orm-codegen/src/entity/column.rs)及 [Diesel 2.3.3 print_schema](https://github.com/diesel-rs/diesel/blob/v2.3.3/diesel_cli/src/print_schema.rs)，采用显式刷新、生成源码随应用构建的方式，不引入其实体或查询系统。固定框架 MySqlGrammar 与现有三个 Schema 驱动已逐项核对。本主题旧版分支未找到可直接复用的生成器。官方 HTTPS 资料保存于本批 research/table-*；无新增 PHP 文件。

表名修订没有新增或升级依赖；上一节锁定的 syn 2.0.119、proc-macro2 1.0.107 及其 MIT OR Apache-2.0 许可、MSRV 1.71 结论不变，Rust 1.94.0 真实构建通过。保留现有语法器与值绑定，不改变公共 SQL 行为。

### 实现与验收

本节方案定稿、实现完成，下列实际范围验收通过。全部临时源码、日志、数据库夹具临时文件和独占构建缓存位于 `target/tmp/model-fields-sync-20261123/`。沿用 build_guard、Rust 1.94.0、增量与正常并发；隔离源码 `source/`，独占 `cargo-target/`。保留现有和协作者改动，未操作开发数据，未暂存、提交、推送或发布。

所有执行入口均以前缀 `python3 -B tests/build_guard.py --workflow model-fields-sync-20261123 --` 运行：

```sh
python3 -B target/tmp/model-fields-sync-20261123/table.py
python3 -B target/tmp/model-fields-sync-20261123/table-post.py
python3 -B tests/mysql_acceptance.py -- python3 -B target/tmp/model-fields-sync-20261123/table-check.py
python3 -B target/tmp/model-fields-sync-20261123/table-docs.py
```

- `table.py`：隔离 manifest 的 `cargo +1.94.0 fmt --all --check`、`clippy --workspace --all-targets --locked -- -D warnings`、`build --bin main --locked`、2项宏 rustdoc 正反例均通过。随后真实 Artisan／rustc／查询消费者在 SQLite、PostgreSQL 18.7、MySQL 8.4.13 分别退出0；后两者通过项目夹具创建独立实例。日志 `table.log`、`table-{sqlite,postgres,mysql}.log` 与各消费者目录。
- 三库实际覆盖：name 在 id 之前以及 zeta/table/id/alpha 列顺序；TABLE 首位；真实模型及字符串查询结果一致；TABLE 用于已有查询及别名；NULL、保留字、列映射、注释及特殊字符；标准表名只出现一次；自定义表及连接；源码首次自定义表、旧编译元数据冲突拒绝、自定义 getter 原文保留；手写 TABLE 和 COLUMN_TABLE 冲突；旧底部与旧紧凑分区升级、doc/derive/inner 属性、BOM/shebang、CRLF、嵌套命令、重复刷新及只读检查；不存在／重命名字段编译失败；普通模型、当前与旧已发布自定义模板生成物真实编译。批量预检、未写入保护和原有动态模型能力保留。
- `table-post.py`：独立 MySQL 执行应用真实迁移并刷新 User。TABLE 后依次 ID、NAME、EMAIL、EMAIL_VERIFIED_AT、PASSWORD、REMEMBER_TOKEN、CREATED_AT、UPDATED_AT，均来自实际结构；8项中文列说明保留。生成区外仅标准 metadata 的表名引用变化，业务代码逐字一致，保留已有密码／令牌常量修改。该生成物通过 fmt、workspace/all-targets 严格 Clippy、main 构建、2项应用骨架、7项Artisan、1项缓存配置模型装配、1项真实 TCP 密码重置／邮箱验证和7项框架字段／绑定／主键回归。日志 `table-post.log`、`table-post-*.log`、`table-framework.log`。
- `table-check.py`：重新构建后的真实应用 `model:fields --all --check` 退出0，User内容与mtime不变。日志 `table-check.log`、`table-user-final-*.log`。
- 三语入门文档同步 TABLE、列顺序、迁移、重建与支持边界。文档 build（含命令入口和正文检查）、12组真实浏览器桌面／移动页面与6组三语导航全部通过，包含 `/` 和 `/docs/`。日志 `table-docs{,-build,-browser,-scope}.log`。

验收过程中修正了测试对 Query Builder 所有权／字符串借用的错误调用，并回收同一测试先前添加的列以免污染后续旧格式等价断言；这些失败不计为通过。最终11项技术／正文文件与实际验收快照逐字节一致，`git diff --check` 通过。最终空间检查仍高于32GB停线，保留可复用缓存；本节未执行清理。隔离实例已由夹具回收，不删除开发服务或数据卷。

本范围无剩余实现阻塞。未运行完整workspace测试、all-features、SQL Server、Windows、release、性能或生产部署；其他工作流改动不因本节通过获得验收结论。字段常量仍是未限定表名的字符串，只保证关联标识存在性，不保证查询模型归属、值类型或外部数据库列存在。普通构建无需数据库；结构及描述变更后仍需显式刷新并重新构建。动态表名逻辑不承诺自动改写或完整源码反射。

提交授权补记：用户于2026-11-23追加“提交代码”指令，本主题模型布局、表名及字段生成、既有密码／令牌常量引用、测试、三语文档与验收记录一起纳入本次本地提交；未推送。上文“未授权／未提交”均描述当时验收阶段，不再代表本次追加授权后的状态。提交前11项技术／正文文件仍与通过验收的隔离源码逐字节一致，未重复构建。

## 完整需求复核与安全刷新（2026-11-23）

### 方案与范围

按用户补充的14项目标重新核对源码及实际消费者。复用已提交的模型顶部布局、`TABLE`、数据库列顺序、字段说明、查询和生成模板，不重复实现。数据库结构仍是字段集合的唯一来源；表名和连接仍由模型映射及显式命令选项决定。普通构建不连接数据库，不新增ORM、服务包装、管理器或依赖。本轮未获得新的Git写入授权，不暂存、提交或推送。

| 用户目标 | 实际入口与裁决 |
| --- | --- |
| 1、9、10：查询体验及保障 | `database/eloquent/fields.rs`的关联字符串常量复用既有Query Builder；实际API为`r#where(User::NAME, "=", value)`，不新增示意`whereEq`。只保证关联常量存在，不检查模型归属、值类型或外部数据库列存在。字段不自动限定表名，联表／别名继续经既有标识符语法器处理，值参数绑定。 |
| 2、3、7：来源、生成及刷新 | `model_make_command.rs`与`model_fields_command.rs`复用Schema的`getColumns()`；TABLE第一，字段保留数据库列顺序，不从fillable/casts推测。保留单模型、批量、显式连接／表、只读check及标准模板。新增未重新构建的新模型复用生成TABLE的回退，避免自定义表名丢失。 |
| 4、5、6、13：目录、阅读及安全编辑 | 模型文件只保存映射与自身行为，框架负责数据库访问和生成，业务服务职责不变。imports后使用配置式分区与简洁宏，宏内集中抑制警告；AST定位保留文档、属性与业务原文。新增旧区完整形状及双重模型归属校验，安全拒绝混入业务项、外层属性或其他模型。 |
| 8：业务说明 | 继续使用数据库列说明生成`///`，控制字符用转义doc属性，无描述不猜测。保留并复验协作中的表描述、无描述时的本地化默认标签，以及APP_LOCALE三语生成说明；表／列描述保持数据库原文。 |
| 11：真实消费者 | 本轮验收状态见下文；生成文本检查与实际编译、运行、查询、编译失败反例分别执行。使用隔离数据库，未操作开发数据。 |
| 12、14：成熟方案与文档 | 延续固定上游与已锁定解析依赖，补查官方实现；三语文档明确命令、布局、重建及类型保障边界。 |

### 本轮发现与处理

1. 真实旧区最小实验中，将普通`business_logic`函数放在begin/end之间，刷新退出0且函数被删除。迁移不能只信任机器标记；现在要求区内恰好一个无外层属性的字段宏，完整解析旧生成语法，注册模型与内部impl类型都必须是当前模型。混入业务项、附加宏属性、其他注册模型或impl所属模型时拒绝且保留原文件。`legacy-wrong-owner`反例另外复现了仅检查注册名仍会误删其他模型声明的问题，已补双重归属检查。失败证据保留为`contract-boundary-before.log`和`contract-wrong-owner-before*.log`。
2. 新生成且尚未编入命令程序的自定义表模型，立即刷新曾回退到类名推断表名。现在优先级为显式选项、已编译映射、标准metadata字面量、现有生成TABLE、类名推断。复用已生成TABLE不需要数据库编译期反射；未编译的自定义连接仍须显式`--database`，重新构建后读取模型连接。动态配置不承诺源码反射。
3. 新增列顺序实际改变后的check／刷新／编译，以及连接前缀下生成模型和真实查询。TABLE保存逻辑表名，连接前缀交由原Schema和查询机制处理，不重复写入常量。
4. 保留并整合其他协作者的表描述与生成说明本地化修改，未覆盖其代码。`console::localization::text_for_locale`复用现有翻译入口，生成器读取当前应用译器语言；三语切换与表描述变化纳入消费者，不将其当作本轮独立重写。

### 调研依据

web检索返回空结果，使用实际HTTPS下载的官方固定源码继续核对；材料位于`target/tmp/model-fields-sync-20261123/research/contract-*`，未新增PHP文件。

- [Rust 1.94关联项](https://doc.rust-lang.org/1.94.0/reference/items/associated-items.html)：关联常量提供名称解析和补全，无自动数据库反射或字符串值类型推断。
- 固定[Model.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Model.php)及[模型测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Database/DatabaseEloquentModelTest.php)：保持表名推断／自定义映射、动态模型和字符串查询；字段生成属于OpenRust可选增强。
- [MyBatis-Plus v3.5.14 AbstractLambdaWrapper](https://github.com/baomidou/mybatis-plus/blob/v3.5.14/mybatis-plus-core/src/main/java/com/baomidou/mybatisplus/core/conditions/AbstractLambdaWrapper.java)：方法引用依赖序列化lambda、属性命名与列缓存，不能直接移植为Rust getter反射；采用关联常量满足补全体验。
- [SeaQuery 0.32.7标识符类型](https://github.com/SeaQL/sea-query/blob/0.32.7/src/types.rs)、前节SeaORM 1.1.19和Diesel 2.3.3生成器：借鉴显式结构生成、标识符与值分离，不引入平行实体、列枚举及查询框架。
- [IDE Helper v3.6.0 ModelsCommand](https://github.com/barryvdh/laravel-ide-helper/blob/v3.6.0/src/Console/ModelsCommand.php)：借鉴数据库结构／说明面向IDE生成，不要求开发者逐字段维护。

没有采用新依赖。继续使用锁定syn 2.0.119与proc-macro2 1.0.107，MIT OR Apache-2.0，声明MSRV 1.71；本轮Rust 1.94.0实际构建。新检查复用已有解析器，维护成本限于既有生成格式的显式迁移；任意业务Rust表达式不自动改写。

### 验收状态

方案定稿、安全刷新修复及消费者实现完成，下列本轮范围验收通过。独立源码、TMPDIR、日志、参考材料及独占Cargo缓存统一位于`target/tmp/model-fields-sync-20261123/`；沿用Rust 1.94.0、正常并发和增量缓存。没有访问开发数据。

实际执行入口如下，均通过守卫设置项目内TMPDIR与独占CARGO_TARGET_DIR：

```sh
python3 -B tests/build_guard.py --workflow model-fields-sync-20261123 -- python3 -B target/tmp/model-fields-sync-20261123/contract.py
python3 -B tests/build_guard.py --workflow model-fields-sync-20261123 -- python3 -B target/tmp/model-fields-sync-20261123/contract-followup.py
python3 -B tests/build_guard.py --workflow model-fields-sync-20261123 -- python3 -B target/tmp/model-fields-sync-20261123/contract-docs.py
```

- `contract.py`：隔离manifest的`cargo +1.94.0 fmt --all --check`、`clippy --workspace --all-targets --locked -- -D warnings`、`build --bin main --locked`及2项宏rustdoc正反例通过。真实`tests/model_fields_acceptance.py`分别在SQLite、隔离PostgreSQL 18.7、隔离MySQL 8.4.13退出0；后两者使用项目对应acceptance夹具。日志`contract.log`及`contract-{sqlite,postgres,mysql}.log`，各同名目录保存实际消费者输出。
- 三库实际覆盖生成／刷新／编译／查询、字符串查询等价、映射／保留字／NULL、字段增加删除重命名、列顺序改变、表和列描述改变、三语分区切换、表前缀、自定义表及已编译连接复用、未重建自定义TABLE、旧底部和旧紧凑布局升级、doc/derive/inner属性、BOM/shebang/CRLF、只读check内容和mtime不变、重复刷新稳定、批量预检、异常安全拒绝。不存在／重命名常量均确认因E0599编译失败；真实生成模型在`-D warnings`下编译，普通模型及自定义模板不退化。
- `contract-followup.py`先执行`contract-queries.py`：框架`--test model_fields`在三库分别通过，覆盖现有选择、筛选、排序、更新、关系、带别名自联表、参数绑定、动态属性、原始表达式及真实缺列错误。日志`contract-query-{sqlite,postgres,mysql}.log`。没有仅用生成文本代替查询执行。
- 随后`contract-post.py`使用独立MySQL执行真实应用迁移和User同步，验证TABLE复用及8项实际字段说明。真实生成物在TABLE上方直接显示数据库说明“用户表”，生成区外业务源码逐字节不变，根模型保留这一协作调整。生成物再次通过fmt、workspace/all-targets严格Clippy和main构建；2项应用骨架、7项Artisan、1项缓存配置模型装配、1项真实TCP密码重置／邮箱验证，以及7项框架字段／绑定／主键回归全部通过。日志`contract-post.log`与`contract-post-*.log`、`contract-framework.log`。
- 最后`contract-check.py`在独立MySQL、重新构建后的真实命令程序执行`model:fields --all --check`，退出0，User字节和mtime不变。日志`contract-check.log`、`contract-user-final-*.log`。
- `contract-docs.py`的`npm run build`（含正文和命令入口检查）、真实浏览器12组桌面／移动页面与6组三语导航全部通过，包含`/`和`/docs/`。日志`contract-docs{,-build,-browser,-scope}.log`。保留既有第三方编译warning与VitePress体积提示，不声明所有输出零warning。

最终8个技术／正文文件与通过验收的隔离快照逐字节相同，Python语法检查与`git diff --check`通过，暂存区为空。格式同步前先单独格式化根文件并比对快照，未用旧快照覆盖协作者语义改动。相关批次隔离数据库容器由夹具回收，其他工作流容器及数据卷保留。收尾核对还发现协作者将表描述调整为不加标签的原文，无描述时使用本地化“表名”标签；已保留该调整并修正连接前缀消费者，前一组合的完整通过日志保存在`before-table-label/`。最新格式已重新执行上述全部入口，全部退出0；8项技术／正文文件再次逐字节比对一致，Python语法、空白和空暂存区检查通过，6个本批隔离数据库容器均已回收。最终可用46,794,064KiB，高于31,250,000KiB停线，未清理可复用缓存。

本次14项目标所述范围没有剩余实现阻塞。未验完整workspace测试、all-features、SQL Server、其他数据库版本、Windows、release、生产及性能；三库关系查询通过不等于完整数据库矩阵全部功能通过。非ASCII／转换后冲突列名仍安全拒绝，SQLite不提供原生表／列描述；任意动态配置源码不自动反射。常量只提供名称存在性检查，外部结构变化仍须显式刷新并重新构建。其他协作者的全部工作、框架整体对齐及正式发布不在本结论内。未暂存、提交、推送或发布。

本轮文件范围：

- 实现：`vendor/openrust/framework/src/foundation/console/model_fields_command.rs`。
- 消费者：`tests/model_fields_acceptance.py`。
- 文档：`docs/en/eloquent/getting-started.md`、`docs/zh-Hans/eloquent/getting-started.md`、`docs/zh-Hant/eloquent/getting-started.md`。
- 接续记录：本文及`.agents/project/current.md`。
- 整合复验的协作文件：`app/models/user.rs`、`vendor/openrust/framework/src/console/localization.rs`、`vendor/openrust/framework/src/console/mod.rs`，以及上列文件中的协作生成说明／表描述改动。收尾时检测到并行本地提交`20ac72347`，已保留；本轮没有执行Git暂存或提交。显式核对上述8项技术／正文文件与最终验收快照一致，不依赖当前未提交文件数量推断源码组合。

## 生成说明本地化与表注释（2026-11-23）

按用户最终格式，`TABLE` 有数据库表描述时直接生成描述原文，不添加“表名：”等前缀；缺失或空白描述时生成当前语言的通用标签，英文为`Table`，简体／繁体中文为“表名”。生成分区标题与两行同步说明跟随应用`app.locale`／`APP_LOCALE`，复用既有Artisan翻译入口及`console.model_fields`键，不新增依赖。数据库表／列描述保持原文，不做自动翻译；控制字符沿用转义doc属性。保留模型模板原有`{{table_reference}}`占位符。

复用既有Schema `getColumns`／`getTables`及当前schema查找顺序、连接前缀，未引入平行元数据来源。参考本主题前节固定Laravel Schema及IDE Helper数据库说明生成实践；本次仅调整生成文本和既有本地化复用，不改变查询契约。

方案与实现已完成，本节范围验收通过。隔离源码、独占Cargo缓存、子进程TMPDIR及日志位于`target/tmp/model-table-doc-20261123/`。为保护同文件协作WIP，另以HEAD加本次差异的隔离组合复验，未将其他任务的安全刷新修改纳入本次提交。

实际命令：

```sh
python3 -B tests/build_guard.py --workflow model-table-doc-20261123 -- python3 -B target/tmp/model-table-doc-20261123/verify.py
python3 -B tests/build_guard.py --workflow model-table-doc-20261123 -- python3 -B target/tmp/model-table-doc-20261123/verify_docs.py
python3 -B tests/build_guard.py --workflow model-table-doc-20261123 -- python3 -B tests/mysql_acceptance.py -- python3 -B target/tmp/model-table-doc-20261123/refresh_user.py
```

Rust 1.94.0的fmt、框架all-targets严格Clippy、main构建通过。SQLite、隔离MySQL 8.4及PostgreSQL 18消费者分别验证三语刷新、只读check、无表描述的本地化标签、有描述的原文、描述变更／空白／删除、特殊字符及生成源码实际编译；既有查询与字段编译正反例通过。三语正文检查、站点构建、12组页面和6组导航浏览器检查通过。日志为`scoped-{fmt,clippy,build,sqlite,mysql,postgres,docs-content}.log`及`scoped-docs-{build,browser,scope}.log`。真实User使用隔离MySQL迁移后刷新及check通过，生成`/// 用户表`，字段描述保持原文；证据为`generated-user.rs`与`user-{migrate,refresh,check}.log`。

未验证完整workspace测试、all-features、SQL Server、Windows、release或生产部署；自定义翻译历史任意变更后的旧分区识别不作完整承诺。未推送或发布，其他协作者改动保留。


提交授权补记（2026-11-23）：用户追加“提交代码”，将“完整需求复核与安全刷新”的修复、消费者测试、三语文档及交接记录纳入本地提交。提交前5项剩余技术／正文文件与最新通过验收的隔离快照逐字节一致，Python语法及差异空白检查通过；复用上述验收结果，未重复构建，未推送或发布。

## 统一语言资源与刷新区域识别（2026-11-23）

方案定稿、实现完成；下述实际范围验收通过。基于 `720d0d9da` 核对并保留协作者的表描述、顶部布局、TABLE复用、AST与原子刷新保护，未重新实现查询框架。本轮没有暂存、提交、推送或发布。

### 采用方案与依据

- 阅读固定 Laravel Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [FileLoader](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/FileLoader.php) 与 [Translator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/Translator.php)：按语言／组加载资源，并按配置回退。复用OpenRust现有Lang译器，文案存入 `lang/<locale>/console.json`；公共函数只接收键、单一默认文案及可选语言，不再枚举简繁英文。命令帮助、列表、参数说明、生产确认和邮件生成提示同步迁移；保留命令级覆盖。
- 复核 [IDE Helper v3.6.0 ModelsCommand](https://github.com/barryvdh/laravel-ide-helper/blob/v3.6.0/src/Console/ModelsCommand.php) 的 `getPropertiesFromTable`：从模型连接／表结构生成IDE信息，符合本需求，不从fillable或casts推断全部字段。阅读 [SeaORM 1.1.19 EntityWriter](https://github.com/SeaQL/sea-orm/blob/1.1.19/sea-orm-codegen/src/entity/writer.rs)，继续采用显式生成源码而非普通构建连接数据库，不引入平行实体框架。前节Diesel、SeaQuery及MyBatis-Plus取舍保持有效。
- [Rust 1.94 原始标识符](https://doc.rust-lang.org/1.94.0/reference/identifiers.html#raw-identifiers)：`where`是关键字，方法调用必须使用 `r#where`；真实rustc反例确认 `.where`解析失败并提示原始标识符。
- 区域定位复用syn真实宏位置，紧邻配置式块注释仅检查结构，不读取标题、不枚举语言或历史译文。改标题或字段值后刷新替换同一区域；宏缺失／改名且仍留配置式说明、宏重复、边界损坏或旧区歧义时拒绝写入。翻译中的块注释开闭符均转义。
- 无新增依赖。复用锁定syn 2.0.119／proc-macro2 1.0.107（MIT OR Apache-2.0、MSRV1.71），本轮用Rust1.94.0实际构建。在线搜索工具本轮无返回，改为HTTPS获取上述固定官方资料并阅读；参考材料在本工作流research目录。

### 实际验收

所有隔离源码、日志、TMPDIR和独占增量缓存在 `target/tmp/model-fields-sync-20261123/`，通过 `tests/build_guard.py --workflow model-fields-sync-20261123 --` 执行；未操作开发数据库。源码位于 `source/`，Cargo target位于 `cargo-target/`。

- `python3 -B target/tmp/model-fields-sync-20261123/i18n.py`：Rust1.94.0 workspace fmt、workspace/all-targets严格Clippy（`-- -D warnings`）、main构建、model_fields两项rustdoc正反例通过。第三方vendor已有警告仍输出，未为本任务屏蔽。
- 同入口的 `tests/model_fields_acceptance.py`：SQLite、隔离PostgreSQL18.7、MySQL8.4.13全部通过；真实生成／刷新／编译与查询，原字符串兼容、缺字段编译失败、列顺序／描述、表映射、NULL、别名／联表、旧格式迁移、无副作用检查、重复刷新及损坏源码保护均保留。新增15份临时语言资源，连同现有3份验证18种语言标识的资源扩展；译文改变、注释开闭符转义、缺词回退和改生成标题／常量后不重复插入均通过。日志 `i18n-{sqlite,postgres,mysql}.log`，详细消费者日志在 `i18n-final-{sqlite,postgres,mysql}/`。
- 早期失败如实保留：首次Clippy暴露descriptor/list调用方未迁移，已修复；首次SQLite独立应用检查未携带语言资源，导致生成说明变为默认英文而判过期，已为独立应用复制真实语言资源，再跑三库通过，不恢复硬编码分支。
- `i18n-post.py`：隔离MySQL以 `DB_PREFIX=tenant_` 迁移真实应用、执行 `model:fields User`、`--check`及重复刷新通过。TABLE仍是逻辑表名 `users`，数据库访问使用连接前缀。根 `app/models/user.rs` 与真实刷新结果逐字节相同，故未制造文件差异；模型业务代码逐字节未变。证据 `i18n-user-final.rs`、`i18n-user-{migrate,refresh,check,repeat}.log`。
- 同入口复验fmt、严格Clippy、main构建，应用console／application_skeleton／mail_generator、缓存配置模型注册、真实TCP密码重置与邮箱验证HTTP、框架model_fields／query_bindings／eloquent_key_values全部通过。日志 `i18n-post-*.log` 与 `i18n-framework.log`。
- `tests/message_generator_terminal.py <独占target>/debug/main`：实际PTY交互、取消／非交互、三语提示、自定义法语帮助与命令级覆盖、缺词回退通过；日志 `i18n-terminal.log`。
- 三语Eloquent正文同步；`i18n-docs.py` 运行npm文档构建（含正文检查）、页面浏览器与根／子路径三语导航检查，结果及最终补充查询矩阵见下方收尾。

### 支持边界

这里只交付数据库生成关联字符串常量：检查Rust引用名称存在，不能检查查询模型归属、值类型或外部数据库结构。数据库变更后刷新、修复旧引用并重新构建。TABLE不固化DB_PREFIX。当前附带三份正式译文，18语言验收使用临时测试资源，不代表已提供18份完整翻译；以后增加语言资源即可，普通编译不需要数据库。

安全刷新采取保守策略：没有可识别字段宏但仍有配置式块注释时拒绝，可能包含用户手写同结构说明，需先人工消除歧义；任意删改所有识别结构后的文件不能保证自动辨识历史来源。生成区不是业务代码区。Windows、其他数据库版本／SQL Server、完整workspace测试及正式发布验收未执行。

收尾：`i18n-queries.py` 在SQLite、隔离PostgreSQL18及MySQL8.4执行真实筛选／排序／投影／更新／联表／关系／动态模型消费者，三库均通过，日志 `i18n-query-{sqlite,postgres,mysql}.log`。最终文档构建、12组页面浏览器、根与子路径6组三语导航全部通过，日志 `i18n-docs-{build,browser,scope}.log`。最终逐字节核对本轮代码、资源、测试、正文与已验收快照一致，User与实际生成物一致；暂存区为空、diff检查通过，本批数据库容器已回收。最终磁盘可用42,875,772 KiB，高于停线阈值，本轮未清理编译缓存。

用户随后于2026-11-23追加授权将本节已验收改动本地提交；提交前重新核对全部代码、资源、测试与正文和已验收快照逐字节一致。没有推送或发布授权。
