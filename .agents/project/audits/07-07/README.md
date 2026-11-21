# 07-07 Package Development

对照固定 [Laravel Package Development](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/packages.md) 和 [ServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/ServiceProvider.php)，采用 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go) 的编译型装配思路。已检查旧版 `6d1a8192` 的 Provider、扩展目录和测试，沿用现有 Provider 生命周期、路由与命令 Facade、视图和翻译解析、迁移记录。

- Cargo 引入扩展 crate，应用显式装配 Provider。`register` 合并配置，`boot` 装配资源。浅合并由应用覆盖第一层同名键，递归替换保留未覆盖的默认键及数组索引。
- 包路由经应用本地 Facade 注册，装配顺序为应用路由、包路由、签名文件路由。延迟 Provider 在路由装配后注册包路由会失败。包命令进入当前 Artisan 内核，使用现有命令注册冲突检查。
- 应用的视图和翻译覆盖包内的同名资源，其余资源回退到包内文件。包迁移与应用迁移共用账本，保留既有排序和文件名冲突校验。Rust 使用编译型注册函数和迁移清单，部署时随应用分发视图与翻译文件。
- 本单元验收范围为独立 crate 的安装、Provider、配置、资源及命令。Artisan 资源发布、自动发现、清单缓存和包资源补充验收见下文。

## 验收

- Rust 1.94.0 锁定离线工作区 520 项通过，4 个忽略项由集成测试作为子进程启动。工作区格式与严格 Clippy 通过。
- 独立 Cargo workspace 编译和严格 Clippy 通过。同进程并行启动两套应用，确认未安装的应用看不到包配置、视图、翻译和命令。
- 独立宿主以真实 HTTP 验证包路由、应用视图覆盖、视图与翻译回退，未安装时包路由返回 404。真实 Artisan 验证命令、说明、SQLite 与 MySQL 8.4 迁移及回滚。包资源从源码目录之外加载。原应用迁移、`inspire`、`GET /` 和 `GET /up` 冒烟通过。
- 三语 VitePress 构建、12 项通用浏览器检查、6 项扩展包页面检查通过。

下一单元 08-01 广播频道与事件分发。

## vendor:publish 筛选、覆盖与路径

- 固定 Laravel 的 Provider 与标签按源路径取交集，重复标签逐个处理，`--all` 优先，`--existing` 限制为已有文件，可与 `--force` 同用。现有 Rust 发布注册、交互提示和迁移重命名继续复用。 [固定 VendorPublishCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/VendorPublishCommand.php) [固定 ServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/ServiceProvider.php)
- 应用相对源路径拒绝上级路径组件。应用内源路径及所有应用目标在处理前检查符号链接，迁移文件名查找也执行此检查。相对目标保留已有词法校验。包可显式登记应用之外的绝对源路径。
- Rust 1.94.0、Apple Silicon macOS。8 项 `test_app` 回归与工作区全目标严格 Clippy 通过。隔离宿主使用实际 Artisan 验证 Provider、标签、默认跳过、强制覆盖、仅更新已有文件及三种符号链接路径。真实 HTTP 读取发布文件、看到覆盖结果，并确认越界资源保持 404。三语 VitePress 构建和 12 项浏览器检查通过。清理独占构建产物、探针和临时目录。
- 首批结束时自动发现与清单缓存仍待验收，后续复用旧版 `tools/cli/src/package_discovery.rs` 和 `tools/cli/templates/bootstrap/package_discovery_build.rs` 的包声明与筛选。

## 包自动发现与清单缓存

- 对照固定 Laravel 的包声明、应用级包名排除及 `*`、包声明之间的排除合并、缺失清单重建，以及 `package:discover` 和 `clear-compiled` 生命周期。 [PackageManifest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/PackageManifest.php) [PackageDiscoverCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/PackageDiscoverCommand.php) [ClearCompiledCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ClearCompiledCommand.php)
- 应用构建时为当前 Cargo 目标和特性生成已链接直接依赖的 Provider 工厂及类型化别名。支持重命名依赖；默认 Provider、发现的 Provider、显式 Provider 依次装配，同类型只注册一次。排除自动发现后仍可显式注册。根应用以 Reverb 验证正式构建和命令入口。
- 默认包清单为 `bootstrap/cache/packages.json`，可通过 `APP_PACKAGES_CACHE` 覆盖。启动按编译清单修复缺失、过期或损坏缓存，`package:discover` 强制重建，`clear-compiled` 幂等移除。应用 Provider 注册仍由编译清单决定，只读部署可启动，写入或清除命令会报告文件错误。当前框架持久化的编译清单为包 JSON。
- 包可注册匿名 JSON 翻译目录、显式 Blade 组件别名和命名空间下的匿名组件。应用翻译与视图仍有覆盖优先级。PHP 类扫描和 Laravel 扩展包各自的优化、重载及诊断钩子由对应运行时或生态单元跟踪。

## 07-07 整单元验收

- 01-02、03-08 和 06-08 已闭合。独立宿主以实际 HTTP 与 Artisan 验证启用、特性关闭、目标不匹配、应用按包名排除、应用全部排除、包间排除和显式注册。部署时清空 `PATH`，仍可处理请求、列出包命令及重建清单。
- 实测默认与自定义缓存路径、强制重建、损坏和过期修复、幂等清除及下次 HTTP 启动重建。实际 HTTP 验证包路由、覆盖和回退、匿名 JSON 翻译、两类 Blade 组件。SQLite 与 MySQL 8.4.13 的实际 Artisan 包迁移及回滚通过，应用间资源隔离通过。
- Rust 1.94.0 下 9 项应用回归、工作区全目标全特性严格 Clippy、独立夹具严格 Clippy、三语 VitePress 构建及 12 项浏览器测试通过。改动文件格式通过；全仓格式仍报告既有 Sanctum 测试排版差异。
- 资源发布、独立 crate 装配、配置、视图与翻译、迁移、命令、自动发现和清单生命周期均已验收。07-07 核心单元闭合。

## 包清单按需导出

- 固定 Laravel 启动时读取 Composer 包清单，OpenRust 从 Cargo 构建时生成并链接的清单装配 Provider。OpenRust 启动时只解析导出路径，`package:discover` 导出 JSON，`clear-compiled` 清除文件。默认路径与 `APP_PACKAGES_CACHE` 保持原契约。
- macOS arm64、Rust 1.94.0，隔离副本构建启用与排除探针包的两个独立二进制。实际 HTTP 和 Artisan 包命令验证 Provider 注册、资源和命令可用、启动无磁盘清单、损坏文件由显式命令重写、默认及自定义路径导出、重复清理和重启后无清单。排除包的二进制在已有导出文件时仍返回路由 404，显式导出为空。此次复验限于包启动和清单命令，沿用前次迁移验收。
- 三语 VitePress 构建、12 项常规浏览器检查及 6 项包文档检查通过，覆盖 `/` 和 `/docs/`。
- 隔离副本的包清单定向 Rust 测试、`cargo fmt --all --check` 和工作区全目标严格 Clippy 通过。独立夹具复用根工作区的本地 SQLx 补丁。

包清单导出与清理共用 `Application::getCachedPackagesPath()`。路径在应用解析环境后可用。 [独立部署验收](../08-04/README.md#独立部署的缓存路径与包清单清理)

## 包清单启动入口定稿（2026-11-21）

用户要求按最终方案稳定应用启动骨架。固定 Laravel Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [PackageManifest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/PackageManifest.php) 分别提供 providers／aliases，支持 dont-discover；应用 Provider 与包清单职责独立。结合 [Cargo 官方代码生成方式](https://doc.rust-lang.org/cargo/reference/build-script-examples.html)，沿用当前 build.rs→OUT_DIR→include! 编译期发现，不为入口排版引入 inventory／linkme 或运行时扫描。`manifest()` 是 Rust 应用适配入口，不声称 Laravel 存在同名 Builder 方法。已重新读取官方原文，保存在本批项目内临时目录。旧版 `6d1a81924:bootstrap/app.rs` 已有 packages 子模块封装生成文件，复用该职责边界；其旧 Provider 清单协议不适用于当前 PackageManifest，不回搬旧 API。

入口固定为 bootstrap/packages.rs 的 pub(super) const fn manifest() -> PackageManifest；bootstrap/app.rs 调用 .withPackages(super::packages::manifest())。私有 compiled 子模块包住生成文件，包发现算法和 withPackages(PackageManifest) 均不改变；bootstrap/providers.rs 仍只列应用 Provider。PackageManifest 已为 Copy，入口只返回静态清单，不访问环境、文件或网络。保留生成的 Rust package_aliases，通过 crate::bootstrap::packages::package_aliases 访问，其内容仍随 Cargo features 与排除规则变化；业务代码优先使用扩展 crate 的公开类型。独立包夹具同步入口，新增实际类型检查，防止封装后遗漏别名导出。三语包开发文档和项目规范同步。

最终验收在项目内隔离源码完成。共享缓存后续被另一工作流占用，本批退出等待并改用 tmp/bootstrap-packages-20261121/target 独占缓存，不复制 target 或修改其他工作流。验收期间工作区将生成文件／常量同步命名为 packages.rs／PACKAGES；保留该改动并同步隔离源码，以下最终结果均重跑于新命名：

- Rust 1.94.0 的 cargo fmt --all --check、全workspace全部targets严格Clippy、独立包夹具全部targets严格Clippy通过；第三方原有deprecated／cfg警告保留，不计为本批新增问题。
- 包夹具类型／清单别名一致性和双应用隔离共2项通过；根application_skeleton两项通过。启用、no-default-features关闭、按包名dont-discover排除三种独立二进制通过真实HTTP／Artisan验收：视图／翻译覆盖、Blade组件、包命令、SQLite专属库迁移回滚、默认及自定义清单路径、损坏文件重建、幂等清除、旧缓存不能恢复已排除包。运行时PATH为空，不依赖Cargo。
- 根应用standalone实际构建通过，复制至独立部署目录后在空PATH执行package:discover及list成功，导出清单为空；实际HTTP /up=200，SIGTERM退出0。没有访问开发数据库或执行开发库迁移；本次没有重跑MySQL／PostgreSQL矩阵，包发现及数据库实现未变。
- 三语站点构建、根路径与/docs/下三语桌面／手机12项通用浏览器检查和12项包开发正文检查通过，正文包含manifest入口和可访问的别名路径，无横向溢出。构建仅既有分块体积提示；git diff --check通过。

验收结束后按最新项目约定，核对无人占用并将本批目录同盘迁入 target/tmp/bootstrap-packages-20261121/，不复制缓存；上文 tmp 路径为验收当时位置。证据与隔离源码现位于该新目录，不提交临时源码及日志。最终源文件已与工作区逐项比对；正常磁盘空间下保留本批缓存复用，全部数据卷保留。核心入口和生成命名已由并行提交4650f403b保存；本轮补充的三语文档、夹具说明及规范／验收记录按用户授权本地提交，未推送。
