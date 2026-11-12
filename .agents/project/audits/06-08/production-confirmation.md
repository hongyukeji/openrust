# 06-08 Artisan 生产确认本地化

对照固定 Laravel [ConfirmableTrait](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/ConfirmableTrait.php)及固定 Goravel [KeyGenerateCommand](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/console/console/key_generate_command.go)。旧版 openrust-v0.x 未找到对应的生产确认提示实现。

生产确认和取消消息复用 06-08 的 `console` 翻译分组。Rust 保留当前单行提示、终端判断和默认取消行为。确认输入为 `y` 或 `yes`。`--force` 可跳过确认。各命令沿用既有退出码和输出流。

## 验收

- 干净归档用 Rust 1.94.0 锁定离线构建。三语实际进程验证 `list`、`help list`、`list --help`、无效参数和结构化命令标识。配置无效 HTTP 端口和不可达 MySQL 后，命令仍可运行。
- macOS 伪终端启动实际 `key:generate`，确认三语提示和取消消息。空输入保留临时应用密钥，输入 `yes` 写入新密钥。使用应用语言文件验证回退覆盖，验证无终端取消及 `--force` 跳过提示。
- Rust 1.94.0 格式、严格 Clippy 和锁定离线全工作区测试通过。主测试进程 592 项通过、7 项忽略，SQLite、MySQL 8.4 和 Redis 8 的独立队列硬超时夹具通过。三语站点构建与 12 项浏览器检查通过，其中 6 项核对 Artisan 到生产确认说明的导航。

[English](../../../../docs/en/digging-deeper/localization.md#artisan-translations) · [简体中文](../../../../docs/zh-Hans/digging-deeper/localization.md#artisan-translations) · [繁體中文](../../../../docs/zh-Hant/digging-deeper/localization.md#artisan-translations)
