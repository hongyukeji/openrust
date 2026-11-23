# Folio

## 支持状态 {#support-status}

**当前源码未提供。** 尚未提供 Folio 文件页面路由器。使用原生路由层显式注册路由并渲染 Blade 视图。

本章保留框架导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [Routing](../the-basics/routing.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将框架Composer 或 Artisan 命令照搬到 OpenRust。
