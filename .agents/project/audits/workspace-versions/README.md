# 统一框架版本管理

2026-11-24。根清单的 `[workspace.package].version` 是框架及随版本发布的官方包的唯一版本入口；当前保留开发版本 `0.0.0`，本轮不实际发版、不建标签、不推送。

## 方案与范围

采用 Cargo 原生 [workspace.package 继承](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-package-table)，20个 `vendor/openrust/*` 包及默认根应用使用 `version.workspace = true`。旧版 openrust-v0.x 根清单和框架包已有相同布局；不搬入其旧依赖或公开契约。根目录原来的具体版本徽章改为指向清单的入口，避免继续手填数字。

三语发布页使用 VitePress 2.0.0-alpha.21 原生[片段区域引用](https://vitepress.dev/guide/markdown#import-code-snippets)，直接显示根清单 `framework-version` region。没有另设 VERSION 文件、自定义 TOML 版本解析器、新依赖或版本同步脚本；文档的master／v0.x菜单继续表示源码分支。原生Markdown回归实际切换两个测试版本并验证三语输出和区域边界。

业务应用可把自身 `[package].version` 改为独立值。`config/app.rs` 的默认值仍来自应用的 `CARGO_PKG_VERSION`，`APP_VERSION` 仍可覆盖；框架 `Application::VERSION`、`App::version()` 及 Artisan版本来自框架包。修改版本后重新构建；使用配置缓存时重新生成缓存，以更新默认应用版本。Socialite HTTP测试的User-Agent和Settings测试包清单改用各自包的Cargo版本，避免发版后仍要求旧数字。

第三方vendor包版本、独立应用／包测试fixture版本、包发现协议的metadata.version=1均保留原语义。`publish = false`、Rust最低版本、版本兼容策略、镜像APP_IMAGE_VERSION设置、历史发布记录不变。本轮没有启用crates.io发布，也没有证明所有官方包都能单独注册表发布；现有path依赖没有全部声明注册表版本约束。源码分发保留workspace元数据；裸复制单个继承成员目录不是独立源码分发方式。

## 实际检查

冻结源为 `082c867beed6fdd83b07e97b94304c44e97d1183`，仅叠加本任务差异，排除并行logging改动；编译与CLI使用项目内独占目标目录和Rust1.94.0。原始日志、独立应用和站点快照放在 `target/tmp/workspace-versions/`，最终检查结果及源指纹见 [results.json](results.json)。

- `tests/workspace_versions.py`先用Cargo完整metadata核对21包默认继承；隔离副本只把根版本改为`999.0.0-workspace-probe`，验证21包同步变化；随后把根业务应用版本改成`9.8.7`，验证20个官方包仍为框架测试版本。脚本最终恢复清单及锁文件，不修改真实项目版本。
- 该临时版本组合下，根应用骨架、原生console／init／资源生成、startup及版本入口共55项Rust测试通过；框架版本和安装器声明识别7项、Socialite真实HTTP7项通过。新增TCP HTTP测试分别在无APP_VERSION和显式应用版本下，验证配置缓存前后共4次HTTP；配置缓存／清除使用真实主二进制Artisan入口。
- Settings Provider另外在临时框架版本下执行1项SQLite真实迁移／配置测试。不是只编译变更后的fixture。
- 原生Cargo打包Blade Compiler叶子包，检查归一化清单具有具体版本及展开后的lints，然后解包到没有workspace.package配置的新workspace，实际执行3项Rust测试。Cargo `package --no-verify`之后的独立构建明确执行，不把归档成功当成编译通过。归档SHA及清单摘要保留。
- 原有独立应用／扩展fixture在自身workspace中分别使用`5.4.3-consumer`和`4.5.6-extension`，其workspace没有统一版本表。原生构建与严格Clippy通过；复用verify.py实测安装资源、HTTP、本地覆盖、包发现、坏清单重建、缓存清除、Artisan及SQLite迁移／回滚。包缓存中的版本必须对应该独立扩展清单。没有将未配置的MySQL路径计为通过。
- 上述Rust测试合计73项，0失败、0忽略；默认版本下workspace all-targets严格Clippy及格式检查通过，独立消费fixture另通过all-targets严格Clippy。
- 三语内容检查、站点完整构建、原生片段回归、原有双挂载点导航浏览器验收通过；三语发布页桌面／移动端共6项浏览器检查通过。保存页面截图，并人工查看英文桌面和简体中文移动端的片段及版式。

完整Rustdoc没有重新生成；已静态核对API导出器从Cargo metadata读取解析后的包版本，而非自行解析成员version字符串。旧Rustdoc导出的源指纹失效时仍须按既有导出流程重建。

## 遇到的问题与处理

1. 最初的验收脚本只使用`metadata --no-deps`，没有刷新临时版本的锁文件，后续`--locked`检查拒绝执行。改为完整offline metadata解析；保留原失败日志，不手工编写锁文件版本。
2. 配置缓存要求应用可执行入口；新增HTTP测试改为调用真正的主二进制Artisan，再由HTTP应用加载同一缓存。不降低缓存断言。
3. 独立包旧fixture存在`TranslationError`多种From转换造成的类型推断歧义；明确转换为HttpError后重新构建、Clippy和协议验收。仅修复测试fixture，不改生产错误行为。
4. 浏览器H1内含装饰性锚点的零宽字符；标题断言去除该装饰并trim后，仍逐语精确比较源码标题。保留首次断言日志。

## 后续发布操作

1. 只修改根Cargo.toml的`[workspace.package].version`。
2. 执行`cargo metadata --format-version 1`刷新锁文件并审查差异；更新当次发布说明。
3. 按项目守卫和隔离快照规则运行版本、安装、HTTP／Artisan、格式／Clippy和文档检查。业务APP_VERSION独立维护，部署重新构建并按需重建配置缓存。
4. 提交已验收差异；标签、远端推送和正式发布按当次授权执行。

验收后清理本批临时源码、依赖安装目录、站点产物及辅助脚本；保留原始日志、截图、归档校验和可复用Cargo／依赖下载缓存。其他协作者的Cargo.lock、依赖和logging在途改动保留。
