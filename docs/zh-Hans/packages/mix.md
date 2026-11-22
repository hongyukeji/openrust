# Mix

## 支持状态 {#support-status}

**当前源码未提供。** 尚未提供 Mix／Webpack 集成、mix helper 或 Mix manifest 读取器。应用可使用自己的工具构建资源并从 public 目录提供。

本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [Frontend](../getting-started/frontend.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。
