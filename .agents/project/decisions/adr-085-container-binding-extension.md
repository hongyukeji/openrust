# ADR-085：统一 Container 的 Binding extend

- 状态：Accepted（总控冻结 contract）；本任务分支已实现并完成下述定向验证，尚未集成 main 或完成全矩阵验收
- 日期：2026-10-30
- 任务：阶段一 R2 / L012.02 的通用 Binding extend 子簇；不代表完整 Container 矩阵关闭
- 前置：[ADR-070](adr-070-laravel-first-rebuild.md)、[ADR-074](adr-074-laravel-modules-directory.md)、已提交 R2-A/B/C/M/N
- 基线：`1838012400742f526e5ace8bec355fdcaa699587`

## 背景与来源

Session 的第三方 driver Provider 应能装饰任意上游 factory 返回的 `Arc<dyn Trait>`，保留其配置、初始化、副作用与清理登记。基线 `1838012` 的 `ContainerBuilder::bind/singleton/scoped/instance` 只有同类型后注册覆盖，`RegisterContext` 没有 extend；已知 Db/Memory 枚举重建不能代替通用装饰。本分支已补通用装饰机制，并以生产 Session Provider 的进程内 HTTP 路径验证；真实 TCP 与数据库服务仍待验收。

固定 Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`：

- [Container.php](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php)：`extend`、`resolve`、`getExtenders`、`forgetExtenders`、`dropStaleInstances`、`rebound`。
- [ContainerExtendTest.php](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Container/ContainerExtendTest.php)：惰性、先 extend 后 bind、显式 instance、多次顺序、rebinding、alias、contextual。
- [ContainerTest.php](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Container/ContainerTest.php)：共享、scoped reset 与 rebound listeners。

上游两条路径必须区分：存在缓存实例时立即变换当前值，调用 rebound，并且**不追加持久 extenders**；没有实例时追加持久 extenders，已 resolved 且有 rebound listeners 时才重新 make 并通知。构造先执行原 factory、再按顺序执行 extenders、最后缓存。重新 bind 的 `dropStaleInstances` 只移除 instance/alias，保留持久 extenders；显式 `instance` 在解析时直接返回，不补跑先前持久 extenders。已有调用方持有的旧引用不会因容器替换缓存而自动改指向。

## Spec

1. 在同一 `ContainerBuilder/Container/Resolver/Scope` 内核添加 typed extend，保留任意原 factory；不新增 driver 专属容器或公开 factory 读取入口。
2. `T: ?Sized + Send + Sync + 'static`；装饰器接收 `Arc<T>, Resolver`，返回 `BoxFuture<'static, Result<Arc<T>>>`，允许外部 dyn trait wrapper。
3. Builder/RegisterContext 提供同步登记；Container/BootContext 提供 async extend，成功返回前完成当前已缓存 singleton/instance 的变换。
4. 未解析服务保留持久有序链，支持先 extend 后 bind；同类型 factory 覆盖保留此链，旧 instance 上的即时扩展不迁移到新 factory。
5. 显式 instance 上登记的 async 扩展单独记录；Provider start 在 boot 前完成它们，直接 build 的用户由首次 resolve 完成，禁止同步读取未装饰旧值。
6. factory 与全部持久装饰器构成一次 singleflight 构造；只有完整成功才发布，取消/失败沿用已有 attempt/错误链，后续独立解析可重试。
7. runtime extend 的缓存变换与该类型解析串行化；不持 std Mutex 跨 await，回调/用户 Drop/唤醒均在内部锁外，递归同服务报 cycle。
8. 装饰器沿用原服务 Resolver 的 captive 规则、配置视图与 ResourceRegistrar；共享服务资源归应用，scoped 资源归相应 scope，transient 资源随调用 scope。
9. runtime 缓存变换失败保留旧缓存且不登记本次即时扩展；不承诺撤销用户对 Arc 内部状态或外部服务造成的副作用。
10. 采用下节明确的多 scope 当前视图隔离、typed rebinding 与 forget_extenders；alias/contextual 等后继仍开放，不伪称上游全部 Container 行为等价。
11. 单元/公开 consumer、Provider 顺序、第三方 custom Session driver、唯一模块示例和新生成应用形成目标验收链；本分支已验证通用容器、模块、生成应用，以及自定义 SessionStore 装饰与受管理 HTTP Cookie 往返。真实 TCP 与数据库服务仍属后继验收。D8 HTTP/Session/validation WIP 不作通用内核的前置。
12. 本分支实现范围为 Container/Provider、私有 helper、公开契约测试、唯一模块的 Provider/服务及测试、独立 consumer 与生成应用 fixture、用户 README 和本 ADR；根 `Cargo.toml` 仅登记契约测试。Session 仅增加 `SessionStoreHandle::store()` 只读克隆入口，未重写默认 Provider 绑定、根生成模板或 Session 存储内核。

## 已冻结公共 API

以下接口已在本任务 worktree 落盘，并通过本文所列定向编译、测试与严格 Clippy。沿用现有 BoxFuture，不加依赖：

```rust
// ContainerBuilder
pub fn extend<T, F>(&mut self, extender: F) -> &mut Self
where
    T: ?Sized + Send + Sync + 'static,
    F: Fn(Arc<T>, Resolver) -> BoxFuture<'static, Result<Arc<T>>>
        + Send + Sync + 'static;

// RegisterContext：同一泛型约束，委托同一个 builder
pub fn extend<T, F>(&mut self, extender: F);

// Container：同一泛型约束，作用于当前根容器视图
pub async fn extend<T, F>(&self, extender: F) -> Result<()>;

// BootContext：同一泛型约束，委托同一个 Container
pub async fn extend<T, F>(&mut self, extender: F) -> Result<()>;
```

调用形态：`app.extend::<dyn Driver, _>(|old, resolver| Box::pin(async move { Ok(Arc::new(ObservedDriver::new(old, resolver.resources()?)) as Arc<dyn Driver>) }));`。装饰器依赖当前公开 trait，不需要知道原 driver 类型、原 factory 或私有配置分支。

### 类型、注册与同步读取

- 用 TypeId 标识服务，保持 `Arc<Arc<T>>` 擦除方式；输出类型必须仍是 T，错误返回 Result，不接受 PHP mixed 的任意类型变化。
- 持久 extenders 独立于 Binding 保存，因此 extend 未绑定类型不创建 binding，`contains/len` 不变；`resolve` 无原 binding 仍报 `service_not_bound`。
- 同一个实例上的 builder 即时扩展按调用顺序记录到该实例，不加入持久链。后续 bind 或 instance 覆盖会丢弃这个实例尚未执行的即时链。
- `get_instance/get_ready` 在待执行的实例即时链存在时返回 None，防止同步路径绕过装饰；Provider start 完成待执行链后才开始任意 boot。这使 Provider boot 能观察装饰后的实例。
- 每个 instance 即时扩展是独立操作：按登记次序逐项发布并逐项调用 rebinding，回调可经 Resolver 读到本次发布的中间值。外部首次 resolve 等待剩余队列；同步 get_instance/get_ready 在仍有待执行项时返回 None。后项失败/取消不撤销已提交前项，重试只重跑未提交项。回调失败发生在本项发布之后，不回滚缓存，也不重复已提交本项的通知。
- 上条是 async 的必要时点适配：Laravel 的同步 extend 可立即执行，Rust 同步 register/build 不可阻塞执行任意 async 回调。回调错误移至 Provider start/首次 resolve 返回；直接 builder 的异步副作用无法保证在后续同步覆盖之前已发生，须公开这一差异，不能宣称字节兼容。
- 同时提供 `forget_extenders<T>`，只删除持久链、不撤销已缓存值；Builder/RegisterContext 同步清除；Container/BootContext/Scope async 等待同型操作与构造后清除。全套 forgetInstance/rebind/alias/contextual 不在此子簇自动新增，仍保持矩阵缺口。

### 运行期与并发 scope

按现有根/入口 scope 隔离方式解释调用上下文：Container 上的 runtime extend 作用于 singleton/instance 以及根 scope 的当前值；不跨请求主动替换其他已活跃 scope 的缓存。注册阶段的持久扩展适用于所有未来构造；当前根 scoped 的即时扩展不进入未来 scope/reset 的持久链，这与 Laravel 当前缓存实例分支一致。

同时提供 `Scope::extend<T,F>(&self, extender) -> Result<()>`（async）：scoped/bind 的未解析链与缓存变换归该 scope，singleton 仍委托应用；子 scope 的即时扩展不污染兄弟 scope，scope 关闭时清理局部扩展与缓存。Container 上未解析 scoped/bind 的持久扩展应用到未来构造，已解析子 scope 保留其旧值。现有 scope 是 Rust 并发隔离增强；Laravel 单容器没有多活跃 scope 同时观察定义更新的直接参照，必须以并发测试与公开差异说明验收，不能不加说明地把 root 变更广播到其他请求。

若总控要求“runtime 全局 extend 对每一个已活跃子 scope 的下次 resolve 也生效”，需要改为每个 scope 记录扩展游标/缓存修订并定义新 scope 与 reset 继承规则；这不是本簇已冻结语义。本簇采用显式 Scope 上下文，避免跨请求变换、用户 async 回调和关闭时点交叉。

本簇新增最小 typed `rebinding<T,F>`，官方 callback 用例不能靠“缓存值变了”代替：回调 `Fn(Resolver, Arc<T>) -> BoxFuture<'static, Result<()>>`，不返回新值；Builder/RegisterContext 登记，runtime extend 成功后按登记顺序调用。未解析 transient 不因有 listener 就提前构造；已成功解析 transient 的 runtime extend 在 listener 存在时重新 resolve 后通知，需增加 resolved 标志。回调失败时缓存已经更新，不声称回滚，错误返回调用方；callback 自递归 extend 及未发布装饰器的同型 resolve 需显式 cycle 检测，不以互斥锁永久等待。Builder/RegisterContext 提供登记；未新增运行期重绑 factory、alias 或 contextual API。Builder 同时保存最小跨类型 pending 操作序列，默认 preboot 按登记顺序逐项 drain，不遍历 HashMap 决定副作用次序；A 第一次→B→A 第二次的装饰和通知顺序有重复构建回归源码。显式回调或装饰器通过 Resolver 依赖另一类型时，仍沿原有 lazy/cycle 语义驱动该类型额外初始化；已提交游标使默认 drain 不再重复这些操作。本簇不把独立运行期请求扩展为跨类型全局串行。

### singleflight、取消、错误与资源

实现复用原 Attempt 与 wait graph：每缓存键另有短期 operation，不持 std Mutex 跨 await。transient 保留并发原工厂，各自登记活跃 attempt；装饰等待已有构造，后续解析等待装饰发布。发布之后 resolve 可读新缓存，其他 extend 仍等待全部通知完成；任务上下文阻止同型递归 extend。scope 关闭在锁外释放局部装饰闭包。

- 原 factory 与惰性扩展使用同一个构造 attempt 和 cycle/captive 检查；不能让装饰器在已缓存原值发布之后异步执行。
- runtime extend 等待正在初始化的同类型 attempt 完成，再按当前缓存分支变换；若原初始化失败则追加持久扩展并返回登记成功，不把原 factory 错误改称 extend 错误。两次 runtime extend 的序列以同类型操作进入顺序为准。
- 采用统一每缓存键显式状态机，resolve/extend 一起参与，不能只锁 extend 而让 get_ready/resolve 读到半成品。用户回调与依赖解析运行在锁外；新增同类等待边接入现有 cycle 检测，禁止 A/B 扩展交叉 resolve 死锁。
- singleton 即时变换取消/失败：保留旧值，后续解析可读旧值；这次即时 extender 不保留。未缓存链失败/取消：不缓存部分结果，持久链保留，下次重跑原 factory 和整条链。已成功的业务副作用不保证只发生一次。
- 已返回的旧 Arc 保持有效；替换缓存不强制关闭仍可能使用的原服务，原服务与 wrapper 的已登记关闭回调由原生命周期所有者按现有规则各执行一次。
- 扩展新增清理回调即使扩展失败仍归原 owner，并在该 owner shutdown 回收；不承诺回滚当前 ResourceRegistrar 已登记项。需要立即释放的临时资源由 extender 在返回错误前显式关闭。
- shutdown/close/reset 先封口，旧 generation 的 extend 不得向新 generation 或已关闭生命周期发布；错误保留现有 `scope_closed/service_cycle/captive_scope_dependency/service_initialization_failed` 安全链，不把原错误内容泄露到响应。

## 本分支验证与开放项

下表仅记录当前分支的实际入口；`--no-default-features` 结果不替代默认 feature、全 workspace 或真实服务验收。日志及命令/退出码 JSON 均在 `/tmp/openrust-r2x-verify/`，全部 Cargo 命令使用同一分支 `target`、`jobs=1`、`CARGO_INCREMENTAL=0`、`DEV/TEST_DEBUG=0`，未启外部服务。

| 入口 | 已验证的边界 | 结果与日志 |
| --- | --- | --- |
| `cargo test -p openrust --no-default-features --test container_extension_contract -- --nocapture` | 公开 typed trait 装饰、先 extend 后 bind 与 factory 覆盖、显式 instance/pending 顺序与重试、持久链与 forget、rebinding/回调错误、transient/singleton/scoped/root 视图、并发 singleflight/取消/失败/cycle/captive/资源 owner/关闭竞态、Provider 跨类型 preboot 顺序 | 24 passed，0 ignored/filtered；`contract-minimal.log` |
| `cargo test -p openrust --no-default-features --lib core::container::tests -- --nocapture` | 原 Container 的生命周期、缓存、scope/reset、资源回收、单飞、错误链与并发回归 | 32 passed，373 filtered；`legacy-container.log` |
| `cargo test -p openrust --no-default-features --lib core::provider::tests -- --nocapture` | 原 Provider 的 register/boot 顺序、配置 overlay、路由与清理回归 | 10 passed，395 filtered；`legacy-provider.log` |
| `cargo test -p openrust --no-default-features --bin server binding_extension_ -- --nocapture` | 唯一 `modules/example` 的 Provider 登记扩展、配置 overlay 后首次解析、boot 与进程内 HTTP 同实例；含隔离的非默认 greeting | 2 passed，29 filtered；`module-after-resource-helper.log` |
| `cargo test --manifest-path tests/consumers/container-extension/Cargo.toml` | 外部 crate 的 `dyn Trait` 异步包装、rebinding 与 scope 隔离 | 2 passed；`consumer-minimal.log` |
| `cargo test --locked --manifest-path tests/consumers/container-extension/Cargo.toml --features session original_custom_session_store_is_extended_through_managed_cookie_roundtrip -- --nocapture` | 原 Session Provider 的 `SessionStoreHandle` 上装饰原 `Arc<dyn SessionStore>`；受管理 HTTP Cookie 跨请求 load/write 与原 store 生命周期 | 1 passed，0 ignored，2 filtered；`session-consumer-locked.log`。移动旧测试到 `cfg(test)` 后，`cargo test --locked --manifest-path tests/consumers/container-extension/Cargo.toml --features session -- --nocapture` 为 3 passed、0 ignored/filtered；`session-consumer-all-after-test-layout.log` |
| fresh CLI 生成应用安装 `tests/consumers/container-extension/fixtures/`，随后 `cargo check`、`cargo test` | 普通应用 Provider 保留原 factory，boot 与进程内 HTTP 读取同一装饰后实例；生成应用完整测试组 | check exit 0，test 6 passed（含新 fixture 1 项）；`generated-check-fixed.log`、`generated-full-test.log`、`generated-final.json` |
| `cargo clippy -p openrust --no-default-features --lib --bin server --test container_extension_contract -- -D warnings` | 本簇适用源码严格 lint | exit 0；`clippy-approved2.log`、`clippy-approved2-input.json` |
| `cargo clippy --locked --manifest-path tests/consumers/container-extension/Cargo.toml --features session --all-targets -- -D warnings` | 新 Session getter 可编译、外部 consumer 所有 target 严格 lint | exit 0；`session-consumer-clippy-after-test-layout.log`。首次执行因原 consumer 测试代码位于普通 bin 顶层而触发 dead-code，已移至 `cfg(test)` 模块，保留失败日志 `session-consumer-clippy.log` |

`cargo fmt --all --check`、独立 consumer 的 fmt check 与 `git diff --check` 已通过。上述 24 项契约测试先于仅有的 import、同型 `clone_from` 与等价空集合断言整理；旧 Container/Provider 两组是在整理后运行。生产模块 helper 提取后，模块 2 项已重验。不同模型静态审查指出的非默认 overlay、Session getter 逃生口说明与原实例计数措辞均已修正并复审解除；main 集成审查和门禁仍由总控确认。

本 Session 用例的 `OriginalStore::new` 在 Provider 构造前执行，`created == 1` 只证明原实例构造一次，不能冒称直接测量框架 singleton factory 次数；通用 factory/singleflight 由上述 24 项契约与 32 项原 Container 回归分别覆盖。`SessionStoreHandle::store()` 是受信装饰器的底层逃生口，直接调用绕过受管理 Session 的状态/容量检查及句柄删除的 ServerSide 检查，不赋予客户端快照全局撤销能力。

尚未完成：第三方 **custom SessionStore** 已有进程内 HTTP 证据，但真实 TCP 或 PostgreSQL/MySQL/Redis 仍未由此验证。alias/contextual、完整 forgetInstance/rebind 等上游 Container 行为仍是后继缺口；本次 32/10 项旧单元回归与 24 项新契约只覆盖所列范围，不能据此关闭全部 Container/Provider 矩阵。默认 features、全 workspace test/doc、main 集成与跨分支组合验证也尚未由本分支完成。纯容器行为不需要 DB/端口；Session 真实服务由总控统一调度，不复用旧证据。

## 文件与交接

- worktree `/tmp/openrust-worktrees/r2-binding-extend`，branch `tasks/r2-binding-extend`，基线 `1838012`；未改 main、未提交或推送。
- 内核：`src/core/container.rs`、`src/core/container/extensions.rs`、`src/core/provider.rs`；公开契约：`tests/core/container_extension_contract.rs`，已在根 `Cargo.toml` 的 `[[test]]` 登记（根清单 `autotests = false`）。未增加根依赖或修改根 lock。
- 实际模块调用：`modules/example/providers/example.rs` 的 `register` 扩展 `ExampleMessage`，`boot` 先 overlay 模块配置、再解析最终实例，并贡献 `/api/example/container-extension`；`modules/example/services/message.rs` 与 `modules/example/providers/example/extension_tests.rs` 保持业务/测试在唯一模块内。
- 外部与生成路径：`tests/consumers/container-extension/` 独立 Cargo package、新的 Session feature 测试和生成 fixture 安装脚本；新生成应用只装普通 Provider，不复制 `modules/example` 或改 CLI 模板。用户说明在 `src/core/README.md` 及 consumer README；本 ADR 记录范围、差异和证据。`src/session/provider.rs` 仅新增 clone getter；共享计划/验收文档未由本分支改动。
- 证据文件位于 `/tmp/openrust-r2x-verify/`，当前输入哈希见 `clippy-approved2-input.json`；最终源文件和证据哈希以交接清单为准。target 保留供总控集成复用，不清理他人产物。
