# Cashier（Stripe）

## 支持状态 {#support-status}

**当前源码未提供。** 尚未提供 Cashier 计费模型、Stripe 订阅生命周期、Webhook 签名处理或支付 UI 集成。

本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [HTTP Client](../digging-deeper/http-client.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。

微信支付与支付宝的一次性收款见[支付扩展](pay.md)。
