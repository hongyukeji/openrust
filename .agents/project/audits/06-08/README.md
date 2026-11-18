# 06-08 Artisan 帮助、命令文档与本地化

依赖 00-03 Artisan 入口和 03-10 翻译器。对照固定 [Laravel Artisan](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/artisan.md)、[Symfony HelpCommand](https://github.com/symfony/console/blob/v7.4.0/Command/HelpCommand.php)、[描述器测试](https://github.com/symfony/console/blob/v7.4.0/Tests/Descriptor/JsonDescriptorTest.php) 与 [Goravel Command](https://github.com/goravel/framework/blob/v1.18.0/contracts/console/command.go)。复核旧版 `tests/unit/console/command_help.rs`，沿用两种帮助入口读取同一注册对象且跳过执行的回归。

帮助与列表共用 Clap 命令元数据，支持文本、JSON、XML 和 Markdown。命令名和别名优先精确匹配，支持唯一分段缩写。应用使用 `console` 翻译分组覆盖界面文本，并复用翻译器回退。语言不改变命令、选项和 JSON 键。结构化输出保留 OpenRust 现有 `implemented`、`aliases` 和单个 `usage` 字符串，短输出通过别名字段保留别名。字段组织与 Symfony 描述器有差异。框架接受全局详细程度参数，命令可从输入读取级别。Clap 仍负责解析及英文错误正文。既有跨命令别名冲突拒绝和显式 `--env` 值规则继续有效。

## 验收

- Rust 1.94.0，`cargo fmt --all --check`、锁定离线严格 Clippy 和并行工作区测试通过。490 项通过，3 个子进程入口由集成测试启动。已通过独立提交 `aab1ff4d` 修复调度夹具目录碰撞，并行全量含 Redis 调度验收。
- 实际 Artisan 进程验证英文、简体及繁体 `list` 和 `help`，中文参数错误退出 1，应用语言文件回退、别名及缩写、安静和静默输出、版本和成功退出 0。
- 从三语文档抽取七条公共命令实际执行，解析 JSON 和 XML 并检查 Markdown。三语站点构建、12 项通用浏览器检查及 6 项 Artisan 到本地化小节导航通过。

下一单元 07-01 Helpers 与 Strings，前置 02-08 已提交。

## Artisan 完整纯文本帮助

- Symfony Console v7.4.0 的 `HelpCommand` 将 `--raw` 作为文本描述器的样式选项。详细帮助、参数及选项仍走原命令描述。复核旧版 `tests/unit/console/command_help.rs`，复用两种入口读取注册定义的回归思路。OpenRust 统一渲染 Clap 长帮助，保留应用注册的长简介和参数详细说明。普通文本帮助尊重 ANSI 选择并翻译标题，`--raw` 强制纯文本。JSON、XML、Markdown 延续既有字段与布局，保持纯文本内容。 [固定 Symfony HelpCommand](https://github.com/symfony/console/blob/v7.4.0/Command/HelpCommand.php#L57) · [固定文本描述器](https://github.com/symfony/console/blob/v7.4.0/Descriptor/TextDescriptor.php#L111)
- 隔离快照 `b6cb04500` 的 17 项 `console`、`feature`、`artisan_entry` 回归通过。实际 `./artisan` 验证中英文三语标题、两种帮助入口、ANSI 与纯文本内容等价、结构化输出和未知命令退出 1。注册命令回归带必填参数和别名，验证帮助退出 0 且保留详细说明。
- Rust 1.94.0 全仓格式及全目标严格 Clippy、三语站点构建与根路径和 `/docs/` 的 18 项浏览器检查通过。已回收本批隔离编译和站点产物。项目卷可用 89.04 GB（82.93 GiB），提交前周额度剩余 27%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-artisan-raw-help.tyEkCgMR`。
- 提交前本地 Git 合并了独立短信文档。核对隔离快照与新基线 `49b40cf74` 的差异仅涉及文档，随后在新基线复跑三语构建和 18 项浏览器检查，全部通过。
