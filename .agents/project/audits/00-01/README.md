# 00-01 启动骨架和配置验收

2026 年 11 月 11 日完成实现核对与隔离验收，等待用户审阅提交范围。基线为 `197e7a0e`。

## 本批内容

- 根入口保留 Tokio 与 `bootstrap::createApplication` 接线，框架选择 HTTP 或 Artisan。
- `create()` 创建应用，选定 Kernel 引导环境、配置及 Provider。所有 Provider 注册后才依次引导。
- `config/app.rs` 负责应用身份、监听设置及 URL，`config/mod.rs` 只装配各配置分区。显式 `APP_URL` 优先，缺省 URL 随 `APP_PORT`。
- 进程环境覆盖文件值，`APP_ENV` 选择环境文件，不存在时回退 `.env`。源码构建从应用 crate 根目录读取配置，standalone 构建从可执行文件目录读取配置，绝对 `APP_BASE_PATH` 优先。
- 补齐三语安装页及应用目录说明，配置页补充 HTTP 监听和 Artisan 的端口校验边界。

`APP_PORT` 合法性继续在 HTTP 监听前校验。曾尝试提前到应用配置加载阶段，但这会使 Artisan 因无效的 HTTP 端口而失败，全量回归发现后已恢复原行为。验收确认 HTTP 拒绝非法端口，Artisan 仍能运行。

## 文件范围和复现

[逐文件 tree](commit-tree.txt)共 400 个候选路径。运行配套为 377 个路径，回归为 4 个路径，正文及根 README 为 19 个路径。与基线相比共新增 380 个文件、修改 20 个文件。已提交的 `main.rs`、路由、Artisan 脚本、环境模板及其他基线文件由 `git archive` 保留。

- [runtime-files.txt](runtime-files.txt)记录运行配套。
- [test-files.txt](test-files.txt)记录回归文件。
- [doc-files.txt](doc-files.txt)记录正文文件，其中四个主题为本批直接交付，Provider 与异常页面提供已有链接和启动行为说明。
- [scope-exclusions.patch](scope-exclusions.patch)记录从当前工作区到候选的差异，覆盖 27 个共享文件。候选排除了 gRPC、广播、限流、分页、集合、模板引擎骨架和可选路由安装器等接线，工作区保留原内容。
- [candidate-sha256.json](candidate-sha256.json)记录候选的文件校验值。构造工具会检查校验值，源文件变更后需要重新核对候选。

在仓库根目录执行以下命令，目标目录须尚不存在。

```sh
python3 .agents/project/audits/00-01/prepare_candidate.py /tmp/openrust-00-01-review
cd /tmp/openrust-00-01-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --release --locked --offline --features standalone
```

离线构建要求本机已下载锁定依赖。构造工具从固定基线提取已提交文件，再应用经审阅的工作区候选。它不会暂存或提交文件。

回到仓库根目录运行实际进程验收，结果写入 [runtime-results.json](runtime-results.json)。

```sh
python3 .agents/project/audits/00-01/verify_runtime.py /tmp/openrust-00-01-review
```

## 实际结果

macOS ARM64、Rust 1.94.0 上，工作区 41 项测试、格式检查与严格 Clippy 通过。隔离候选的 15 项测试、格式检查、严格 Clippy 和锁定离线 standalone 发布构建通过。相同候选再次由构造工具重建，400 个文件的校验值匹配，随后编译检查通过。

实际进程覆盖九种 HTTP 配置场景，每个场景发出两个请求，并在 HTTP 监听期间执行 `inspire`、`list` 和 `help list`，全部通过。另验证相对根路径拒绝、三个非法 HTTP 端口的 HTTP 失败和 Artisan 成功，以及异地工作目录运行源码产物、Artisan 脚本和 Cargo 入口。共 18 个 HTTP 请求，另有 33 次成功的 Artisan/Cargo 调用和 5 次预期失败的启动检查。

三语候选正文构建和 12 项浏览器检查通过，覆盖 `/` 和 `/docs/`。Windows 实机验收待完成。单独的 `.env` 缓存、加密、维护模式及各驱动完整行为按对应单元继续验收。

## 上游对应与旧版复用

本批对应 [00-01](../../plan.md#unit-00-01)，关闭启动骨架、环境选择、配置优先级及应用目录段落。配置章节的缓存、环境文件加密和维护模式等内容保留待后续单元核定。安装页的本地与 standalone 命令已运行，发布平台支持矩阵由 08-04 验收。

Laravel 以应用骨架 `06d016a364a37430eec9cbc52209adce3d7667ce`、框架 `1449bf241017087fbd38c118ecaabef3f6797fdc` 和文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8` 为准，Goravel 以应用骨架 `31651f646ccedf1701471bf01285111270a9d370` 为启动方式参考。

Rust 用静态模块和配置加载函数表达 PHP 的文件载入，保留 `withMiddleware` 和 `withExceptions` 闭包。源码和独立部署两种根目录规则、内置 HTTP 端口以及 URL 随端口默认生成是已确认的语言和运行方式适配。依赖树中其他模块的类型骨架只计入编译依赖，功能完成状态按各自验收单元记录。

只读核对旧版 `6d1a8192` 的 `config/app.rs` 与 `vendor/openrust/framework/tests/unit/config/application_sources.rs`，沿用应用 URL 复用端口、配置来源优先级的契约和回归思路。当前入口采用新主线的配置仓库与根目录机制。

目录清单复核补充：另有 [1 个必要删除路径](deleted-files.txt)，即 `bootstrap/cache/.gitignore`。构造工具显式移除基线归档中的该文件及空目录；先前运行验收时文件仍在副本中，但不参与编译或启动。选定文件的历史 SHA-256 保留不变；删除规则已在累计 00-03 候选重新构造核对。00-01 的源文件后续已有变化，历史构造脚本会按校验值拒绝不同内容，应使用最新累计候选。

## 本地提交

按用户 2026-11-11 新授权，已提交本地 Git：`050a8dab9917f82166f618d00e65548bf33f32db`。本地提交范围为 401 个变更路径；前述“未提交”描述为验收完成时的历史状态。未推送。工作区中候选排除的其他模块内容保留。
