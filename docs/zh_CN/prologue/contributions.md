# 贡献指南

## 报告问题

提供源码提交、操作系统、Rust 版本和最小复现。HTTP 或 Artisan 问题请附实际请求或命令、观察结果及预期结果。涉及数据库或驱动时注明所用环境。

## 提交改动

应用接线放在根应用，框架源码放在 `vendor/openrust/framework/`。遵循固定的 Laravel 基线及项目的 Rust 约定。公开行为有变化时同步英文、简体中文和繁體中文文档。

从仓库根目录运行检查：

```sh
cargo +1.94.0 fmt --all --check
cargo +1.94.0 test --workspace --locked
cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings
```

涉及 HTTP、Artisan 或后端驱动的行为时，用对应的实际入口验收，并在提交说明中记录环境与结果。文档改动见[文档站指南](https://github.com/hongyukeji/openrust/tree/master/docs)。
