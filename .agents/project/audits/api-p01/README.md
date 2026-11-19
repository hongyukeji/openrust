# API-P01 Precognition

2026-11-20：骨架、实现及本单元实际行为验收完成。V09 仅关闭 Precognition 子项；Fortify、Socialite、OIDC RP／SSO 及最终发布矩阵继续独立跟踪。

## 固定参考与复用

- Laravel Framework 13.40.0：`1449bf241017087fbd38c118ecaabef3f6797fdc`；文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8`。实际下载的 Middleware、Request concern、FormRequest、两个 Precognition dispatcher、验证生命周期、Session 和上游路由测试等 SHA 见 `vendor/openrust/framework/tests/fixtures/precognition/source-manifest.json`。
- 开工先只读核对 `v0.x:vendor/openrust/framework/src/http/precognition.rs` 和对应历史 source-manifest。提取“客户端尝试／服务端确认”及响应协议概念；不复制旧 Axum middleware、共享 AtomicBool 或未经行为验收的旧夹具。新实现接当前原生 Request／Router／Controller／FormRequest 管线。
- PHP 8.5.12，固定镜像 `php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9`，离线容器执行未修改的 `CanBePrecognitive::shouldValidatePrecognitiveAttribute`。28 个选择头 × 15 个属性共 420 个判断；27 个 HTTP 可表示头的 405 个判断由 Rust Validator 逐项核对。NUL 请求头由 HTTP HeaderValue 拒绝，不当作可接收输入。Unicode、非 UTF-8 头、空字段、空格、重复星号、正则特殊字符及属性末尾换行均覆盖。
- `prepare_precognition.py` 从固定引用重新下载并逐文件核 SHA；下载后 oracle 与提交夹具逐字节一致，修改源码的负样本明确拒绝。此 oracle 只验证实际 PHP 字段匹配方法，不冒称已执行全套 PHP HTTP 测试。
- 官方 `laravel-precognition` core 2.0.0，Git `3c7e8892fbd4ab673427d0d0379f761b7484a906`；npm 完整性和固定前端测试锁文件随夹具保留。Chromium 153.0.8010.12／Playwright 1.65.0 实际调用，未把 axios／Vue／React 等其他适配器称为已验收。
- 验收源码是 `6bcace6ee3d99dafc76b4fc6c84322990288de67` 的独立 archive 加本批文件。同期 Passport-03 于 `0fa6759dd` 独立提交，不叠加其 OAuth 改动，本批不替代 V17 最终主线整合。

## 实现和 Rust 适配

- `Request::isAttemptingPrecognition()` 精确判断 `Precognition: true`；`isPrecognitive()` 读取 RequestAttributes 的服务端 bool。未安装中间件时头本身不会抑制动作或 Session 保存。
- `foundation::http::middleware::HandlePrecognitiveRequests` 及内置 `precognitive` 别名，优先于 Cookie／Session 中间件。确认后添加响应 `Precognition: true`，所有响应向 `Vary` 追加 Precognition。保留上游第一行 Vary、重复值、`*` 和 PHP 假值 `0` 的行为；错误／下游提前响应保持原状态。
- Dispatcher 选择保存在当前请求，在最终匹配路由的管线上生效；不修改全局容器绑定，也不绕过全局／路由中间件。ControllerDispatcher 先 await 当前应用控制器工厂，注入式 FormRequest 继续解析、准备、授权、动态规则和验证，然后在动作前以响应异常短路。
- 直接闭包及 View／APIResponse 适配器沿 Handler 的默认预验证分发返回 204；有 FormRequest 参数的适配器覆写分发钩子，保留解析顺序。普通手动 Request 回调不执行函数体；需要执行表单验证时使用类型化注入，不把控制器内手动调用验证当作预分发验证。
- `Precognition::afterValidationHook` 接验证器契约，只有 Validate-Only 头存在且消息为空时抛出成功响应。无该头时先执行 passedValidation，再在分发处停止；空头同样跳过 passedValidation。
- 规则选择按实际展开属性执行，保留原声明路径以免破坏数据库规则的隐含列名／自定义消息。字节 RegexSet 表达固定 PHP PCRE 的无点星号、锚点和末尾 LF；Rust 规则与正则资源上限超限沿既有 ValidationError 返回 413，不进行无界匹配。显式追加到过滤结果后的字段不被先前过滤追溯删除。
- `FormRequest::validationRules(&self, &Request)` 默认委托 rules，允许按 isPrecognitive 选择要求。沿用实际上传文件规则；服务端不会因预验证自动忽略已上传文件或 required。
- StartSession 保留启动、读取、Cookie 和 GC，确认后跳过数据库身份保存准备、上一页 URL 与 save。普通中间件 Session 修改及闪存老化不落库；不声称回滚中间件、构造函数、验证规则、外部系统或显式 Session 操作的副作用。

## 实际验收

证据目录 `/private/tmp/openrust-precognition-20261120/`，主日志 `acceptance.log`。所有正式验收 Cargo 命令显式使用隔离 source 工作目录和专属 target，最低工具链 1.94.0，offline／locked。最终批次 `org.openrust.acceptance.run=1a67398728e60519d9b37cdce62d000f`，实际 MySQL 8.4.13，23 个 Rust 测试函数全过，无忽略：

- 专项 8：字段选择、PHP oracle、204／422／403、授权与异步控制器工厂、passedValidation 时序、闭包／注入式回调／View／APIResponse（回调若执行即 panic）、下游短路和 Vary、并发请求与双应用、真实 MySQL 控制器不写入、Web Session／Cookie／CSRF／闪存／上一页，以及真实浏览器 helper。
- MySQL：三次预验证之后表仍零行，普通提交一行；筛选 `users.0.name`／`users.1.name` 后 exists 继续推断原列名 name，真实查询成功／失败符合预期。
- Web：预验证保留闪存和令牌、丢弃 mutation、不更新 URL；原始尝试头在无中间件路由正常持久化。POST 缺 token 返回 419，合法 token 才返回 204；预验证不是 CSRF 绕过。
- 浏览器 helper：选定 name 成功 204／失败 422，普通提交 200，真实文件 multipart 204、普通字符串文件 422；createValidator 默认省略文件时必填失败，validateFiles 后字段有效。实际动作计数仅普通提交一次。
- 相邻回归 15：middleware_lifecycle 5、middleware_exclusions 1、validation_http 精确 5（含真实 MySQL、FormRequest DI、multipart）及 session_http 精确 4（Array／Cookie／旧输入／flash）。未执行混合其他数据库的全仓测试。
- 实际根 main：HTTP 500→awaited guest authentication→Pail，模板恢复 200、超时／会话清理、SIGINT 退出 0、端口释放和独立 Artisan inspire 全过。
- `cargo +1.94.0 clippy --offline --locked --workspace --all-targets --features pail,passport -- -D warnings` 和 `cargo +1.94.0 fmt --all --check` 通过。Python AST 与 JS 语法检查通过。
- 三语正文、Rust 注入式响应示例与实际 HTTP fixture 同接口。`npm --prefix docs run build` 通过（保留既有 chunk 大小提示）；12 项通用＋6 项既有契约＋6 项 Precognition 浏览器检查通过，覆盖三语、`/` 和 `/docs/`、正文／代码／链接。

测试补充 View／APIResponse 的实际预验证回调抑制断言后，8 项专项再次完整通过，见 `precognition-final.log`；最终严格 Clippy／格式重跑通过后才收尾。

最终主验收命令由 `acceptance.py` 记录，入口：

```sh
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
CARGO_TARGET_DIR=/private/tmp/openrust-precognition-20261120/target \
python3 -B tests/mysql_acceptance.py -- python3 -B ../acceptance.py
```

浏览器专项需要 `OPENRUST_PRECOGNITION_BROWSER_SCRIPT` 指向 `tests/precognition_browser.mjs`、`OPENRUST_PRECOGNITION_BROWSER_ROOT` 指向按 frontend-package／frontend-package-lock 复制为 package.json／package-lock.json 后 npm ci 安装的独立目录，以及 DOCS_BROWSER_PATH。对应 Rust 用例默认 ignore，实际主批次显式 `--include-ignored`，8 项全部执行。

## 范围和收尾

源码／夹具／正文与实际副本逐一 SHA 校对见 [source-sha256.json](source-sha256.json)。本批只修改 Precognition 和所需的路由、FormRequest、Session、规则筛选、测试及三语文档。不暂存同期 Passport、历史 patch、core/ 或 translation-database/。

专属 target／browser／docs dist 已直接清理，df 实际增加 5.607 GB，清理后 Available 68.140 GB（63.461 GiB）；三轮临时 MySQL 标签容器已回收，全部数据卷、共享 PHP 镜像、依赖缓存、源码及日志保留，见 [cleanup.json](cleanup.json)。早期两次 check／test 启动误用主工作区，均立即终止，不计入正式验收；全部通过结果来自 SHA 对齐后的隔离副本。核定 33 文件本地提交，不推送、不改发布版本、不创建标签或 Release。V09 其余子项、V10–18 外部目标与最终集成保持原门禁。
