# 资源构建／Vite

## 支持状态 {#support-status}

**部分原生支持。** Blade 支持文档所述的 @fonts 消费 version 1 Vite 字体 manifest。尚未提供完整 Vite 集成、JavaScript／CSS 入口 manifest 解析器或前端开发服务器生命周期。

本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [Blade Templates](blade.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。
