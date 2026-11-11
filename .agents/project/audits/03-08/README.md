# 03-08 布局、组件、视图组合器

## 契约与实现

- 对照固定 Laravel 的 ManagesLayouts、ManagesComponents、ManagesEvents、Factory 和 Blade 组件编译入口。Goravel 的应用级视图服务保存共享数据。复用旧版请求级视图数据与显式覆盖共享默认值的合并规则。 [Laravel Views](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/views.md#view-composers) · [Laravel Blade](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/blade.md#building-layouts) · [Goravel View](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/view/view.go)
- 支持布局继承、区段、默认内容、`@parent` 和继承局部数据的子视图。继承链共享子区段，渲染树退出时随状态一并释放，失败后不会留在应用容器中。嵌套渲染限制深度。
- 支持匿名组件、显式注册的 Rust 类组件、静态与绑定属性、默认及命名插槽。已渲染插槽以 HTML 片段传递，其中的变量照常转义。应用容器保存组合器、创建器和共享值，回调对每个视图实例操作。渲染保留当前应用的门面作用域。
- Rust 使用闭包和 Component trait 注册类组件。剩余 Blade 属性包、`@props`、动态组件、PHP 表达式及其他指令仍待覆盖。Windows 实机未验收。

## 验收

隔离副本以 `21a65e98` 与此前已验收的依赖为基线，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，测试报告 392 项通过。未为本批启用真实数据库及外部存储。
- 真正的回环 TCP 发送 5 次 HTTP 请求，覆盖两个独立应用并发渲染、两级继承、默认与命名插槽、类组件、组合器与创建器时机、缺失父布局 500，以及失败后普通布局的区段回退。
- 三语文档的 Rust 注册代码与 Blade 模板编译并各通过一次真实 HTTP。实际运行 `./artisan view:cache`、`./artisan view:clear` 和 `./artisan inspire`，退出码均为 0。
- 文档构建通过，根路径与 `/docs/` 子路径通过 12 项通用浏览器检查和 6 项组合器、布局、组件专项检查。

下一批为 03-09 Redis，配置和缓存前置分别为 01-01 与 03-06。
