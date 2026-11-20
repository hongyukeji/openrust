# Horizon

## 支持状态 {#support-status}

**当前源码未提供。** 已有原生队列、Worker 与队列监控命令。尚未提供 Horizon 面板、负载均衡监督进程或 Horizon 专属任务元数据。

本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及其已验证范围见 [Queues](../digging-deeper/queues.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

缺失集成没有已验证的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。未来移植需分别核定公开 API、配置、测试和三语正文。
