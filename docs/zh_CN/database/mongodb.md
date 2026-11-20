# MongoDB

## 支持状态 {#support-status}

**当前源码未提供。** 当前数据库层没有 MongoDB 驱动、BSON 查询构造器或 MongoDB Eloquent 适配器。SQL Eloquent API 不代表支持 MongoDB。

本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [Database](getting-started.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。
