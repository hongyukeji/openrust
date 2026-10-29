# ADR-078：只读请求检查与内容协商

- 状态：已接受；C04-A 适用验收通过
- 日期：2026-10-29
- 对应：L019.02；不关闭完整 Request/Response 范围

## 决策

公开 `openrust::http::RequestInfo`，实现 `FromRequestParts<S>`，保存当前请求的只读元信息；与现有 `Json<T>`、`Query<T>`、`Path<T>`、`ClientIp` 组合使用。Rust body 只能被一个提取器拥有，因此不为读取 headers 而缓存或复制 body，不创建伪装成完整 Laravel Request 的空壳。原生 Axum Request 继续作为连接/正文逃生口；动态输入、Session、用户、文件与可变输入语义由后继 C04/R7 完成。

1. `path` 使用 OriginalUri（缺失时使用当前 URI），按 Laravel 去首尾斜线、根为 `/`，不自动百分号解码；`method` 返回 Rust Method，`is_method` 忽略大小写。`uri` 保留原始 URI 的公开只读访问。
2. `host` 复用域路由的实际 authority 校验，缺失、重复、畸形或 Host/URI 冲突为 400，不采信 Forwarded/X-Forwarded-Host。此簇不新增代理信任配置；IP 继续使用 ClientIp，URL 生成继续使用既有登记模板与 app.url。
3. `header(name) -> Result<Option<&str>>` 和 `has_header` 使用首个同名 header；`headers` 提供只读 HeaderMap 以保留二进制头部访问。文本值不是 UTF-8 时稳定返回 `http_request_header_invalid`，不回显值。`bearer_token` 对首个 Authorization 按固定上游提取，不验证凭据，也不替代 Auth；Debug 不输出 header、token、URI/query。
4. 内容协商方法归 RequestInfo：`acceptable_content_types`、`accepts`、`prefers`、`accepts_any_content_type`、`accepts_json/html/markdown`、`wants_json/markdown`、`expects_json`、`is_json`、`ajax`、`pjax`、`format(default)`。内部解析器不建立第二套公共协议接口。
5. 以 Laravel v13.39.0 方法行为及其允许的 Symfony HttpFoundation v8.0.0 解析为固定比较点：质量降序、相同质量保留声明顺序、带引号参数和重复项规范键；不自行丢弃 q=0。accepts/prefers 的匹配方向、空 Accept、通配符、结构化后缀、AJAX/PJAX、JSON 判定和默认 MIME format 表分别按来源对应，不能用“更符合 RFC”悄悄替换上游行为。prefers 支持默认格式别名；空候选返回 None，不 panic。
6. Rust 常驻进程明确限制快照 header 总量 64 KiB/128 个值、Accept 文本 16 KiB/128 项；超限为 431 `http_request_headers_too_large`，无截断或静默放宽。此为资源边界适配，记录与上游无限制 helper 的差异。无新增依赖、全局可变 format 表或新配置。

## 交付与验收

真实模块业务 endpoint 按协商返回 HTML/JSON 并发送 Vary: Accept；请求快照与 Json 同时提取，证明 body 未消费。生成项目公开 consumer、真实 TCP、资源限制/敏感 Debug、非法 authority、q/通配/参数/重复头与格式别名均有行为证据。原统一 JSON envelope 的默认/错误响应策略本簇保持已定 contract，不因增加协商 helper 自动改写全部异常渲染。FormRequest、Cookie/Session、redirect/flash、CSRF、文件验证和其他 L020 行为继续保留后继范围。

## 固定来源

- [Laravel Request v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Request.php)
- [Laravel InteractsWithContentTypes v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Concerns/InteractsWithContentTypes.php)
- [Laravel InteractsWithInput v13.39.0](https://github.com/laravel/framework/blob/v13.39.0/src/Illuminate/Http/Concerns/InteractsWithInput.php)
- [Symfony AcceptHeader v8.0.0](https://github.com/symfony/http-foundation/blob/v8.0.0/AcceptHeader.php)、[AcceptHeaderItem](https://github.com/symfony/http-foundation/blob/v8.0.0/AcceptHeaderItem.php)、[Request](https://github.com/symfony/http-foundation/blob/v8.0.0/Request.php)
