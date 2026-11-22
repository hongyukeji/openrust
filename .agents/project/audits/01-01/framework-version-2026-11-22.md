# 框架版本查询（2026-11-22）

## 范围与依据

源码基线 `b214e46be62e8af975885e7bb0175eb37c5ba592`。用户授权实现和隔离验收，未授权暂存、提交、推送或发布。保留 SetLocale 与其他对话框的 framework-parity 在途修改。本批材料、参考源码、快照、日志和独占 Cargo 目录在 `target/tmp/framework-version-20261122/`。

固定 Laravel Framework v13.40.0（`1449bf241017087fbd38c118ecaabef3f6797fdc`），没有升级基线：

- [Foundation Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php) 的 VERSION 与 version()：实例返回框架常量，不从应用配置读取。官方固定原始源码与项目已有固定副本逐字节一致。
- [App Facade](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades/App.php) 声明 version()，实际解析 app 服务。Rust 沿用 getFacadeRoot 与 BindingResolutionError，而非绕过应用解析直接读常量；未使用与版本无关的 TranslationError。
- [Foundation Console Kernel](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/Kernel.php) 的 getArtisan() 将 app->version() 传入 [Console Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/Application.php) 构造函数，再交底层命令解析器。OpenRust 的 Console 执行入口已有 Foundation Application 参数，因此在构建解析器时传 application.version()，不额外存储同一版本，也不新增构造入口。所有解析路径，包括补问参数和程序调用，使用同一来源；保留已有 `artisan <version>` 及子命令输出格式，不将上游品牌或13.40.0写为本项目版本。
- 同时读取固定 FoundationApplicationTest.php，未发现直接针对 version() 的断言；本批按实际公开消费者补回归，不声称运行了上游 PHPUnit。查阅本地 v0.x 的 foundation/application.rs 未发现可复用的同名版本实现，复用当前装配、Facade 和 Console 机制即可。

## 实际修改

- Foundation Application 增加 `pub const VERSION: &'static str = env!("CARGO_PKG_VERSION")` 及 `version(&self)`，唯一版本来源为框架包 package.version。
- App Facade 增加 `version() -> Result<&'static str, BindingResolutionError>`。常量及实例方法在 boot 前可读，Facade 仍要求应用作用域。
- Console 解析器接收 Application 返回的版本，移除其单独的 env! 读取点。应用 config/app.rs 与 APP_VERSION 机制保持原样。
- 框架、根应用版本测试覆盖 boot 前读取、作用域错误、真实命令及 Facade、环境覆盖和运行期 app.version 修改；三语配置正文解释两种版本及调用方式。
- 扩大现有根 console 回归时发现 facades_keep_concurrent_applications_isolated 夹具只传 name/version，遗漏欢迎页当前所需 language，实际HTTP返回500。仅为该测试两条路由补 `.with("language", "en")`，不修改生产欢迎页。

## 验收

以 Rust 1.94.0、项目内 TMPDIR、正常并发与增量执行。最初复用旧工作流缓存时发现另一工作流实际持锁，终止本批等待中的 Cargo（退出-15，不计通过），改为本批独占 cargo-target，未中止对方进程。磁盘各阶段实测记录在 results.jsonl，未触及32GB停线；不操作开发数据库、缓存、卷或本地.env。

第一份 source 是包含当时协作者修改的完整隔离快照；框架版本及 console_observation 三项通过。完整工作区 fmt 检查发现协作者 validation Factory／extensions 在途格式差异，未代为修改。后续 source-scoped 使用 HEAD 加本轮文件，单独验证本轮可交付范围；不能把 scoped 质量检查扩展为其他协作者的未完代码全部通过。后续命令和结果如下。

最终执行与结果（各完整命令、退出码及空间前后值见 results.jsonl）：

| 范围 | 实际命令／消费者 | 结果 |
| --- | --- | --- |
| 框架及命令事件 | `cargo +1.94.0 test --locked -p openrust-framework --test application_version --test console_observation` | 本轮2项及既有1项通过，0 ignored；工作区快照与本轮范围快照均实际运行 |
| 根应用及 Artisan | `cargo +1.94.0 test --locked --test framework_version --test console` | 最终8项通过，0 ignored；此前遗漏language的失败退出101保留在scoped-root.log，修复后的结果在scoped-root-final.log |
| 不同包版本消费者 | 隔离应用package.version改为7.6.5-consumer，框架package.version保持0.0.0；`cargo +1.94.0 test --locked --test framework_version --test version_consumer -- --nocapture` | 2项通过，实际输出consumer=7.6.5-consumer、framework=0.0.0、facade=0.0.0；不是仅比较当前相同的两个0.0.0。临时测试只在验收源码中，脚本结束恢复应用Cargo.toml／Cargo.lock并移除临时消费者文件 |
| 真实源码入口 | `./artisan --version` 与 `./artisan config:show app`，在正常应用和7.6.5-consumer应用中分别测试未设置APP_VERSION及APP_VERSION=9.8.7-cli | 4组、共8个真实入口进程通过；版本始终artisan 0.0.0，应用配置分别为0.0.0／7.6.5-consumer或环境覆盖9.8.7-cli。check_entry.py仅是捕获及断言的验收驱动，不是正常启动依赖。证据artisan-entry.log、consumer-artisan.log |
| 格式 | `cargo +1.94.0 fmt --all --check` | 本轮范围隔离快照通过，final-fmt.log |
| 静态质量 | `cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings` | 本轮范围快照两次通过，最后为final-clippy.log；第三方现有cfg／弃用警告另保留，不称所有依赖零诊断 |
| 三语文档 | `node docs/tests/check_content.mjs`、`npm --prefix docs run build`、`node docs/tests/check_browser.mjs`、`node docs/tests/check_scope.mjs` | 内容检查／构建通过；/与/docs/、三语桌面／手机12组合通过，两前缀全部三语导航亦通过；docs-content.log、docs-build.log、docs-browser.log、docs-scope.log |

没有修改框架／应用Cargo版本、rust-version或工具链文件；没有新增版本管理器、HTTP响应头或端点。应用配置、APP_VERSION与欢迎页生产实现均未改。本轮无需数据库／缓存矩阵；未执行Windows、Docker镜像、release发布与完整上游PHPUnit，不把这些计作通过。其他协作者的在途文件不属于本轮交付范围；最终源码哈希和隔离消费元数据恢复检查见closure.json。未暂存、提交、推送或发布。

## 本地提交复核（2026-11-23）

用户随后明确授权本轮本地提交，不含推送或发布。提交前以当前 HEAD `067ec254a` 核对版本实现与最终 source-scoped：Application、Console、版本测试与三语配置正文一致；App Facade 仅有额外语言说明注释，该无关改动保留工作区、不纳入本次提交。沿用上述实际验收结果，不将其扩展为最新全部工作区已通过。本次重新执行三语内容检查及暂存差异检查，其他协作者的权限、语言与模板修改继续保留。
