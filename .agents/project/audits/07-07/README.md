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
