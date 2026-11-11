# 04-05 Validator、API 422 与 FormRequest

## 契约与实现

- 对照固定 Laravel 的 Validator、ValidationException、FormRequest 和 ValidationRuleParser，参考固定 Goravel 的工厂与请求上下文传递。复用旧版 `6d1a8192:vendor/openrust/framework/src/validation/{path,runner,tests}.rs` 的路径预算、通配展开、缺失/null/空字符串规则及回归。 [Laravel Validation](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/validation.md) · [Laravel Validator 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Validation/ValidationValidatorTest.php) · [Goravel Validation](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/contracts/validation/validation.go)
- `Validator::make` 支持声明顺序、基础类型和长度规则、`bail`、`sometimes`、`nullable` 及异步自定义规则。应用容器提供工厂和翻译器。验证投影保留嵌套结构，父数组与子规则共用时排除未校验的子字段。JSON 失败由异常 Handler 返回 Laravel 的 `message` 和 `errors`，保留展开后的点路径。
- Rust 通过显式 `validateResolved` 调用保留准备、授权、验证及通过后处理顺序。授权拒绝返回 403。请求继续使用现有输入解析和错误处理。基础规则之外的文件、数据库规则及自动参数注入仍待实现。Web 重定向、旧输入和命名错误包属于 04-06。

## 验收

隔离基线 `21a65e98` 加已验收依赖和本批改动，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过，共 421 项测试通过。
- 真实回环 HTTP 覆盖手动验证、FormRequest 预处理与成功钩子、授权拒绝、嵌套通配 422、null 与缺失、未校验字段排除、中英文并发消息和本地化摘要。异步自定义规则专项覆盖短路、字段标签和自定义消息。
- 三语各两个代码示例编译并分别完成实际 HTTP 200 与 422。 `./artisan inspire` 退出 0。VitePress 构建、12 项通用浏览器组合及 6 项 Validation 正文和导航检查通过。

下一批 04-06 依赖本单元与已验收的 04-04 Session。
