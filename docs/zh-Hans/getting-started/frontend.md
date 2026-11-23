# 前端

## 支持状态 {#support-status}

**部分原生支持。** 已有服务端 Blade 视图、公共文件与 JSON 响应。当前源码没有 Vue／React 应用脚手架、Inertia 适配器或前端包管理工作流。

本章保留框架导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [Views](../the-basics/views.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将框架Composer 或 Artisan 命令照搬到 OpenRust。
