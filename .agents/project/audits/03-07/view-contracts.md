# 03-07／03-08 视图、工厂与引擎契约

## 基线

对照固定 Laravel 的 [Views](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/views.md)、[View](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/View/View.php)、[Factory](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/View/Factory.php)、[Engine](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/View/Engine.php) 及 [ViewFactoryTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/View/ViewFactoryTest.php)。参考固定 [Goravel View](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/view/view.go) 的应用级共享数据和视图目录。复核旧版 v0.x（`6d1a8192`）的 HTTP 视图、模板转义及目录覆盖用例。实现沿用主线既有查找、编译和渲染路径。

## 实现与 Rust 适配

- 视图保留所属工厂，可在修改数据后再次渲染。工厂从当前应用容器或 View 门面取得，契约和具体工厂共享同一套创建器、Composer、命名空间和共享数据。视图数据覆盖合并数据。
- 文件视图使用现有路径约束。相对路径按应用目录顺序查找，绝对路径可落在已配置的应用目录或命名空间目录。支持 `.blade.html`、`.html`、`.css`。Engine 的根路径参数显式界定文件访问范围。
- 直接渲染采用同步 Rust 契约，HTTP 返回视图仍由原处理路径调度阻塞渲染。注册回调使用类型化闭包，命名空间注册用 Result 传播参数错误。现有具体工厂调用保持兼容。

## 验收

- 实际 TCP HTTP 验证两个应用共用模板并发渲染时，Composer 和共享数据各自隔离，模板输出保持转义。文件、命名空间替换、重复渲染、合并数据优先级、未知扩展、路径及符号链接越界均由定向用例覆盖。
- 三语示例与实际 HTTP 路由的渲染函数逐项核对。VitePress 构建与 18 项 Chrome 检查覆盖 `/` 和 `/docs/`。

[English](../../../../docs/en/the-basics/views.md#view-contracts) · [简体中文](../../../../docs/zh-Hans/the-basics/views.md#view-contracts) · [繁體中文](../../../../docs/zh-Hant/the-basics/views.md#view-contracts)

Rust 1.94.0 格式、全目标严格 Clippy 和锁定离线串行工作区测试通过。测试日志合计 580 项通过、6 项忽略，含独立进程夹具。本批启用 Redis 回归，MySQL 可选用例未开启。
