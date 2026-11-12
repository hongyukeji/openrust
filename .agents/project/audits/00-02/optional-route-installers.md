# 00-02 补交 API 与广播路由安装器

- 复核固定 Laravel 13.x 的 [API 安装器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ApiInstallCommand.php) 和 [广播安装器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/BroadcastingInstallCommand.php)。旧版 `6d1a8192` 未发现同名安装器。
- 复用 08-03 已验收的编译型安装流程。OpenRust 修改应用源码和静态装配，随后重新构建。API 认证、频道规则和广播驱动由应用配置。Laravel 的 PHP 依赖安装与交互流程未映射到本批安装器。
- 从 `fa3d54c6` 叠加核定文件。Rust 1.94.0 锁定离线工作区 538 项、格式及严格 Clippy 通过。独立 Artisan 进程验证并发安装、失败时保留自定义接线、重复安装保留用户编辑和无重复声明。
- 使用生成模块重新构建并启动 HTTP。默认 `/api/probe` 和自定义 `/v1/internal/probe` 返回 204，未挂载路径返回 404。使用独立内存会话和测试密钥，`POST /broadcasting/auth` 对无规则请求返回 403。实际 `artisan inspire` 成功。
- 执行三语正文的安装命令。VitePress 构建、12 项通用浏览器检查和 6 项 Artisan 页面定向检查通过。
