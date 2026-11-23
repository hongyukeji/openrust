# Pint

## 支持状态 {#support-status}

**当前源码未提供。** Rust 源码使用 rustfmt 格式化并通过 Clippy 检查。尚未提供 Pint 可执行文件、PHP 格式化器或 pint Artisan 命令。

本章保留框架导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [Contribution Guide](../prologue/contributions.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将框架Composer 或 Artisan 命令照搬到 OpenRust。
