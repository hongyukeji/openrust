# 03-01 本地文件系统与公共磁盘

核对固定 Laravel 13.x 的[配置、本地驱动、公共磁盘和磁盘实例](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/filesystem.md#configuration)、[磁盘管理器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemManager.php)、[适配器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemAdapter.php)、[适配器测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Filesystem/FilesystemAdapterTest.php)、[storage:link](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/StorageLinkCommand.php) 和 [storage:unlink](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/StorageUnlinkCommand.php)。参考固定 [Goravel 文件系统 Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/filesystem/service_provider.go) 的单例装配，服务绑定在当前应用容器。核对旧版 `6d1a8192` 的 `src/filesystem.rs`、`src/config/storage.rs` 及 `tests/server/storage/{driver_contract,http}.rs`，提取按目录句柄逐段打开、可见性模式、暂存后发布和失败不覆盖旧内容的行为。

- 接通 `Storage`、`FilesystemServiceProvider`、`FilesystemManager` 与本地适配器。支持默认、具名及按需磁盘，缓存按应用隔离，`forgetDisk` 后重新解析。读取、写入、流、存在性、删除、路径和可见性可用。默认读写失败回退至 `None` 或 `false`，可配置 `throw` 和 `report`。路径校验失败会直接返回错误。
- Unix 使用目录句柄与已锁定的 rustix 版本，拒绝越界、遍历符号链接和读取多硬链接文件。流写入完成后以同目录重命名发布。源错误或发布前取消保留旧内容，发布操作已发出后取消仍可能完成。Windows 读写采用受限目录和系统 ACL，显式可见性与 Windows 实机验收保留为后续事项。
- `storage:link` 支持 `--relative` 和 `--force`。`storage:unlink` 只处理配置中的有效符号链接，遵循 Laravel 对悬空链接的跳过行为。原有直接挂载仍按 URL 路径和注册顺序工作，Unix 磁盘挂载按文件和目录模式过滤私有内容。
- Rust 用异步读取器、类型化配置及 `Result` 表达 PHP 资源、配置数组和异常。保留严格路径校验及原子发布。磁盘自定义权限映射、通用目录与元数据 API、S3、上传下载与签名临时 URL 仍有后续范围。

## 验收

macOS arm64、Rust 1.94.0。隔离副本位于 `/private/tmp/openrust-03-01.qtGwlV/app-under-test`，以 02-09 候选为底，按本批变更合入共享框架文件。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。374 项通过，其中 5 项为本批新增回归，覆盖命名解析、应用隔离、真实流、失败和取消、Unix 权限与越界、真实 HTTP 和 Artisan。真实 MySQL 未在本批复跑。
- 实际主程序在隔离目录启动。GET 和 HEAD 请求均命中二级公开挂载，私有文件、默认私有磁盘和错误 URL 返回 404。实际 `./artisan storage:link --relative`、`./artisan storage:unlink` 和 `./artisan inspire` 退出码均为 0。子进程回归还验证普通文件碰撞、已有及悬空符号链接、强制重建和自定义目录。
- 三语 Rust 示例逐一编译，在真实应用 Kernel 的请求作用域内写盘及回读。`npm run build`、12 项常规浏览器组合及 6 项文件系统正文和侧栏检查通过。工作区锁定依赖解析与 `git diff --check` 通过。

下一批为 03-03 S3 磁盘，依赖本批的磁盘管理器和适配器契约，并需要隔离的 S3 兼容服务。
