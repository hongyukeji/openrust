# 01-02 第十六部分 · Application 环境检测

2026-11-11 完成 Application 默认环境检测与 Artisan 环境选项验收。

- 对照固定 Laravel [LoadEnvironmentVariables](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bootstrap/LoadEnvironmentVariables.php)、[LoadConfiguration](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Bootstrap/LoadConfiguration.php)、[EnvironmentDetector](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/EnvironmentDetector.php)、[Application 环境查询测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Foundation/FoundationApplicationTest.php#L186)和 [EnvironmentCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/EnvironmentCommand.php)。文档对应[确定当前环境](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md#determining-the-current-environment)。
- Goravel 的 [main.go](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go) 提供编译型入口参考。OpenRust 在现有 console kernel 将参数送入引导。
- 检查旧版的 console/invocation.rs、config/app.rs、config/README.md 与 tests/unit/application_entry.rs。沿用现有文件选择器，复用双形式参数及首次选值的回归场景，依据固定 Laravel 补上 CLI 对检测名称的覆盖。
- 加载配置后、Provider 注册前绑定 `env`，读取 `app.env` 字符串，缺省为 `production`。安装使用弱容器引用的默认回调，查询读取 `env` 服务。配置修改和检测名称拥有各自的存储位置。
- `Application::environment(None)` 对应无参数查询。`Some` 支持单模式、模式切片、区分大小写的通配符以及空切片返回 false。Rust 用 Option 表达参数是否提供，用 EnvironmentResult 表达名称或布尔值。PHP 枚举模式可先转换为 Rust 字符串模式。
- CLI 首次 `--env` 值覆盖检测名称。文件依次尝试 CLI 值、外部 `APP_ENV` 和 `.env`。外部字符串 `0` 保持 PHP 的假值选择语义，进程变量继续优先。选项遵守 `--` 终止符，缺值和无效后缀走既有错误通道。Rust 路径后缀沿用 ASCII 字母、数字、连字符和下划线的范围。
- `artisan env` 提升为实际命令，保留上游描述与消息文本，采用现有 Rust 输出缓冲区实现 INFO 样式。ANSI 样式实现属于 Rust 终端适配。

## 验收与范围

macOS ARM64，Rust 1.94.0。工作区 289 项测试，隔离副本 270 项测试，另有 16 项命令子进程探针。两套格式、严格 Clippy、锁定离线 standalone release 均通过。[Rust 验收结果](rust-results.json)

两项新增集成测试覆盖 Provider 前置环境绑定、通配符、空切片、配置更新、无效配置类型与两个应用的并发真实 HTTP，以及 Artisan 两种参数形式、重复选项、文件回退、坏文件、无效参数和进程重试。发布回归通过 58 次 HTTP 与 6 次 Artisan。命令验收另通过 49 个进程，含文档中的 Artisan 调用。[HTTP 结果](http-results.json) · [命令结果](command-results.json)

三语构建、57 个 Rust 示例及 12 项浏览器检查通过。[文档结果](docs-results.json) · [示例结果](example-results.json)

16 个文件包括 9 个框架文件、1 个集成测试及 6 个三语页面。共享文件只提取环境相关差异。[文件 tree](changed-tree.txt) · [范围说明](scope-notes.json) · [SHA-256](candidate-sha256.json)

```sh
python3 .agents/project/audits/01-02p/prepare_candidate.py /tmp/openrust-01-02p-review
cd /tmp/openrust-01-02p-review
cargo +1.94.0 fmt --all --check
CARGO_INCREMENTAL=0 cargo +1.94.0 test --workspace --locked --offline
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings
CARGO_INCREMENTAL=0 cargo +1.94.0 build --locked --offline --release --features standalone
```

Application 默认环境检测缺口关闭。下一批核对全局实例和 ArrayAccess 的 Rust 接口。环境属性已满足默认解析器前置条件，SelfBuilding、类与参数属性及投影仍按计划推进。

本地代码提交 `753c08e5414719d1f47c425d5cde0c794322c0cd`，未推送。16 文件从不可变提交恢复校验通过。清理独占 target 1,380,284 KiB；磁盘可用约 51.73 GB，最近额度剩余 93%。保留候选源码、必要发布产物及共享缓存。
