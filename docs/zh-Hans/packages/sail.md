# Sail

## 支持状态 {#support-status}

**当前源码未提供。** 仓库提供 Docker／Compose 部署文件，没有 Sail 包装器或 sail 命令。请按实际容器装配与持久化存储说明使用。

本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [Deployment](../getting-started/deployment.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。
