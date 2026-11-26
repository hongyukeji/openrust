# 00-01 启动骨架和配置验收

本批复现脚本、清单和结果可在 Git 提交 `67d28aab` 的 `.agents/project/audits/00-01/` 下查阅。复现命令使用独立的历史检出。

2026 年 11 月 11 日完成实现核对与隔离验收，等待用户审阅提交范围。基线为 `197e7a0e`。

## 本批内容

- 根入口保留 Tokio 与 `bootstrap::createApplication` 接线，框架选择 HTTP 或 Artisan。
- `create()` 创建应用，选定 Kernel 引导环境、配置及 Provider。所有 Provider 注册后才依次引导。
- `config/app.rs` 负责应用身份、监听设置及 URL，`config/mod.rs` 只装配各配置分区。显式 `APP_URL` 优先，缺省 URL 随 `APP_PORT`。
- 进程环境覆盖文件值，`APP_ENV` 选择环境文件，不存在时回退 `.env`。源码构建从应用 crate 根目录读取配置，standalone 构建从可执行文件目录读取配置，绝对 `APP_BASE_PATH` 优先。
- 补齐三语安装页及应用目录说明，配置页补充 HTTP 监听和 Artisan 的端口校验边界。

`APP_PORT` 合法性继续在 HTTP 监听前校验。曾尝试提前到应用配置加载阶段，但这会使 Artisan 因无效的 HTTP 端口而失败，全量回归发现后已恢复原行为。验收确认 HTTP 拒绝非法端口，Artisan 仍能运行。

## 文件范围和复现

逐文件 tree（`commit-tree.txt`）共 400 个候选路径。运行配套为 377 个路径，回归为 4 个路径，正文及根 README 为 19 个路径。与基线相比共新增 380 个文件、修改 20 个文件。已提交的 `main.rs`、路由、Artisan 脚本、环境模板及其他基线文件由 `git archive` 保留。

- `runtime-files.txt`记录运行配套。
- `test-files.txt`记录回归文件。
- `doc-files.txt`记录正文文件，其中四个主题为本批直接交付，Provider 与异常页面提供已有链接和启动行为说明。
- 当时的候选排除了 27 个共享文件中的 gRPC、广播、限流、分页、集合、模板引擎骨架和可选路由安装器等接线，并保留原工作区内容。历史范围以本节固定提交和候选构造工具为准；临时排除补丁已清理，见[清理复核](../worktree-inventory.md#历史补丁清理)。
- `candidate-sha256.json`记录候选的文件校验值。构造工具会检查校验值，源文件变更后需要重新核对候选。

在仓库根目录执行以下命令，目标目录须尚不存在。

```sh
git worktree add --detach /tmp/openrust-00-01-evidence 67d28aab1629ffc7364f0a8f8ebbe018cf14eecc
python3 /tmp/openrust-00-01-evidence/.agents/project/audits/00-01/prepare_candidate.py /tmp/openrust-00-01-review
cd /tmp/openrust-00-01-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --release --locked --offline --features standalone
```

离线构建要求本机已下载锁定依赖。构造工具从固定基线提取已提交文件，再应用经审阅的工作区候选。它不会暂存或提交文件。

运行历史检出中的实际进程验收，结果写入 `runtime-results.json`。

```sh
python3 /tmp/openrust-00-01-evidence/.agents/project/audits/00-01/verify_runtime.py /tmp/openrust-00-01-review
```

## 实际结果

macOS ARM64、Rust 1.94.0 上，工作区 41 项测试、格式检查与严格 Clippy 通过。隔离候选的 15 项测试、格式检查、严格 Clippy 和锁定离线 standalone 发布构建通过。相同候选再次由构造工具重建，400 个文件的校验值匹配，随后编译检查通过。

实际进程覆盖九种 HTTP 配置场景，每个场景发出两个请求，并在 HTTP 监听期间执行 `inspire`、`list` 和 `help list`，全部通过。另验证相对根路径拒绝、三个非法 HTTP 端口的 HTTP 失败和 Artisan 成功，以及异地工作目录运行源码产物、Artisan 脚本和 Cargo 入口。共 18 个 HTTP 请求，另有 33 次成功的 Artisan/Cargo 调用和 5 次预期失败的启动检查。

三语候选正文构建和 12 项浏览器检查通过，覆盖 `/` 和 `/docs/`。Windows 实机验收待完成。单独的 `.env` 缓存、加密、维护模式及各驱动完整行为按对应单元继续验收。

## 上游对应与旧版复用

本批对应 [00-01](../../plan.md#unit-00-01)，关闭启动骨架、环境选择、配置优先级及应用目录段落。配置章节的缓存、环境文件加密和维护模式等内容保留待后续单元核定。安装页的本地与 standalone 命令已运行，发布平台支持矩阵由 08-04 验收。

Laravel 以应用骨架 `06d016a364a37430eec9cbc52209adce3d7667ce`、框架 `1449bf241017087fbd38c118ecaabef3f6797fdc` 和文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8` 为准，Goravel 以应用骨架 `31651f646ccedf1701471bf01285111270a9d370` 为启动方式参考。

Rust 用静态模块和配置加载函数表达 PHP 的文件载入，保留 `withMiddleware` 和 `withExceptions` 闭包。源码和独立部署两种根目录规则、内置 HTTP 端口以及 URL 随端口默认生成是已确认的语言和运行方式适配。依赖树中其他模块的类型骨架只计入编译依赖，功能完成状态按各自验收单元记录。

只读核对旧版 `6d1a8192` 的 `config/app.rs` 与 `vendor/openrust-framework/tests/unit/config/application_sources.rs`，沿用应用 URL 复用端口、配置来源优先级的契约和回归思路。当前入口采用新主线的配置仓库与根目录机制。

目录清单复核补充：另有 1 个必要删除路径（`deleted-files.txt`），即 `bootstrap/cache/.gitignore`。构造工具显式移除基线归档中的该文件及空目录；先前运行验收时文件仍在副本中，但不参与编译或启动。选定文件的历史 SHA-256 保留不变；删除规则已在累计 00-03 候选重新构造核对。已提交阶段的构造工具优先按 `git-commit.txt` 从不可变提交恢复，源工作区变化不影响复现。

## 本地提交

按用户 2026-11-11 新授权，已提交本地 Git：`050a8dab9917f82166f618d00e65548bf33f32db`。本地提交范围为 401 个变更路径；前述“未提交”描述为验收完成时的历史状态。未推送。工作区中候选排除的其他模块内容保留。

## Config 门面

固定 Laravel 提交 `1449bf2410` 的 `Config` 通过 `config` 容器名称委托到配置仓储。旧版 `v0.x` 使用 `ConfigRepository::get` 读取点分键。当前 `openrust::support::facades::Config::get` 沿用规范绑定和配置仓储，在应用作用域内解析，返回 `Result<Option<Value>, BindingResolutionError>`。读取可见同一应用内的规范绑定替换。

以本地提交 `946232c61` 的隔离副本验收。两项定向测试覆盖 HTTP 路由、并发应用隔离、规范仓储重绑定、缺失键、显式空值和引导前访问。全目标严格 Clippy、本批格式检查、三语文档构建及两个部署路径的十二项浏览器检查通过。全仓格式检查仅报告既有 Sanctum 断言换行差异。

## 应用名称默认取 Cargo 包名（2026-11-19）

应用 `config/app.rs` 的 `APP_NAME` 缺省值改为 `env!("CARGO_PKG_NAME")`，与版本的 `env!("CARGO_PKG_VERSION")` 使用相同的编译期包信息。运行时 `.env` 显式配置继续优先，骨架示例的 `APP_NAME=OpenRust` 仍覆盖包名。现有配置回归补充名称和版本的默认值、名称显式覆盖断言。

Rust 1.94.0 配置测试、应用二进制及配置测试的严格 Clippy、全仓格式和差异空白检查通过。三语配置文档同步，站点构建及两种部署路径的 12 项浏览器检查通过。本批浏览器依赖已清理，共享构建缓存保留。


## 默认骨架扩展边界复查（2026-11-21）

用户要求在 Provider 修正后检查其他位置。检查基线为 `81dd7a148`，工作区仅有原先未跟踪的 `.agents/project/memory/`，未读取或改动其中内容。本轮检查默认 app／bootstrap／config／routes／database、Cargo features／包发现、相关包 Provider、核心安装器及骨架回归测试；属于源码审查，未修改运行时代码，也未重新运行 Cargo、三库或文档站验收。

确认仍存在的同类问题：

- `config/mod.rs` 无条件装配 `oidc` 与 `reverb`。关闭对应 Cargo feature 仍会读取专用环境配置并把这些配置组纳入应用配置；这不表示服务或路由已自动启用。`config/oidc.rs` 还预置名为 `work` 的身份提供方示例。
- 根 `config/settings.rs`、`config/pay.rs` 虽用 feature 隔离，仍由默认应用承担包默认配置。Settings Provider 已有相同默认值，形成双重维护；Pay Provider 尚未合并包默认配置，独立依赖包的应用需要自行提供配置。feature 隔离没有解决默认配置的所有权。
- `tests/application_skeleton.rs` 只覆盖 app/providers 与 bootstrap/providers，未覆盖配置组或默认业务模块；`tests/configuration.rs` 反而要求默认存在 oidc／reverb 和 work 示例。这解释了 Provider 已修正但其他入口仍有同类遗漏。

处理方案：沿用现有 `mergeConfigFrom`、包发现与资源发布机制，将扩展默认配置及可发布模板归所属包；应用定制由显式发布／安装生成，OIDC 业务提供方由采用方配置。Reverb 配置抽离时须保留广播客户端的连接选项能力，并修改 init 的发布前置流程；不能只删配置文件或加 feature 分支。Settings／Pay／OIDC／Reverb 同步验证包独立使用、关闭／启用、应用覆盖、配置缓存及重复安装保留定制；再扩展骨架测试，区分核心驱动配置、包资源和显式生成代码，避免用所有扩展名称的全局禁用代替边界验收。上述为本轮发现及修复方案，尚未实现，不能沿用上一批成功验收宣称已关闭。

未判为同类问题：默认 User／用户工厂／DatabaseSeeder 与 Laravel 应用骨架职责一致；根路由没有扩展演示登录或支付端点；Cargo optional 依赖声明与通用包发现本身不会开启包。Passport／Fortify 等显式安装生成的应用代码属于安装结果。短信 `sms_deliveries` 是核心排队短信的提交去重／失败状态记录，生产 Worker 依赖该表，非测试业务示例；其默认资源策略应单独评估，不可在本轮按扩展演示代码直接删除。SMS、gRPC、images 当前位于框架核心，不能仅因属于 Laravel 之外的能力就视作可选包误装配。

调研：先复核现有包发现／Provider 合并与发布机制及旧版 `v0.x:app/providers/app.rs`。web 搜索没有返回可用结果；直接 HTTPS 成功读取固定 Laravel 应用配置目录 `06d016a364a37430eec9cbc52209adce3d7667ce` 与框架 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 ServiceProvider，另读取 Laravel Reverb `v1.6.3` 的 ReverbServiceProvider。应用骨架不预装上述扩展配置；Reverb 包在 register 合并自身配置，在 boot 声明 reverb／reverb-config 发布组，支持沿用项目现有机制，无需新建装配系统。后者为版本标签引用，未另行固定不可变提交。

来源：[Laravel 应用配置](https://github.com/laravel/laravel/tree/06d016a364a37430eec9cbc52209adce3d7667ce/config)、[ServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/ServiceProvider.php)、[ReverbServiceProvider](https://github.com/laravel/reverb/blob/v1.6.3/src/ReverbServiceProvider.php)。本轮只向现有主题追加结论，不保存源码副本／patch 或重建产物；没有操作数据库、容器或数据卷，没有暂存、提交或推送。


## 扩展配置归属修复与验收（2026-11-21）

上一节发现已实现并通过以下定向验收，替代“尚未实现”的停点。复用该节固定 Laravel 的包配置合并、发布及缓存机制，以及项目已有包发现、ExtensionInstallers 和 SourceInstallation 源码锁；未增加依赖或第二套装配系统。

- 根 `config/` 移除 Settings／Pay／OIDC／Reverb 文件及加载项。前三者中的 Settings／Pay 与 Reverb 在所属包维护同一份默认配置和可发布模板；OIDC 默认 `providers` 为空，`work` 仅在显式发布的模板中提供。广播客户端连接选项由根 `broadcasting` 自身维护，不再依赖可选服务端配置。
- 四包提供具名 `*-config` 发布标签。通用发布只生成文件，编译型加载须按三语文档显式登记并重建；OIDC／Reverb 的显式 init 复用已有安装器自动发布、登记，重复执行保留定制，force 才覆盖模板。注册失败不会写入不完整加载器。
- 核心公开 `configurationIsCached()`；浅合并和递归合并在缓存命中后均跳过。Settings／Pay／Reverb／Passport／Fortify／Passkeys 先判断缓存，再读取环境默认值，防止环境改变污染快照或令缓存应用启动失败。
- 骨架回归从 Cargo 可选包声明检查根配置预装，并覆盖默认 app／routes；根配置回归要求准确的核心配置组。规范补充默认配置归属、显式生成、缓存边界及验收要求。

隔离源码以 `81dd7a148` 的 Git 归档叠加本批核定路径构造，目录 `/private/tmp/openrust-config-boundary-20261121`；不包含并行 Cashier、Cargo 清单及锁文件、支付计划和 Cashier 文档改动。Rust 1.94.0、jobs=2、incremental=0、dev debug=0，嵌套 Cargo 显式继承同一工具链。执行结果：

1. 默认 feature 的骨架／配置／包配置 7 函数通过；开启 `settings,oidc,reverb,pay` 的根回归 28 函数通过，包含配置缓存、安装器、Pay 迁移及数据保留。原生 feature-off／on 脚本验证默认组、空 OIDC、缓存、显式发布及重复保留均通过。
2. Settings 包测试在 SQLite、PostgreSQL 18、MySQL 8 分别执行，每轮 15 函数通过；其中 Provider 专项固定使用 SQLite，不将其计作三驱动迁移。Pay 原生迁移／值保留／回滚在三库通过。PG／MySQL 使用现有临时容器入口及必需连接标志，不接触开发库；Settings 原生安装／缓存／实际 HTTP 脚本通过。
3. 四份发布模板实际编译并运行，应用覆盖与缓存值正确。已显式配置的应用执行 `init --extensions=all --without-migration -n` 成功并保留原定制，覆盖 Passport／Fortify 的组合生成流程；本项不计为全新应用的完整认证迁移或业务认证复验。
4. 最新 Passkeys 修复复制到上述已安装应用后，全 feature 实际重建；运行 `tests/extension_configuration.py <binary> --authentication-root <application>`，先生成包含 Fortify／Passkeys／Passport 的缓存，再把两种环境密钥设为非 UTF-8，三组 `config:show` 成功且快照字节不变。初版根测试因只启用 Fortify feature、未显式安装而没有执行目标 Provider，已移除并改用本项真实安装夹具，未将该失败结果计入通过项。
5. 最终 `cargo +1.94.0 fmt --all --check` 及 `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked --offline -- -D warnings` 通过。三语内容检查（108 章节／110 文件）、构建、根／子目录的12项普通浏览器及12项 Settings 浏览器检查通过，另核对三语全部导航路由及26个支持页面。

日志、运行脚本和生成应用保留在该临时批次目录，源码与历史差异由本次 Git 提交保存，不另存 patch／源码副本到审计目录。未复验真实 OIDC 供应商、商户资金收付、跨平台部署或完整认证业务；并行 Cashier 功能由扩展对话独立验收，不并入本批结论。

清理前 `df -kP .` 可用33,456,236 KiB；确认 target 无打开文件后，仅删除本批独占target、两个复制的原生二进制与生成应用的文档dist，清理后可用43,987,836 KiB，净增10,531,600 KiB。生成应用／日志／SQLite及共享node依赖保留，Docker卷清单前后均为空。API指纹构建器已将包`config/`列为编译输入；历史API快照继续按自身源码指纹适用，未重新生成全库API交付。
