# 入门套件

## 支持状态 {#support-status}

**当前源码未提供。** 尚未提供现成套件 UI、控制台页面或 React／Vue／Livewire 套件。原生 init 向导与 Fortify 提供应用装配和无界面认证，页面由应用自行实现。

本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及其已验证范围见 [Fortify](../packages/fortify.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

缺失集成没有已验证的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。未来移植需分别核定公开 API、配置、测试和三语正文。
