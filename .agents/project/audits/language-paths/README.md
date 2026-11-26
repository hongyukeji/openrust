# 应用级语言路径与发布接线

2026-11-23。用户经侧对话授权主对话补齐语言路径公开契约，保留18语资源扩展在途改动。本批不暂存、不提交、不推送；不是18语整项或全翻译模块的完成结论。

## 固定契约与调研

对照固定 Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php)、[Contract](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Foundation/Application.php)、[路径助手](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/helpers.php)及 [join_paths](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/functions.php)。上游在应用初始化时选择既有 resources/lang，否则根 lang，不强制迁移旧应用。langPath 返回路径，useLangPath 更新应用路径及 path.lang 绑定；已经解析的 Loader 不因后来修改路径而重新构造。

检查旧 `openrust-v0.x/vendor/openrust-framework/src/translation/file_loader.rs`：旧 Loader 已有初始化时一次选择新旧目录的规则，未发现对应公开 Application 路径覆盖入口。本批复用现行 private language_path 选择器、Shared Paths、容器和 Loader，没有引入另一套语言管理器、环境变量或复制旧 Loader。

| 上游入口或消费者 | OpenRust 入口与结果 |
| --- | --- |
| Foundation Application::langPath | foundation::Application::langPath(&str) -> PathBuf；空后缀返回目录 |
| Application::useLangPath | useLangPath(Into<PathBuf>) -> Result<&Self>；复用现有路径 setter 的绑定错误形式 |
| Contracts Foundation Application::langPath | contracts::foundation::Application::langPath；实际 Application 覆写读取同一 Paths |
| App Facade 的路径方法 | support::facades::App::langPath／useLangPath；后者返回 Arc<Application> 支持继续读取应用 |
| 全局 lang_path | support::lang_path(&str) -> Result<PathBuf>；使用当前作用域，没有应用时沿用 FacadeNotSet 错误 |
| path.lang | 容器中的 Arc<PathBuf>，应用初始化即注册，覆盖后同步更新 |
| TranslationServiceProvider | Loader 首次解析时读取同一 Paths；应用自有分组、Lang、__ 与包覆盖一致 |
| lang:publish | 从 app.langPath("") 取得目标；保留集合维护者的模板、许可及覆盖保护修改 |
| 包 Provider / vendor:publish | publishesPaths 现接受明确指定的绝对目标，包括自定义项目外目录；保留相对遍历和符号链接拒绝 |

Rust 没有 PHP 默认参数，入口沿用既有 storagePath 的显式空字符串后缀；容器解析错误用 Result。目录是应用级配置，应在装配阶段、首次解析翻译服务之前设定，不是逐请求语言状态。仅改变绑定，不创建或搬移现有语言文件。数据库的应用级翻译仍解析容器 translator；独立 DatabaseManager 无应用容器时保留根目录选择器回退，不引入全局路径。

## 新发现并闭合的关联缺口

首轮测试误用了 private add_package_namespace，已改为真实 ServiceProvider::loadTranslationsFrom，不扩大内部可见性。公开 Provider 消费者随后发现 publishesPaths 拒绝 app.langPath 返回的绝对目标，导致 Provider 启动失败。已直接修复发布登记和写入路径检查，不用 strip_prefix 等测试绕行隐藏缺口。绝对目标只扩展本地文件／目录映射；嵌入资源 publishes 的应用内相对目标规则保持原样，避免改变嵌入资源来源标识。写入前仍逐层拒绝 symlink、拒绝 ParentDir 等越界片段，原负例通过。

初次严格 Clippy 指出布尔表达式可简化，已修复并重新通过。首次私有调用编译失败、Provider 启动失败及 Clippy 日志均保留，不能作为最终通过证据。

## 资源与骨架边界裁决

官方现代骨架默认不含 lang；本项目三语自有资源及后续18语正式交付是已确认的 OpenRust 扩展，由对应资源扩展任务维护。不据官方默认骨架删除现有 lang 文件，也不移动其他协作者正在维护的模板。无语言目录的新应用只获取路径不会创建目录，显式 lang:publish 才发布模板；应用自有词条仍由应用维护。18语资源正确性、欢迎页和全消费者矩阵以其负责人的验收记录为准；技术验收与母语审校分别跟踪，本批不重复更新其完成统计。

## 实际验收

源码快照来自记录于 `target/tmp/language-paths-20261123/baseline.txt` 的 HEAD。先用提交版发布集合加路径改动验收，再覆盖总控的18语 CATALOGS／stubs 做交集验收。所有命令经 build_guard，Rust1.94.0；独占缓存复用本代理前一批已退出且无占用的 `target/tmp/model-fields-sync-20261123/cargo-target`，没有复制整份缓存。源码／日志／子进程 TMPDIR 均在 target/tmp 内。

- `cargo test --manifest-path target/tmp/language-paths-20261123/source/Cargo.toml --locked -p openrust-framework --test language_paths`：3项通过，覆盖根 lang、旧 resources/lang 的真实命令发布、Contract、绑定、App/helper/Lang/__、项目内外自定义路径、Provider 包覆盖及 vendor:publish、重复发布、force、existing。详见 `tests-complete.log`。
- `cargo test --manifest-path ... --locked -p openrust-framework --test test_app publish`：5项发布回归通过，包含目录、迁移命名、来源筛选、事件及符号链接负例。详见 `publish-complete.log`。此前完整 test_app 尝试12项通过、1项独立部署夹具按原标记忽略；没有将忽略项计为通过。
- `cargo clippy --manifest-path ... --locked -p openrust-framework --all-targets -- -D warnings`：通过，详见 `clippy-complete.log`。已核对最终工作区与验证快照的11份核心文件 SHA，见 `final-source-sha256.json`。
- 三语 localization 及 packages 正文同步方法、生命周期与绝对映射规则；VitePress 的命令入口／正文检查及构建通过，`/` 和 `/docs/` 桌面／手机12项浏览器检查通过。详见 `docs-build-final.log` 与 `source/language-paths-browser-final.log`；正文 SHA 见 `docs-sha256.json`。

未进行本批 Windows／Linux 原生路径验收、完整 workspace／all-features、数据库版本矩阵或真实生产部署，不把已有其他任务的结果移作本批证据。语言路径用例仅使用项目内临时文件，不连接开发数据库、不涉及供应商账户或数据卷。

未触及32GB停线，按新规则保留可复用缓存和日志。本批没有执行删除构建缓存或数据卷的操作；总控另行清理的空间不计为本批释放。最终空间以收尾 df 为准。

旧版参考补记：已核对 `tests/unit/translation/file_loader_paths.rs` 的 existing-directory／不合并与构造时固定路径用例；只复用规则，未迁入旧类型或旧快照机制。旧版 HEAD 与源文件 SHA 保存在 `old-v0x-reference.json`。最终工作区与验证快照11份核心文件一致，六份三语正文 SHA 也一致。本轮收尾复查可用约 107.449 GB，高于32GB停线；保留热缓存和日志，未暂存、未提交、未推送。

## 本地提交授权补记

用户追加“提交代码”，仅提交语言路径、消费者测试、对应三语说明与本记录。共享 lang_publish_command.rs 的已提交版本保留原四组英文模板，仅替换路径读取并保留 en 后缀；TranslationServiceProvider 保留既有 FileLoader::new，不夹带 with_defaults 和18语内置资源。三语本地化正文仅提交本批新增语言目录章节，当前交接仅提交本批条目。此前“不提交”描述保留为当时状态，不延伸为最新授权。其他模型字段、18语资源、配置及许可改动留在工作区，不推送。

分块提交组合补验：使用实际索引中的 Rust 文件覆盖独立快照，3项路径消费者、5项发布回归和框架 all-targets 严格Clippy再次通过；日志 `commit-tests.log`、`commit-publish.log`、`commit-clippy.log`。该组合保留 HEAD 的四组英文模板、FileLoader::new，没有依赖未提交的 bundled 模块或语言资源。
