# ADR-077：Laravel 式 Domain Routing

- 状态：已接受；实现、适用验收与资源收尾完成
- 日期：2026-10-29
- 上游：Laravel v13.39.0 `RouteCollection` / `AbstractRouteCollection` 与 Laravel 13.x Routing 文档

## 背景

Laravel 路由可通过 `Route::domain('{account}.example.com')->group(...)` 将域名约束、URI 参数、middleware 和命名 URL 放在同一组。v13.39.0 的 route collection 对当前 method 先检查 domain routes，再检查 unrestricted routes；调用者无需把 domain route 声明在普通路由之前。当前 OpenRust 仅按 URI path/method 组装 Axum Router，具名 URL 只存 path，没有域参数或 authority 校验。

直接合并两张含相同 path/method 的 Axum Router 会在构建时冲突；依赖响应 status 判断路由是否匹配会把业务 handler 的真实 404/405 错当作 miss，并可能重复执行副作用。域分派必须保留 Axum 所有权下的原始 Request/body，并只针对框架内部 miss 标记继续路由。

## 决策

1. 公开 API 采用 `RouteGroup::domain(pattern)`；嵌套组继承父域，子组显式指定 domain 时覆盖而不拼接。支持完整 DNS 标签和整标签 `{parameter}`，参数独立由 `DomainParams<T>` 提取，不覆盖 Axum `Path<T>`。
2. Domain pattern 在构建时验证；host authority 严格解析并剥离端口、规范化大小写。仅在应用声明域路由时 host 参与分派。重复、多值、畸形或冲突的 Host/URI authority 失败关闭。域参数只捕获一个合法 DNS label，不构成认证或授权。
3. 当前 method 的匹配顺序是按声明顺序检查所有匹配 domain patterns，再检查 unrestricted routes。host 匹配但 path/method miss 时继续下一个候选；业务 handler 的 404/405 是终端响应。所有 path/method 候选均未命中后才选择适用 fallback 或产生 404/405。重叠 pattern 保留声明顺序，不擅自排序 exact/parameter；Axum 默认 `Allow` 合并进最终 405。
4. 路由分派只信任有效请求 authority，不读取 `Forwarded` / `X-Forwarded-Host`。现有 trusted proxy 配置只覆盖 client IP；反向代理 authority 支持须另立契约。绝对 URL 生成仍以 `app.url` 的 scheme/端口为准，domain named route 仅用已登记 domain pattern 与显式参数形成 hostname。
5. Named URL 参数先消费 domain placeholders，再消费 path placeholders，剩余值成为 query。相同名字同时出现在 domain 与 URI path 时启动报错，避免歧义。域参数按 DNS label 验证，禁止路径百分号编码规则替代 authority 校验。
6. 初始 Rust 实现支持 ASCII DNS label（包括标准 `xn--` punycode）与整标签占位符；不做 Unicode IDNA 转换，不支持 IP literal 与末尾点。相同规范化 domain 下 Axum 路由冲突稳定报错，与 Laravel 后注册覆盖存在明确差异；保持缺口记录，不宣传此行为完全等价。
7. 对不匹配继续派发使用私有 miss marker 携带未读取 body 的 Request；不检查通用 404/405 status。每个候选 router 执行前重建入口 URI、headers、version 和外层 extensions，清除上个候选写入的 Axum matched-path/path-parameter 元数据。内部 miss handler 不消费 body、不应用业务组 middleware。
8. 普通路由与 fallback 分别记录声明顺序；fallback 仅在所有普通候选未命中后执行。不同域模式按首次 fallback 声明顺序，同一模式内及普通无域 fallback 按静态路径 scope 深度优先；参数化或通配符前缀包含经外层组传播的域 fallback，均在构建时拒绝。站点根目标仅在业务路由与 fallback 未命中时执行，内部 forward 保留原 URI authority。

## 验收

见 [R3-F Spec](../todo.md#当前-r3-f-speclaravel-domainsubdomain-routing)：domain-first、域与根路由冲突/回退、Host 安全、DomainParams、具名 URL、fallback、真实 TCP 与生成应用行为；实现后的偏差须逐项保留为已知差异。

## 依据

- [Laravel 13 Routing — Subdomain Routing](https://laravel.com/framework/docs/13.x/routing#subdomain-routing)
- [Laravel v13.39.0 RouteCollection](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Routing/RouteCollection.php)
- [Laravel v13.39.0 AbstractRouteCollection](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Routing/AbstractRouteCollection.php)
