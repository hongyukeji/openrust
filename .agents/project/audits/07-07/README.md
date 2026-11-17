# 07-07 Package Development

对照固定 [Laravel Package Development](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/packages.md) 和 [ServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/ServiceProvider.php)，采用 [Goravel Application](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go) 的编译型装配思路。已检查旧版 `6d1a8192` 的 Provider、扩展目录和测试，沿用现有 Provider 生命周期、路由与命令 Facade、视图和翻译解析、迁移记录。

- Cargo 引入扩展 crate，应用显式装配 Provider。`register` 合并配置，`boot` 装配资源。浅合并由应用覆盖第一层同名键，递归替换保留未覆盖的默认键及数组索引。
- 包路由经应用本地 Facade 注册，装配顺序为应用路由、包路由、签名文件路由。延迟 Provider 在路由装配后注册包路由会失败。包命令进入当前 Artisan 内核，使用现有命令注册冲突检查。
- 应用的视图和翻译覆盖包内的同名资源，其余资源回退到包内文件。包迁移与应用迁移共用账本，保留既有排序和文件名冲突校验。Rust 使用编译型注册函数和迁移清单，部署时随应用分发视图与翻译文件。
- 首批之后的 Artisan 资源发布验收见下文。自动包发现、清单缓存、匿名 JSON 翻译目录及包视图组件命名空间仍待复核。本单元验收范围为独立 crate 的安装、Provider、配置、资源及命令。

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
- 07-07 维持开放。包自动发现与清单缓存下一批可参考旧版 `tools/cli/src/package_discovery.rs` 和 `tools/cli/templates/bootstrap/package_discovery_build.rs`，按当前编译型应用装配复用包声明、禁用清单和目标／特性筛选。
