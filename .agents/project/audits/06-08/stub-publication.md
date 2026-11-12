# 06-08 Artisan 模板发布

- 对照固定 Laravel 的 [Stub Customization](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md#stub-customization)、[StubPublishCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/StubPublishCommand.php) 和 [PublishingStubs](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Events/PublishingStubs.php)。固定 [Goravel MakeCommand](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/console/console/make_command.go) 提供编译型模板参考。复核旧版 `v0.x`（`6d1a8192`）的模板、命令和发布路径，未发现同名命令。
- 默认发布缺失模板并保留应用文件。`--existing` 只覆盖已有模板，两选项同传时该范围仍有效。`--force` 单独使用覆盖现有并补齐缺失模板。共用生成器模板清单提供十份可实际消费的模板。普通迁移、创建表和更新表分别读取各自的覆盖文件，保留原先的表推断与生成逻辑。
- Rust 内置模板随框架编译，覆盖使用 UTF-8 源文件。发布前派发类型化 `PublishingStubs`，同步监听器可用 `add`、`insert` 和 `remove` 调整本次发布。目标限定为模板目录内的文件名。Laravel 清单中尚无对应 Rust 生成器的模板随各生成器交付。
- 基于 `023fa9f0` 的隔离候选。Rust 1.94.0 锁定离线工作区串行 566 项通过，5 项既有夹具忽略。格式及全目标严格 Clippy 通过。真实 Artisan 覆盖失败后重试、并发发布、保留、两种覆盖模式和应用目录隔离，修改后逐一确认十份模板均被生成器读取。另用应用监听器验证添加源文件、插入编译型模板、移除默认项和拒绝越界名称。
- 三语文档九条发布命令均通过源码 `./artisan` 实际执行，覆盖模板再生成。VitePress 构建、12 项通用浏览器检查和 6 项定向检查通过。Seeder 模板接线与主工作区既有写入函数改动分块，保留原改动，合并版本的实际生成回归通过。
- 补交 Seeder 写入重构。以 `81eb516d` 为基线，`make:seeder` 复用既有生成器的文件名转换及写入逻辑，沿用现有命令参数和模板。macOS、Rust 1.99.1 下，`cargo test --locked --offline --test seeding` 返回 5 项通过。实际 Artisan 覆盖命名空间生成、拒绝覆盖、强制替换和拒绝越界名称，SQLite Seeder 回归通过。两个 MySQL 用例因未配置测试连接而未执行数据库操作。改动文件的 rustfmt 检查与差异空白检查通过。
