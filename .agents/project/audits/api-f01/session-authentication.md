# Fortify Session 登录与密码确认

## 范围

本批在 `aab3e52c1` 的独立 archive 上实现原生 Session Guard 登录／登出、认证流水线、限流、密码确认及状态接口，提供显式 `FortifyServiceProvider`。**API-F01 整包仍未闭合**：注册、重置密码、邮件验证、账户双因素管理／挑战、安装命令及三语完整正文继续开发；Passkeys 属于 API-F02。Provider 在加载路由前拒绝启用尚未接通的功能，避免只注册部分接口而成功启动。保留上游完整默认配置；当前仅 `features: []` 或 `ignoreRoutes()` 可完成启动，没有自动发现 metadata。

固定参考沿用 [首批记录](README.md#固定参考)。额外核对 `v0.x` 的 `auth/fortify/actions.rs`、认证 API／HTTP 及计划；旧版 authority／令牌机制不能替代当前 Laravel Session Guard，未直接移植。测试均放在 crate 的 `tests/`。

## 已实现行为

- 配置、回调、服务和路由归属于应用容器。默认 `email`，可修改 username／guard／路径／前缀／重定向；views=false 不注册 GET 视图，保留同 URI 的 POST 后 GET 返回原生 405。
- LoginRequest 走原生 FormRequest 验证；Pipeline 优先级为 `authenticateThrough` → 配置数组 → 默认阶段。保留空值过滤、顺序、短路；配置中的未知阶段在运行任一阶段前失败。
- 内置登录使用原生 SessionGuard.attempt，保持用户提供器、密码校验、rehash、事件和记住登录。成功后 regenerate(false)，清除失败次数；登出后 invalidate／regenerateToken，撤销记住登录 token。
- 默认五次失败后 429 JSON／302 Web 错误包，键为用户名小写／原生 transliterate 与 IP；使用原生 RateLimiter。命名 limiter 通过原生 throttle 中间件替换默认阶段。
- 自定义认证失败继续派发原生 Failed，并借用本次 Credentials；Lockout 借用 RequestParts。扩展复用 `EventContext` 的借用参数入口，保留事件对象地址检查，不复制密码到事件快照。
- 密码确认提供 `ConfirmPassword` 动作、应用回调、可替换成功／失败响应、Session 时间戳与确认状态。原生提供器的密码参数不接受 null，默认分支拒绝 null，回调仍收到原始 Option。
- 确认状态使用固定 PHP 的 int < 混合请求值比较；支持数字字符串、浮点数、布尔值、null、数组和关联数组。保留 X-Retry-After 的已过去秒数，以及 0 被 array_filter 丢弃的行为。受信 Session 时间戳限定原生 i64；相减溢出显式失败，不模拟损坏 Session 的 PHP 算术强转。
- LoginResponse／LogoutResponse／PasswordConfirmedResponse／FailedPasswordConfirmationResponse／LockoutResponse 及两种 ViewResponse 可由容器替换。`loginView`／`confirmPasswordView` 支持原生 Blade 名称或异步响应回调，Blade 使用原生请求／Session 的 CSRF 和 guest 上下文。
- 框架补齐参数化 `guest:web` 别名，以及 ValidationException 的可选择 JSON 状态；默认 422、Web 重定向保持原有行为。

## 编译型适配与当前边界

Rust 的原生 Responsable 消耗 self，不能作为 dyn 容器服务；Fortify 另提供借用式、对象安全的 `contracts::Response`。PHP 任意可调用值改为强类型异步回调；view 回调直接返回原生 Response，使用原生 View API 时自动保留请求上下文。PHP 的 `['request' => $request]` 任意对象反射不在 Blade 编译型值域中。

配置流水线接受已编译内置阶段短名、`OpenRust\Fortify\Actions\…` 或 `openrust_fortify::actions::…` 完整路径；任意 PHP 类反射改由 `authenticateThrough` 注册强类型 native PipelinePipe。其他命名空间的同名类不被误识别为内置阶段。2FA 阶段暂时明确失败，Provider 同时拒绝启用对应功能；后续实际接通后移除拒绝。

当前配置字符串使用 Rust 字符串域，未完成 PHP 对任意错误配置类型的全面对照。Passkeys 环境变量解析及完整安装默认行为留待后续单元，不宣称已经通过。

## 验收证据

独立目录 `/private/tmp/openrust-fortify-auth-20261120/`，Cargo target 固定在该目录内；不叠加协作者 Passport-04／05 在途代码，不替代 V17 最终主线整合。

- 固定 PHP 8.5.12，执行未修改的 `ConfirmedPasswordStatusController.php`；279 组 int／混合 seconds 对照逐项一致。harness 仅替换时钟、Request 和 response 边界，不宣称执行完整 PHP HTTP 栈。源码 SHA 验证、重新生成逐字节一致；JSON SHA256 为 `ffa57098fc8fe0c2f65ed7183e1adedc6eb88ada59fa9a11e9629e865497610f`。
- Fortify 首轮全量 25 个函数通过；最终新增流水线完整名称用例后，HTTP 14 个函数全部通过，共计 26 个不同函数。包含原有 Provider／Features 回归、状态对照、Provider 配置及实际 TCP HTTP。
- 实际 MySQL **8.4.13**，Database／Eloquent 两种 UserProvider、数据库 Session、字符串主键 `001`、Session／CSRF 轮换、记住登录 Cookie 恢复与登出后的旧 Cookie 撤销、密码确认、旧会话行删除均通过。使用 `tests/mysql_acceptance.py`，不以缺失连接而跳过的用例计通过，不运行 SQLite。
- 原生 12 项回归：Session Authentication 六项（含 MySQL、原生认证生命周期与 Lockout）；MySQL Session Web Validation 两项；FormRequest 验证／注入两项；原生事件借用／MySQL 投递两项。均通过。
- Rust 1.94.0 `fmt --all --check`；完整 workspace／all-targets、features `pail,passport,fortify`、`-D warnings` Clippy 通过。
- 实际根 main HTTP 异常→Pail，恢复 200，Pail 超时清理、SIGINT 退出 0／端口释放及独立 Artisan inspire 均通过。最终日志 `last-check.log`，原生回归 `regressions.log`，整包 `final-fortify.log`。
- 未修改三语产品文档，本批不重复无影响站点构建。API-F01 整体验收时补齐三语正文／构建／浏览器。

```sh
python3 -B .agents/project/audits/api-f01/fixtures/prepare_password_status.py \
  /private/tmp/openrust-fortify-20261120/upstream /private/tmp/password-status.json
CARGO_INCREMENTAL=0 python3 -B tests/mysql_acceptance.py -- \
  cargo +1.94.0 test --locked -p openrust-fortify --tests
```

上述正式 Cargo 命令在独立快照工作目录执行，并显式设置该快照的 CARGO_TARGET_DIR。源码一致性见 [source-auth-sha256.json](source-auth-sha256.json)；资源清理见 [cleanup-session.json](cleanup-session.json)。最初 `guest:web` 缺失、GET 405／Request.path 夹具预期、PHP stdClass 与 JSON 关联数组的夹具错误及 Provider 底层错误链断言均保留早期日志，修正后重新验收。

40 份源码／Cargo／对照输入与验收快照 SHA 一致。专属闲置 target 已直接删除（逻辑 5.682 GB，df 实际增加 5.484 GB），清理后 Available 66.612 GB（62.037 GiB），数据卷清单前后均为空；共享缓存、PHP 镜像、源码和日志保留。三个临时 MySQL 批次已按专属标签回收。
