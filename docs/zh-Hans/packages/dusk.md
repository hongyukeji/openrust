# 浏览器测试／Dusk

## 支持状态 {#support-status}

**未提供。** OpenRust 没有 Dusk 包、WebDriver 管理器或 `dusk` Artisan 命令。可使用外部浏览器测试工具测试正在运行的应用。

本章保留框架导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及用法见 [HTTP Tests](../testing/http-tests.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

OpenRust 不提供此集成的安装命令。不要因存在此导航入口就将框架Composer 或 Artisan 命令照搬到 OpenRust。
