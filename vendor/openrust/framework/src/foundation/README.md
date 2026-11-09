# 应用启动与生命周期

Foundation承载运行应用、编译期应用声明与统一Provider生命周期。完整官方Application契约仍按对应矩阵验收。

`Application::commands()` 与 `ProviderRuntime::commands()` 返回按登记顺序持有 `Arc<dyn Command>` 的快照。启动与运行期 Provider 共用同一命令目录，同名替换保留原槽位；已取得的快照继续持有原命令，新快照读取替换后的实例。命令枚举、匹配和执行不会跨用户回调或异步等待持有目录锁；调用方需要命令类型身份时使用 `command.as_ref().type_id()`。运行期与启动期命令都由同一注册表管理。

## 应用异常 Handler

应用或模块 Provider 可在 `boot` 中调用 `BootContext::exceptions(|handler| { ... })`，按实际 Rust 异常类型登记 `map::<E>`、`report::<E>`、`dont_report::<E>`、`render::<E>` 与最终 `respond`。业务错误用 `Error::internal(source)` 保留原始类型及 source 链；需要指定 HTTP 状态时可用 `Error::http_with_source(status, code, message, source)`。`ReportDecision::Stop` 停止后续报告和默认日志，但仍渲染。`dont_retry::<E>` / `dont_retry_when` 供应用队列决定重试，HTTP 渲染不会自行重试。CLI、队列或调度器可调用 `Application::report_exception`，仅应用的报告策略生效，不伪造 HTTP 请求并进入渲染。Handler 随应用实例隔离，不保存进程全局策略。调用方提供的 `context` 进入报告日志，应用须自行避免秘密信息；默认报告仅写安全错误码和状态。

HTTP 默认根据请求的 `Accept`、AJAX/PJAX 语义选择 HTML 或 JSON；`should_render_json_when` 可覆盖，`api_envelope_when` 可按路由显式保留 OpenRust 统一 API envelope，Web 请求不被强制包装。`respond` 接收已渲染的原生 `Response`，可保留或有意修改状态、头、扩展和流正文。`app.debug` 只由服务端配置决定，非 debug 的内部异常不输出 source 或追踪。完整入口与原生 Axum 逃生边界见 [HTTP 说明](../http/README.md#异常报告与渲染)。此家族目前处于 code-first 候选，最终组合门禁、真实请求及独审尚未执行。

`throttle::<E>` 可登记 `ReportThrottle::Lottery` 或应用实例内的 `Limit`；后者共享当前 App 的克隆 Handler，但不跨实例或部署副本协调。需要跨副本的异常报告限频时，应用应在类型化 report 回调中接入共享服务。`should_report` 会消耗本次限频配额，调用方不应为同一报告先探测再调用 `report_error`。
`dont_report_duplicates` 对共享的 `Arc<Error>` 使用 `report_shared` 时按同一实例身份去重；借用的 `&Error` 没有可保留的身份所有权，普通 `report_error` 每次独立报告。

## 容器与 Provider 生命周期

实现 `DeferrableProvider::provides()` 返回服务键，再用 `ProviderBuilder::deferred_provider` 或模块的 `RegisterContext::deferred_provider` 登记编译工厂。容器在解析别名后检查共享实例，首次需要某个延迟键时沿同一注册表装载 Provider。应用已启动时，首次解析等待该 Provider 完成 `boot`。可通过 `Application::is_deferred_service` 检查精确键，通过 `load_deferred_providers().await` 装载清单剩余项。

同一 Provider 的并发调用共享装载结果。注册失败消费触发键，已经发布的贡献保留，其他声明键可再次尝试。应用有在途装载时，`flush()` 返回 `deferred_provider_in_progress`。按键触发沿固定 Laravel 的启动阶段顺序。

领域 Provider 可在 `BootContext::extension_mut::<T>()` 中修改应用级贡献。调用者用 `Application::extension::<T>()` 读取。同一种贡献的借用独立协调，异步工作前先复制或克隆所需数据并结束借用。HTTP 装配在调用路由工厂前取得路由贡献快照，`Application::exception_handler()` 也返回可用于异步工作的 Handler 快照。

`Container::call`、`Scope::call`、`Resolver::call` 和 `ApplicationState::call` 使用同一份显式 `ConstructorParam` 声明解析 callable 参数。调用级参数只属于当前调用。回调在经过检查的槽位读取服务或值，可以把取得的 `Arc` 移入异步工作。应用入口沿当前请求或命令作用域解析；直接使用容器则选根作用域。

Provider 可在 `RegisterContext` 中通过 `register_method` 登记对象方法，通过 `bind_method` 登记替代实现。调用者将 `MethodCall` 交给 `call_method`，可提供已有对象或从容器解析接收者。显式对象调用使用类型上下文，类入口沿用原上下文。绑定命中时直接执行，绑定与方法声明在容器 `flush` 后继续可用。

静态入口通过 `call_static` 复用同一作用域。传入 `StaticCall::direct::<Declaring>()` 可按声明类型解析依赖。命名入口 `StaticCall::named::<Called, Declaring>(name)` 还按调用类型与名称查找 `bind_static_method`。替代回调取得调用类型键和当前解析器。两类静态入口均无需接收者。

`openrust::container` 提供 `ContainerBuilder`、`Container`、`Resolver` 与 `Scope`。`Resolver` 使用弱容器句柄，延迟解析时容器已释放则报告 `container_closed`，避免 singleton 保存解析器造成引用环。类型键支持 `dyn Trait`，`bind` 每次创建，`singleton` 同容器共享，`scoped` 同生命周期共享，`instance` 注册已有 `Arc`。同类型后注册覆盖前定义；`build` 建立唯一容器后，Provider 注册仍可在同一容器动态重绑，不使用 PHP 反射构造。重绑使各 scope 的旧缓存失效；已交给调用方的旧 `Arc` 和在途引用仍有效，新解析按新定义构造。绑定版本与缓存槽准入同锁核验，旧构造不能回填新槽。

`RegisterContext::extend::<T, _>(...).await?` 保留原 factory，再把异步装饰器依登记顺序作用于其结果；可在 `bind` 前登记，也可包装外部 `Arc<dyn Trait>`。装饰器接收旧 `Arc<T>` 与 `Resolver`，返回 `BoxFuture<'static, Result<Arc<T>>>`。同类型 factory 覆盖仍保留未解析服务的持久装饰链；`forget_extenders::<T>().await?` 清除未来构造的持久链，不撤销已缓存服务。`rebinding::<T, _>(...).await?` 登记监听者；服务已绑定时立即解析并返回当前 `Arc`，后续缓存更新按顺序通知。回调错误在当前 `register` 位置短路，不延迟到 `register` 返回后。已缓存 singleton/instance 可通过 `Container::extend(...).await` 或 `BootContext::extend(...).await` 即时变换；变换失败保留旧缓存，通知失败则新值已发布。`ServiceProvider::register` 返回 `BoxFuture`，绑定、实例、装饰与通知均在原调用位置 `await?`；已完成的贡献即使后续操作失败也不回滚。直接使用 `ContainerBuilder` 时，显式 `instance` 的待执行装饰仍在首次异步 `resolve` 执行；待执行时同步 `get_ready` 不返回旧值。

```rust,ignore
app.extend::<dyn Driver, _>(|original, resolver| {
    Box::pin(async move {
        let dependency = resolver.resolve::<Dependency>().await?;
        Ok(Arc::new(ObservedDriver::new(original, dependency)) as Arc<dyn Driver>)
    })
}).await?;
```

运行期 `Container::extend` 处理 singleton/instance 与根 scope 当前值，不主动替换其他活跃请求 scope 的缓存；请求内用 `Scope::extend` 只装饰该 scope 的 scoped/bind 服务，singleton 仍归应用。已返回调用方的旧 `Arc` 不会自动变成新值。此异步时点和多活跃 scope 是 Rust 适配；alias/contextual 等 Container 后继契约仍开放。唯一业务示例在 `modules/example/providers/example.rs` 装饰配置驱动的 `ExampleMessage`，`/api/example/container-extension` 展示原 greeting、装饰标记及 boot/HTTP 同一服务实例。CLI 新建应用只有空模块清单，不自动复制该业务模块；应用可按 Provider 模式注册自己的装饰器。

根容器也有 scoped 生命周期，`forget_scoped_instances()` 清除旧代，下次解析重建；子 scope 独立。关闭/取消在途解析后可重新构造，不保留失败占位；同链与并发等待环返回类型链。singleton 捕获 scoped 被拒绝，这是长驻进程安全增强。错误 `Debug` 只输出 code/status，工厂原始错误保留在内部 source，不直接输出 HTTP。

缓存工厂在成功、失败或取消时先结束构造帧并清除已结束的等待边，再发布结果；已完成服务不再创建等待关系。scope关闭和根reset的唤醒、用户值释放都在容器内部锁之外执行，允许Waker同步重入。

生产 `bootstrap` 使用同一 `Container`：`ApplicationDefinition::default_providers` 从同次配置仓储选取框架默认清单，已编译包经 `package_providers` 显式接入，`ApplicationDefinition::providers` 仅给出应用清单，模块清单随后接入；`ProviderBuilder` 先建立唯一容器，依清单顺序执行异步 `register`，经基础配置校验后依 active 顺序 `boot`，最后校验配置与贡献并解析必需服务。普通重复具体 Provider 类型只保留首次实例，显式 `force_provider` 则重新执行 `register`，成功后在原首次 boot 槽位替换实例。`Application::register_provider` 与 `force_register_provider` 在启动后使用相同容器；新实例注册成功便立即 boot。当前失败 Provider 的索引不更新，已成功登记的子项和此前贡献仍可见；boot 失败保留新索引和贡献，普通重复不会重试 boot。请求 `Inject<T>` 从请求 `Scope` 异步解析；每次命令使用独立 scope，Provider 使用同一应用容器。`ApplicationState::get<T>` 仅同步读取已就绪的实例，首次工厂构造使用 `resolve<T>().await`。启动阶段及运行期 Provider 登记的资源都归同一应用 `ResourceOwner`；force 不提前关闭旧资源，失败 boot 已登记的资源仍在统一 shutdown deadline 内逆序回收。启动 register/校验失败或启动 future 取消时，已提前解析的 root scoped 资源与 Provider 资源共用同一回收期限；关闭开始后拒绝新 Provider 登记。请求 scope、受管任务和连接池也遵循该共同期限，错误不会静默忽略。配置区段及包默认值在各次注册后即对后续解析可见，已有来源值始终高于 Provider 默认值；不同 Provider 可以按登记顺序对同一包区段贡献默认值，schema/校验器只允许原 owner 显式 force 替换。最终仍经过 `validate_base` 校验，包操作仅应用一次；`Application::config()`/`figment()` 保持启动快照，`config_repository()`/`section()` 读取运行期仓储，不自动重建既有服务。运行期路由贡献可被随后 `build_router` 读取；已运行的 Axum Router 是快照，其实时可见性仍待固定上游真实 HTTP 对照。

`ApplicationDefinition::package_providers` 返回带来源的 `PackageProvider`。框架在启动注册前校验包间类型冲突，按 `ServiceProvider::dependencies` 稳定排序包段。确定会登记的依赖优先于可能嵌套的候选。缺失、后置及循环依赖会在预检报告。

### register 中登记子 Provider

`RegisterContext::register_provider`、`force_register_provider` 和 `boxed_register_provider` 与 App 对应入口共用同一注册执行器。父 register 中 await 子注册后才继续下一语句；正常启动时子 register 不提前 boot，全部 register 完成后按实际登记顺序 boot。首次注册时子先完成并进入清单，父随后进入；已有父类型的force仍保首次槽位。已启动 App 的子注册则在返回前完成 register/boot，父才继续。不要手工调用另一 Provider 的 register 或 boot。

```rust
use openrust::foundation::RegisterContext;
use openrust::core::Result;
use openrust::support::{BoxFuture, ServiceProvider};
struct ChildProvider;
impl ServiceProvider for ChildProvider {}
struct FeatureProvider;
impl ServiceProvider for FeatureProvider {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            app.register_provider(ChildProvider).await?;
            Ok(())
        })
    }
}
```

同类型普通子注册去重，force 成功后保留首次槽位；父失败不会撤销已经成功的子及其发布贡献。启动整体失败/取消走原资源关闭，运行期失败的已发布资源继续由同一 App 的 shutdown 回收。进行中的新类型递归登记报 `provider_registration_cycle`；取消释放进行中标记，已登记实例不丢失。已有实例的非force查询仍返回旧身份。测试移除约束覆盖同一次启动中的嵌套 force；已启动 App 的显式运行期注册仍按原接口处理。

`RegisterContext::resolve<T>()` 显式解析同一容器的服务，提前创建的受管资源仍随同一启动失败/取消或应用关闭回收；文件初始化也可直接调用。`config_repository()` 返回同一次注册的仓储句柄，子写入可由父后续读取；不重建既有客户端。原平面 Provider 依赖预检保留，子自身在调用点检查前置依赖。若外部 Provider 通过类型依赖引用子项，父通过 `ServiceProvider::nested_providers()` 纯声明这些身份；预检按其出现位置处理，父 register 返回前还核对它们是否实际登记或复用，不能拿声明代替执行。Rust 无法预先反射异步回调中的注册类型，未声明的未来类型仍不在预检图内；该显式声明是此依赖增强的必要适配，不冒充 Laravel 自动拓扑。

模块所需的空 Events 默认服务使用独立内部 Provider 身份，只补未绑定的服务，不抢占真实 `EventsServiceProvider` 的去重位置；后续嵌套监听 Provider 正常替换默认绑定。显式移除真实事件 Provider 同时抑制默认补位。唯一示例的父 Provider 与生成应用公开 fixture 均通过此管线接入，模块 JSON 发现与编译 files 初始化沿此管线接入；class aliases 与完整 deferred 加载仍有后继范围。

### 通过事件装载延迟 Provider

在 `DeferrableProvider::when()` 返回事件的稳定名称或 `*` 通配模式。派发使用应用现有的 `Events` 服务。事件装载执行普通 Provider 注册，保留所有服务键。若应用已启动，派发等待 Provider 启动完成。响应中的 `DataValue::Object` 可下转为 `RegisteredProvider`，借用同一注册实例。服务键解析和事件同时到达时，共享一次装载。应用清空后，外部持有的旧派发器保留监听。新派发器使用新的监听目录。

### 正式引导的服务清单

普通应用在 eager 注册前校验并复用 `bootstrap/cache/services.json`，清单缺失或声明变化时同目录原子发布。低层 `ProviderBuilder` 和 `TestApp` 维持内存清单。编译类型和工厂归当前进程，文件只保存有序元数据。用 `clear-compiled` 清理服务与包缓存，配置缓存由 `config:clear` 单独处理。

### 测试替换容器中间件

`TestAppBuilder::without_middleware_type::<M>()` 和 `TestApp::without_middleware_type::<M>()` 可将容器登记的 `TerminableMiddleware` 类型替换为直通处理。全局和命名登记共享同一类型判定。`with_middleware_type::<M>()` 恢复后续请求。处理与响应完成后的终止读取同一请求快照，重建测试应用时恢复蓝图。

## 入口作用域与资源回收

- HTTP：每个请求创建独立 scope，响应 body 仍持有它；body 结束、出错或丢弃，以及请求处理被取消时关闭。流式响应尚未结束时，scope 仍然有效。
- 命令：每次 `Application::run_command` 有独立 scope；长驻的 `queue work` 命令 scope 不代替每次任务尝试的 scope。
- 队列：每次 job attempt 使用新 scope，成功、失败、超时或取消都会关闭；重试不会复用上一次的 scoped 实例。`JobContext::service` 从该 attempt 解析。直接调用没有应用状态的 `Queue::run_next` 时，不提供应用服务上下文。
- 调度：每个到期时段的事务派发使用独立 scope，后续 worker 执行时再创建 job attempt scope。当前全局调度不继承调用者的租户、请求 ID 或其他执行元数据；调度项目前不声明租户。

`BootContext` 和 `ApplicationState` 都可登记请求与命令的生命周期时长处理器。内核在终止事件、中间件及应用回调之后，按登记顺序运行耗时严格超过阈值的处理器。处理器在原入口作用域内运行，可读取以应用时区显示的开始时间。命令状态随 `Command::run` 的结果映射为成功 0 或失败 1。HTTP 处理器在响应正常完成后借用请求和响应头。

`Scope::run` 为当前 future 安装 scope，让同一容器的 `ApplicationState::resolve` 使用它。普通 `tokio::spawn` 不继承 task-local scope 或执行上下文；需要共享本次生命周期的后台工作时，用 `Scope::spawn` 登记受管任务，显式传入允许传播的元数据和 deadline。scope 关闭时会请求取消并等待受管任务；任务仍需合作检查取消信号。例如，在已取得 `Arc<Scope>` 的代码中：

```rust,ignore
use openrust::core::ExecutionMetadata;

scope.spawn(ExecutionMetadata::default(), None, |cancel| async move {
    cancel.cancelled().await;
})?;
// 在 scope 的所有者处：
scope.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5)).await?;
```

`Scope::shutdown(deadline).await` 关闭解析并等待资源清理；`Scope::close()` 和 Drop 触发关闭与有界回收，但调用者不能据此认定异步清理已结束。超过期限会返回错误，已逃逸的 `Arc` 仍由持有者持有，不能强制销毁；关闭后通过该 scope 新解析返回 `scope_closed`。应用对活动子 scope 有容量限制，达到上限返回 `scope_capacity_exceeded`；生命周期标识耗尽返回 `container_capacity_exceeded`，调用方应处理这些错误。

自定义队列 driver 若在超时或租约丢失时丢弃 handler future，应在确认或重试前显式等待 `JobAttempt::shutdown`。每次重试都创建新的 context 和 guard；`JobRegistry::execute` 会复用已创建的 attempt scope。下面省略 driver 的租约和持久化逻辑：

```rust,ignore
use openrust::core::{Cancellation, Error};
use openrust::queue::JobContext;
use std::time::Duration;

let attempt = JobContext::new(job_id, attempt_number, max_attempts, Cancellation::new())?
    .with_app_state(app_state.clone())
    .begin_attempt()?;
let result = {
    let execution = registry.execute(job_name, payload, attempt.context().clone());
    tokio::pin!(execution);
    tokio::select! {
        result = &mut execution => result,
        () = tokio::time::sleep(Duration::from_secs(30)) => {
            Err(Error::custom("job_timeout", "job attempt timed out"))
        }
    }
}; // 先丢弃执行 future
let cleanup = attempt.shutdown(tokio::time::Instant::now() + Duration::from_secs(5)).await;
match (result, cleanup) {
    (Err(error), Err(cleanup_error)) => {
        // 安全记录 cleanup_error 的错误码，然后保留首个错误。
        let _ = cleanup_error.code();
        return Err(error);
    }
    (Err(error), _) | (Ok(()), Err(error)) => return Err(error),
    (Ok(()), Ok(())) => {}
}
```

`JobAttempt` 的 Drop 是取消兜底，不等待回收。已开始的 context 不能重绑定到另一个应用：`scope`、`service` 和 `JobRegistry::execute` 会以 `queue_attempt_container_mismatch` 拒绝跨容器使用。将 context 留在原应用内使用；若 handler 与清理均失败，调用方应保留首个业务错误并安全记录清理失败。

## 当前应用启动资产

### 测试应用

`openrust::testing::TestApp<A>` 使用与生产相同的 `ApplicationDefinition::providers`、`ProviderBuilder` 和 Router。测试显式传入 `Figment`，不修改全局环境或工作目录。`remove_provider(ProviderId)` 在注册前调整清单；`configure_after_register` 在所有 Provider 注册后、boot 前通过同一 `Container` 异步覆盖绑定，因此后续 `boot` 能读取替代服务。测试可通过 `request` 发起进程内请求，默认总期限五秒、响应体上限 1 MiB；`request_with_limits` 可调整这两个预算，`raw_response` 保留流式正文并由调用方消费或丢弃。

`TestResponse` 提供状态、响应头、字节和 JSON 读取以及链式断言；断言失败不会回显正文或头值。状态断言遵循应用配置的 `[api] http_status`，默认为 Laravel 式语义状态；仅显式配置 `always_ok` 时才将框架 JSON 错误映射为 HTTP 200。`shutdown(deadline).await` 等待本应用拥有的资源回收；`reset(deadline).await` 先关闭整个旧应用，再用相同蓝图重建，不把根 scoped 缓存清空冒充完整重置。这里的 deadline 只约束旧应用关闭，新应用的启动与路由构建不受同一期限约束。丢弃 `TestApp` 会立即封闭新解析；存活的 `Tokio` runtime 才能继续异步兜底回收。独立 `TestApp` 持有各自容器，捕获的外部共享 `Arc` fixture 由调用方负责重置。实际业务替换与 Provider 移除示例见 [`modules/example`](../../../../../modules/example/README.md)，CLI 生成项目也包含普通 HTTP 与认证测试。进程内请求不替代真实 TCP 验收。

需要从空数据库测试迁移命令时，使用 `TestApp::<A>::builder(sources).run_command("migrate", args).await`：该入口以相同命令名引导并执行一次命令，然后限时回收应用；成功后再用普通 `build()` 验证迁移后的启动与请求。命令失败仍回收资源，命令和回收同时失败时保留命令错误并记录回收错误码；取消执行 future 时沿夹具 Drop 做异步兜底回收。它不返回带命令上下文的应用。

其他 OpenRust package 所依赖的核心契约包括：

- [`ApplicationDefinition`]：项目实现一次该 trait，用于描述应用的组装方式。
- [`ProviderBuilder`] / [`ServiceProvider`](crate::support::ServiceProvider) / [`Application`] / [`ApplicationState`]：框架、应用模块和第三方 package 使用同一扩展点与容器。`register` 声明绑定和配置区段，`boot` 贡献路由、命令及 middleware。依赖可在所有注册完成后解析；配置覆盖在最终校验前仍可提交。`Application::run_command` 允许同一应用依次执行命令；顶层 CLI 在命令结束后调用 `Application::shutdown`，HTTP `serve` 在退出时合作取消、限时回收受管资源。
- `openrust_modules::ModuleManifest` 声明已编译模块及激活器；启用模块的 Provider 进入同一 register/boot 管线，全量 metadata（含未加载模块）与激活器由同一 Container 的 `ModuleRepository` 共享。`ModuleDescriptor::requires` 保留启用依赖校验与拓扑排序；普通启停不级联修改依赖状态，也不删除迁移历史。资源根、独立状态路径与文件边界见 [模块激活配置](../../../modules/README.md#模块激活与运行根)，业务接线见 [`modules/example`](../../../../../modules/example/README.md)。
- [`Command`](crate::console::Command)：应用二进制对外提供的运行时命令（`serve`、`migrate`、`queue work` 等）。命令默认拒绝额外参数；接受参数的命令须实现 `accepts_args`。
- 控制台入口在引导前确定命令名，Provider 可通过 `BootContext::console_command()` 与 `is_console_command()` 读取本次候选命令。普通 `bootstrap()`、直接 `ProviderBuilder` 与 `TestApp::build/reset` 均无该上下文；受控的 `TestAppBuilder::run_command` 只引导、执行并关闭同名命令。框架按最终匹配的命令实例执行启动前检查；候选名本身不授权跳过校验。内部迁移入口允许空库先执行官方迁移，再由后续正常应用启动校验所需表。
- [`Error`](crate::core::Error) / [`Result`](crate::core::Result) / [`ApiResponse<T>`](crate::core::ApiResponse) / [`PageMeta`](crate::core::PageMeta)：统一错误类型、JSON 响应封装和类型化分页元数据。
- [`ExecutionContext`](crate::core::ExecutionContext) / [`ExecutionMetadata`](crate::core::ExecutionMetadata) / [`Cancellation`](crate::core::Cancellation)：单次 HTTP、命令或 Job 尝试的 task-local 关联信息、时间预算和合作取消；task-local 不自动传播至 `tokio::spawn`。
- [`RateLimiter`](crate::core::RateLimiter) / [`RateLimitStore`](crate::core::RateLimitStore)：固定窗口配额服务契约。存储故障向上传播；超限请求返回 429 和 `Retry-After`；每种实现都需说明共享范围与原子性边界。
- [`Readiness`](crate::core::Readiness)：进程 listener 生命周期状态，并允许 Provider 注册有界异步依赖探针；HTTP `/ready` 聚合探针结果，`is_ready()` 本身仍只表示 listener 状态。
- `config`：应用在根 `config/*.rs` 声明可编辑配置，经 `ApplicationDefinition::config()` 返回显式 `ConfigManifest`。加载器先建立所选 dotenv 与外部环境快照，再求值已编译配置函数，发布到唯一 `ConfigRepository`；不运行时扫描 Rust。模块使用相同代码配置机制；需要 TOML 时显式注册可选 source，不再默认扫描 `config/*.toml`。`APP_NAME`、`APP_URL` 和命名空间读取沿公开 `ConfigContext` 与统一来源优先级；缓存命中跳过 dotenv 和配置函数，遇不可序列化 typed/factory 必须明确拒绝，不能静默丢弃。Rust 代码修改需要重新编译，环境配置修改需要重启；框架包的 `src/config/` 定义框架类型与加载机制，根 `config/` 定义应用策略。
- [`lifecycle`]：启动顺序、优雅停机信号与应用后台刷新任务回收。

### 外部包预装配

外部包通过 `ProviderRegistration` 在任何 register 回调前校验声明并绑定初始服务：返回项保留清单位置，`ProviderPreparation::append_provider` 尾接已编译 Provider。同类型重新 `registration` 会替换并移动声明槽位，`registration_mut` 只修改内容；默认 Provider 的插入边界不随删除或替换错位。声明失败先于用户回调，所有实际注册、依赖、boot 和资源回收仍交唯一内核。`bootstrap_provider` 用于核心命令之后、默认与应用项之前的扩展配置和命令，用户同名命令仍后覆盖。

模块包使用这些公开契约，模块状态与发现机制见 [独立模块扩展](../../../modules/README.md)。框架库不包含模块实现或反向依赖该扩展。

应用内核位于独立 `openrust-framework` package 的 `vendor/openrust/framework/src/foundation/`；`config`、`http`、`database`、`auth` 和 `mail` 是同一 package 中的兄弟 Rust 模块。根应用依赖此框架包，使用 `openrust` 库名。CLI、proc-macro 和可复用测试工具继续作为独立 workspace package，维持工具与构建边界。

`ServerConfig` 包含 `[server].trusted_proxy_ranges`（默认未配置，显式空列表关闭平台自动信任）与 `trusted_proxy_headers`（默认 62）、可选 `[server.trusted_hosts]`、`[server.compression]`（默认启用）和 `[server.cors]`（默认禁用）。可信代理项接受 IP 或 CIDR，由 HTTP `ClientIp` extractor 使用；只有直接 socket peer 可信时才处理转发头，否则忽略任意转发头。可信主机可在应用或模块 Provider 的 `boot` 中用 `BootContext::trust_hosts` / `trust_hosts_with` 覆盖静态配置；请求与异常边界见 [HTTP 说明](../http/README.md#可信主机)。CORS 默认允许来源列表为空，方法为 GET/POST/PUT/PATCH/DELETE/OPTIONS，请求头为 `content-type`/`authorization`/`x-request-id`，暴露 `x-request-id`，不启用 credentials，预检缓存 600 秒。环境变量使用 `APP_SERVER_CORS_ENABLED=true`；列表采用 Figment 数组语法，例如 `APP_SERVER_CORS_ALLOWED_ORIGINS='["https://example.com", "https://other.example"]'`。

HTTP 响应使用 `ApiResponse<T>`，包含 `success`、稳定的 `code`、安全的 `message`、有业务载荷时出现的 `data`、可选的类型化 `PageMeta` / `errors`；错误 JSON 额外包含 `request_id` 和 RFC 3339 UTC 字符串 `timestamp`（毫秒精度）。每个 HTTP 响应都通过 `x-request-id` 响应头提供请求关联 ID，成功 JSON 正文不重复携带诊断字段。无载荷时省略 `data`，空集合仍返回 `[]`。错误可使用 `Error::custom(code, message)`（semantic 模式默认状态码为 400）或 `Error::custom_with_status(status, code, message)`。`Error::rate_limited(seconds)` 会生成 `Retry-After`；`Error::validation_with_detail` 提供 422 字段错误。5xx 错误详情会隐藏。HTTP 并发上限触发的 `http_overloaded` 属于预期拒绝，不逐请求写 ERROR 日志；启用指标时由固定标签的过载计数器统计。HTTP middleware 为每个请求设置不可变的 `request_id` 和 `[api] http_status`。默认 `semantic` 使用 历史决策 008 中的状态码表；只有显式设置 `APP_API_HTTP_STATUS=always_ok` 才将框架 JSON 响应状态映射为 HTTP 200。

`RateLimiter::check(key, limit, window)` 会在已注册的 `RateLimitStore` 中计入一次请求，并返回剩余配额；超限时返回带 `Retry-After` 的 429 错误。核心契约不决定 HTTP 身份提取方式或 middleware 策略。调用方应按策略命名 key，并对同一个 key 使用一致的时间窗口；存储故障不会被当成未超限。数据库 store 保存 SHA-256 摘要而非原始 key，但无法阻止对低熵值的猜测。

`FrameworkConfig.i18n` 定义 locale 默认值。`TranslationServiceProvider` 注册共享 `Translator` 服务；HTTP middleware 解析 `Accept-Language`，并为当前 task 设置请求 locale。框架响应和内置 validator 规则消息使用该 locale，同时保留错误码与字段路径。应用消息可从 JSON 资源合并，并通过 `RequestLocale` 翻译；普通 `{name}` 文本保持兼容，也可用有限 `plural` / `select` 分支。`Translator::format_number` 接受精确十进制字符串；`format_date` 接受已按业务时区确定的日期，`format_datetime` 按传入 Chrono 值的本地墙上时间格式化。显式 task scope 使用 `Translator::with_locale`。未指定语言时采用同一应用的 `default_locale`，后台可用 `with_default_locale`；Provider 将该服务接入同一异常 Handler，默认 JSON/HTML/应用 envelope 的用户消息与验证摘要共用目录，稳定 code/status/header 与内部诊断不翻译。生产 debug=false 时 5xx 保持安全通用消息，不泄露 source。具体配置与边界见[本地化](../../../../../docs/src/digging-deeper/localization.md)。

`openrust::validation::validate(value)` 可在命令与 typed Job 中复用 HTTP `Validated<T>` 的 validator 错误转换。`ExecutionContext::current()` 读取单次调用的 metadata、剩余 budget 与 cancellation；子调用 deadline 不超过父级，子 timeout 不会取消父级。队列可传播经校验的 request ID、traceparent、locale 和可信租户 ID；不序列化用户身份或请求凭据；见 历史决策 044。

```json
{
  "items": {
    "plural": {
      "argument": "count",
      "forms": { "one": "{count} item", "other": "{count} items" }
    }
  },
  "member": {
    "select": {
      "argument": "role",
      "cases": { "admin": "管理员", "other": "成员" }
    }
  }
}
```

`plural` 使用 ICU/CLDR 类别键（`zero`、`one`、`two`、`few`、`many`、`other`），`select` 按参数精确匹配有限字符串键；两者都必须提供 `other`。缺少参数或分支时走 `other`，模板中缺少的 `{name}` 仍保留原占位符。地区格式使用 ICU4X 编译数据，无效十进制或不可格式化日期返回错误，不会改写 API 数值字段。

邮件支持 `LocalizedEmail`，可使用显式选择的 locale 从共享 i18n catalog 渲染主题和正文；其他格式仍可通过自定义 `Email` 实现。SMS 使用 `openrust::sms::{SmsMessage, SmsSender, SmsServiceProvider}` 和公开的 `SmsDriver` 契约。默认log驱动只记录接受摘要且可在production启动；`sms-twilio` 会启用 Twilio Messages adapter 和有界共享 HTTP client。日志驱动不代表外部送达，实际Twilio投递需显式配置与授权。成功 receipt 仅表示供应商接受请求；异步队列重试和最终送达跟踪是独立能力。

`ConfigLoader::figment()` 现在返回 `Result<Figment, ConfigError>`，直接调用者使用 `?` 传播来源错误；`load`/`load_framework` 使用同一失败语义。此行为不修改领域默认值、环境变量优先级或具名driver配置。

服务工厂初始化失败的 CLI 诊断保留依赖服务路径，服务身份由受封装的 Rust 类型信息构造，并显示框架定义的固定配置键和原因码；不拼接任意来源文本或自定义错误码，深依赖路径有界。`ConfigError::Diagnostic` 的原始来源留在私有错误链，HTTP 5xx 继续仅返回通用消息。
