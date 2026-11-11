# 01-04 日志轮转与自定义渠道

对照固定 Laravel 13.x 的 [日与月文件渠道](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#configuring-the-single-daily-and-monthly-channels)、[自定义工厂](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#creating-custom-channels-via-factories)、[临时渠道](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/logging.md#on-demand-channels)和[渠道测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Log/LogManagerTest.php)。核对 [LogManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/LogManager.php)及 [Monolog 轮转处理器](https://github.com/Seldaek/monolog/blob/3.10.0/src/Monolog/Handler/RotatingFileHandler.php)。参考固定 [Goravel 应用级渠道与日志栈](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/log/application.go)。

- 旧版 `6d1a8192` 的 `src/logging/manager.rs` 和 `tests/unit/logging/manager.rs` 提供具名驱动、失败重试、目录摘除及防止旧工厂覆盖缓存的回归。该版日志写入端只有单文件、标准错误、空输出和组合，日与月轮转按固定上游实现。
- `daily` 和 `monthly` 共享已验收的同步文件写入、权限、锁和级别过滤。使用记录的 UTC 时间选择文件，支持日渠道旧 `days` 配置，`max_files` 优先，零表示不限数量。清理精确匹配有效日期的同名普通文件。保留当前活动文件，并处理并发时晚到的上一周期记录。
- `Log::build` 构造临时渠道，`stackChannels` 组合命名渠道和已解析对象。Rust 用带类型的混合成员表达 PHP 数组。临时构造结果由调用方持有。标准文件处理器与自定义日志契约接收同一消息及上下文。
- `extend` 注册具名驱动，`registerFactory` 注册 `custom` 的 `via`。Rust 使用编译闭包和日志契约表达 PHP 容器解析工厂及 Monolog 对象。工厂在目录锁外执行，首次并发查找允许重复构造，命名渠道共享最终发布结果。失效前已开始的解析可返回原对象，不能重新写回缓存。工厂错误走脱敏紧急路径。Monolog 专用 formatter、processor、tap 及 handler 仍需各自的 Rust 对应实现。

## 验收

macOS arm64，Rust 1.94.0。隔离副本基于 `21a65e98` 和已验收的 01-02y、01-02z、01-03，加入本批文件。工作区其它改动保持原样。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。318 项工作区测试，新增 4 项覆盖真实落盘、确定性周期切换、不同命名格式隔离、工厂重试、混合栈及工厂与缓存失效并发。
- 三次真实回环 HTTP 请求验证两个应用的日文件隔离。Artisan 子进程执行 `log:probe`，由自定义工厂构造月渠道并落盘。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语各两段示例编译执行并检查落盘。VitePress 构建通过，根路径与子路径的 12 个浏览器组合验证导航和轮转正文。

下一批 01-05 接请求及任务 Context，依赖已验收的日志记录路径和容器生命周期。公开与隐藏上下文、作用域退出清理以及与日志合并需要实际并发入口验收。
