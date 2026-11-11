# 04-06 Web 表单回传和错误包

## 契约与实现

- 对照固定 Laravel Handler 的 invalid/invalidJson、ValidationException、FormRequest、RedirectResponse 和旧输入章节。复用旧版 `6d1a8192:vendor/openrust/framework/src/foundation/exceptions.rs` 与 `session/error_bag.rs` 的敏感字段排除、旧输入与错误包回归。 [Laravel 验证](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md#named-error-bags) · [Laravel 旧输入](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/requests.md#old-input) · [Laravel Handler](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Exceptions/Handler.php)
- 已装配 Session 的 Web 请求默认返回 302，在同一 Session 保存命名错误包和过滤后的旧输入。默认包名为 default，支持 `FormRequest::errorBag`、`Validator::validateWithBag` 和 Laravel 的 `_error_bag` 请求覆盖。Blade 按请求共享错误包。自定义异常渲染优先，JSON 422 继续返回字段错误。
- Rust 仅输出站内 Location。显式站内目标、同源 Referer、会话记录的上一页、根路径依次回退，外站 Referer 不会成为跳转目标。旧输入递归移除默认密码字段，`dontFlash` 支持点路径和通配。会话中保存的是 JSON 命名包，Blade 使用属性路径读取。
- 保留差异：上一页按站内路径保存，跨源重定向及代理来源信任不属于此路径。未装配 Session 时验证失败沿用现有错误响应。错误包内容可在 Blade 中遍历，PHP 对象式 ViewErrorBag 方法未移植。

## 验收

隔离基线 `21a65e98` 加已验收依赖与本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 424 项测试通过。
- 实际回环 HTTP 在应用内存和 MySQL 8.4.13 会话驱动均通过，覆盖 URL 编码表单、失败跳转、同源和外站 Referer、显式目标、命名错误包、Blade 显示、旧输入过期、敏感字段排除、并发会话隔离和 JSON/自定义响应不闪存。MySQL 临时库通过 `./artisan migrate --force` 创建表，随后清除临时库及账号。 `./artisan inspire` 退出 0。
- 三语 `dontFlash` 示例编译并运行实际 HTTP。VitePress 构建、12 项通用浏览器组合及 12 项 Validation/Requests 正文导航检查通过。

下一批 04-07 命名限流与 HTTP 节流，依赖 03-06、03-09、04-03。
