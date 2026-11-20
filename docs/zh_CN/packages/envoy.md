# Envoy

## 支持状态 {#support-status}

**当前源码未提供。** 尚未提供 Envoy 任务文件解析器或远程部署命令。原生进程与并发服务可执行应用自己的命令，不是 Envoy 任务运行时。

本章保留 Laravel 导航路径供对照；存在导航入口不代表已经安装相应包，也不承诺对应 PHP API 兼容。

## 可用入口 {#available-entry-points}

原生接口及其已验证范围见 [Processes](../digging-deeper/processes.md)。集成其他库或服务时，应用需实现适配层，并按该服务的契约验证生命周期、错误与凭据处理。

## 范围 {#scope}

缺失集成没有已验证的安装命令。不要因存在此导航入口就将 Laravel Composer 或 Artisan 命令照搬到 OpenRust。未来移植需分别核定公开 API、配置、测试和三语正文。
