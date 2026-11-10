# 00-03 Artisan 基线与发布目录

2026-11-11 按本单元条件验收通过。代码基线为 `197e7a0e`，依赖 00-01 和 00-02 候选。未暂存、提交或推送。

## 对照与实现

- [固定 Laravel artisan 入口](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/artisan)调用 Application::handleCommand 并退出。[固定 Application::handleCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php)依次调用 Console Kernel handle、terminate 并返回状态。当前 OpenRust 保留同名职责和顺序。
- [固定 console.php](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/routes/console.php)通过 Artisan 门面注册 inspire 并设置 purpose。现有 console.rs 保留同样结构，Rust 用 register() 和 Result 装配。
- Goravel 固定应用 `31651f646ccedf1701471bf01285111270a9d370:main.go` 只调用 bootstrap.Boot 与 app.Start。OpenRust 保留已确认的单二进制入口，由框架分流 HTTP/Artisan，应用 main.rs 只负责 Tokio 与 bootstrap 接线。
- 核对旧版 `6d1a8192:tools/cli/tests/unit/artisan_entry.rs`。提取参数原样传递、空格及 Unicode 路径、工作目录、exec 同进程和退出码回归思路；未带回旧版部署分流和品牌环境变量。
- 文档对应固定 `960eedf0911155ad16326ade49c0ea5d18eda0b8` 的 artisan#introduction、artisan#registering-commands 与 deployment。

本批不改动入口实现。补充 Unix 脚本回归，纳入已有 7 项 CLI 进程回归；补齐三语源码/发布入口及部署说明。

## 差异与边界

PHP 的独立 HTTP 和 Artisan 入口在 Rust 中共享 main 二进制；源码脚本负责调用 Cargo，独立发布运行 main artisan，不携带 Cargo。无模式参数启动 HTTP，artisan 模式执行命令，其他模式返回外层用法与退出码 2。Artisan 参数或分派失败返回 1，成功返回 0。

命令清单中的 Planned 与 implemented 字段为骨架状态说明，不能视为对应命令已实现。帮助排版使用 Rust Clap，非 Symfony 控制台的逐字输出；完整命令 I/O 与本地化归入 06-08。Windows artisan.bat 仅保留既有静态审查，未做 Windows 实机验收。未将可选安装器、gRPC 或广播接线纳入最小候选。

## 文件范围与复现

[本批 8 文件 tree](changed-tree.txt)包含 2 个回归入口与三语的 artisan/deployment 共 6 页。加上前序必要运行配套，共 [415 个候选文件](commit-tree.txt)。[清单](commit-files.txt)、[校验值](candidate-sha256.json)和[排除补丁](scope-exclusions.patch)保留可复现边界，工作区其他 WIP 未被移除。候选的 Artisan 页面排除了可选路由安装器段落，当前工作区保留该段。

```sh
python3 .agents/project/audits/00-03/prepare_candidate.py /tmp/openrust-00-03-review
cd /tmp/openrust-00-03-review
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
cargo +1.94.0 build --locked --offline --release --features standalone
```

另一副本重新构造后，415 个选定文件 SHA-256 一致。macOS ARM64、Rust 1.94.0 下隔离 24 项测试、格式检查、严格 Clippy 和 release 构建通过。工作区 43 项测试与严格 Clippy 通过。

在根目录运行真实进程验收：

```sh
python3 .agents/project/audits/00-03/verify_runtime.py /tmp/openrust-00-03-review
```

[40 次进程结果](runtime-results.json)全部满足预期状态，覆盖源码版、异地发布版、无命令等同 list、两种 help、inspire 普通/ANSI、文本/JSON/XML/Markdown 列表、未知命令、非法选项和参数、Planned 拒绝、错误模式先于应用创建、源码脚本与 Cargo 入口。HTTP 端口无效时仍可执行 Artisan。发布二进制位于包含空格的目录，从外部工作目录启动。

三语文档构建与根路径、/docs/、三语桌面及移动的 12 项浏览器检查通过。HTTP 与磁盘的真实请求沿用前序 00-02 的 58 项验收，本批没有修改运行框架。

目录清单复核补充：另有 [1 个必要删除路径](deleted-files.txt)，即 `bootstrap/cache/.gitignore`。构造工具显式移除基线归档中的该文件及空目录；先前运行验收时文件仍在副本中，但不参与编译或启动。选定文件内容及 SHA-256 未变，删除后重新构造核对通过。

## 本地提交

按用户 2026-11-11 新授权，已提交本地 Git：`29f18d628d38b0a2aa6d282632180c9d3c0a2e8c`。本地提交范围为 8 个变更路径；前述“未提交”描述为验收完成时的历史状态。未推送。工作区中候选排除的其他模块内容保留。
