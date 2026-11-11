# 06-08 Artisan 帮助、命令文档与本地化

依赖 00-03 Artisan 入口和 03-10 翻译器。对照固定 [Laravel Artisan](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md)、[Symfony HelpCommand](https://github.com/symfony/console/blob/v7.4.0/Command/HelpCommand.php)、[描述器测试](https://github.com/symfony/console/blob/v7.4.0/Tests/Descriptor/JsonDescriptorTest.php) 与 [Goravel Command](https://github.com/goravel/framework/blob/v1.18.0/contracts/console/command.go)。复核旧版 `tests/unit/console/command_help.rs`，沿用两种帮助入口读取同一注册对象且跳过执行的回归。

帮助与列表共用 Clap 命令元数据，支持文本、JSON、XML 和 Markdown。命令名和别名优先精确匹配，支持唯一分段缩写。应用使用 `console` 翻译分组覆盖界面文本，并复用翻译器回退。语言不改变命令、选项和 JSON 键。结构化输出保留 OpenRust 现有 `implemented`、`aliases` 和单个 `usage` 字符串，短输出通过别名字段保留别名。字段组织与 Symfony 描述器有差异。框架接受全局详细程度参数，命令可从输入读取级别。Clap 仍负责解析及英文错误正文。既有跨命令别名冲突拒绝和显式 `--env` 值规则继续有效。

## 验收

- Rust 1.94.0，`cargo fmt --all --check`、锁定离线严格 Clippy 和并行工作区测试通过。490 项通过，3 个子进程入口由集成测试启动。已通过独立提交 `aab1ff4d` 修复调度夹具目录碰撞，并行全量含 Redis 调度验收。
- 实际 Artisan 进程验证英文、简体及繁体 `list` 和 `help`，中文参数错误退出 1，应用语言文件回退、别名及缩写、安静和静默输出、版本和成功退出 0。
- 从三语文档抽取七条公共命令实际执行，解析 JSON 和 XML 并检查 Markdown。三语站点构建、12 项通用浏览器检查及 6 项 Artisan 到本地化小节导航通过。

下一单元 07-01 Helpers 与 Strings，前置 02-08 已提交。
