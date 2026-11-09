# 请求生命周期与应用启动

默认 `src/main.rs` 只启动 `bootstrap/app.rs` 中的应用。应用通过 `ApplicationDefinition::config()` 提供代码配置清单，通过 Provider 清单和模块清单提供启动贡献。

启动先选择 dotenv 和部署环境，再求值配置函数，向同一 `ConfigRepository` 发布配置。框架、应用和模块 Provider 进入同一个 Container：`register` 登记服务，`boot` 使用已登记服务挂载路由和资源。模块不是第二套容器或独立子应用。

服务工厂失败时，日志会转义控制字符，并将每个服务类型名限制在 128 字节以内。`Error::ServiceInitialization` 的 `service.name()` 仍返回完整类型名，内部错误链可供诊断。HTTP 响应使用 `service_initialization_failed` 和通用错误消息。

### 声明可构造的具体类型

应用在 `ApplicationDefinition::configure_providers` 中调用 `builder.class_descriptors(...)`，把应用或扩展提供的类描述加入启动清单。框架在任何 Provider 准备或注册之前检查重复类型、重复符号和构造参数声明。登记清单时不会执行构造器。

```rust
use std::sync::Arc;
use openrust::container::{ClassDescriptor, ConstructorParam};

struct Greeting(Arc<Locale>);
struct Locale;

let classes = [
    ClassDescriptor::concrete::<Locale>("app::Locale", vec![], |_| Ok(Arc::new(Locale))),
    ClassDescriptor::concrete::<Greeting>(
        "app::Greeting",
        vec![ConstructorParam::class::<Locale>("locale")],
        |args| Ok(Arc::new(Greeting(args.service::<Locale>(0)?))),
    ),
];
// 在 configure_providers 内将 classes 传给 builder.class_descriptors(classes)。
```

普通 `resolve::<Greeting>()` 在当前作用域递归解析依赖，再调用构造回调。描述符提供的是编译期类型和构造信息，因此只支持主动声明的类型。单独登记描述符不会令 `contains::<Greeting>()` 成立。显式绑定优先，可沿用 `bind`、`singleton`、`scoped` 或 `instance` 控制生命周期。

### 构造参数的弱标量转换

构造描述默认以 `ConstructorParam::scalar::<T>` 校验精确的 Rust 类型。需要与 Laravel 构造调用的弱标量行为对应时，显式选择 `ConstructorParam::php`。例如 `ConstructorParam::php("retries", PhpTarget::Int)` 可把完整数字字符串 `" 8 "` 转换成 `i64`。非空调用覆盖、上下文值和默认值共用同一转换规则。

| 目标 | 构造回调读取的类型 |
| --- | --- |
| `PhpTarget::Int` | `i64` |
| `PhpTarget::Float` | `f64` |
| `PhpTarget::Bytes` | `PhpBytes` |
| `PhpTarget::Bool` | `bool` |
| `PhpTarget::Number` | `InputNumber`，保留整数或浮点种类 |
| `PhpTarget::IntBool` | `PhpIntBool` |

`PhpBytes` 保留原始字节。业务接口需要文本时可用 `std::str::from_utf8(bytes.as_bytes())` 验证编码。显式 `PhpStringable::new` 可为字节目标提供字符串回调。可空槽接受明确传入的 `ParameterValue::null()`，变参通过 `ResolvedArgs::values` 读取有序槽值。

容器完成整批依赖和参数选择后，按声明顺序转换弱标量。选参失败时，转换和构造器都不会执行。转换发出的诊断会同步交给策略，策略拒绝时把错误返回调用者。独立使用 `ContainerBuilder` 时，可用 `scalar_diagnostics(ScalarDiagnostics::new(handler))` 指定处理函数。

由 Provider 启动的应用默认把弱转换警告作为构造错误返回。应用可通过 `logging.error_reporting` 设置报告掩码，默认值为 `-1`。清除位值 `2` 会让警告级转换继续。精度丢失是弃用诊断。应用装配配置后即可报告，包括 Provider 的注册与启动阶段。配置 `logging.deprecations` 可指定 `channel`、`trace` 与 `while_testing`。默认不写弃用日志，测试环境可通过 `while_testing: true` 启用。当前可用通道由 `tracing` 提供，读取当前应用配置。通道注册、缓存和其他 Laravel 日志驱动仍归日志模块。设置 `trace: true` 会附上 Rust 回溯。根应用和生成项目也从 dotenv 读取 `LOG_DEPRECATIONS_CHANNEL`、`LOG_DEPRECATIONS_TRACE` 和 `LOG_DEPRECATIONS_WHILE_TESTING`。

### 具名键、别名与 trait 视图

`ServiceKey::named("app::Greeting")` 使用登记时的类符号，与该类的类型键共用绑定和缓存。其他字符串是独立键，保留原样并区分大小写。可用 `bind_key`、`singleton_key`、`scoped_key` 或 `instance_key` 在这些键上登记服务。

使用 `alias(target, alias)` 添加别名。按键解析、`get_alias` 和 `resolved_key` 跟随别名链，出现环时返回 `service_alias_cycle`。 `contains_key` 把别名本身计为已登记，`is_shared_key` 查询原键。写入绑定或实例会替换原键上的别名。`extend_key` 沿别名链装饰所选服务，回调接收该服务的声明类型。

为类描述添加 `with_projection(openrust::class_projection!(Greeting => dyn GreetingView))` 后，可用 `resolve_key::<dyn GreetingView>(key)` 获取同一个对象的 trait 视图。查找先选定绑定或描述符，再在调用工厂前检查声明输出。视图与具体类型共享实例和作用域缓存。需要以接口类型直接解析时，可以显式登记该接口的别名或绑定。

HTTP 处理器可通过 `State<ApplicationState>` 调用 `state.resolve_key::<T>(key)`，使用当前请求作用域。

### 条件绑定与调用参数

Provider 可用 `bind_if`、`singleton_if` 或 `scoped_if` 安装默认工厂，对具名键使用相应的 `*_key_if` 方法。容器原子检查原键。实例、工厂或别名已占用该键时返回 `false`。仅有类描述时仍可登记默认工厂。

```rust
use openrust::container::{Container, ParameterKey, ParameterOverrides, ParameterValue};
use std::sync::Arc;

# async fn example(container: &Container) -> openrust::Result<Arc<String>> {
let mut parameters = ParameterOverrides::new();
parameters.insert(ParameterKey::name("title"), ParameterValue::new(String::from("月报")));
container.resolve_with::<String>(parameters).await
# }
```

类构造器按精确参数名读取覆盖值。用 `ParameterValue::null()` 为允许空值的槽明确传空，用 `ParameterValue::new(Arc<T>)` 覆盖类依赖。类型不符会在业务构造器执行前报错。显式工厂可通过 `Resolver::parameters()` 读取整个集合，包括整数键。

非空参数集合会为这次调用独立构造对象，绕过共享实例与缓存，并且不登记普通解析历史。普通嵌套解析使用独立的空参数集合。参数帧在调用结束或取消时失效。可通过 `resolve_key_with` 将同一规则用于具名键、别名和 trait 视图。

### 为消费类指定依赖

在 Provider 的 `register` 中通过 `when` 选择构造消费者。使用 `needs` 指定类依赖，使用 `needs_parameter` 指定构造参数名称。Laravel 的 `$timeout` 对应 Rust 声明中的 `"timeout"`。

```rust
use openrust::container::{ParameterValue, ServiceKey};
use openrust::foundation::RegisterContext;

trait ReportStore: Send + Sync {}
struct ArchiveStore;
struct ArchiveReport;

fn register(app: &RegisterContext<'_>) -> openrust::Result<()> {
    app.when(ServiceKey::typed::<ArchiveReport>())?
        .needs(ServiceKey::typed::<dyn ReportStore>())
        .give(ServiceKey::typed::<ArchiveStore>())?;
    app.when(ServiceKey::typed::<ArchiveReport>())?
        .needs_parameter("timeout")
        .give_value(ParameterValue::new(30_u32))
}
```

本例要求 `ArchiveReport` 有已登记的构造描述，且 `ArchiveStore` 通过绑定或构造描述公开 `dyn ReportStore` 视图。也可用 `give_factory` 直接登记异步实现。上下文实现优先于该抽象键的普通共享实例，并且不填充该键的缓存或普通解析记录。重定向到独立服务键时，被选服务仍按自己的生命周期解析。

构造参数先使用调用级覆盖，随后检查消费类的上下文规则，再决定是否采用参数默认值。用 `give_config::<T>` 可从当前配置仓储按声明类型读取值，缺失时使用显式默认值。用 `give_many` 为类变参提供按顺序解析的服务键，对应 Laravel 的类数组实现。也可使用 `give_tagged` 或 `give_variadic_factory`。容器无法构造成员时，变参回退为空序列。工厂返回的业务错误仍会传给调用者。Rust 构造描述通过 `ConstructorParam::variadic` 声明变参，并在构造回调中通过 `ResolvedArgs::values` 按顺序读取。普通服务工厂内部的解析有自己的构造上下文，避免套用外层消费类的规则。

### 将服务键绑定到编译类

在 Provider 中使用 `bind_class` 登记类绑定，用 `singleton_class` 或 `scoped_class` 选择共享生命周期。构造器和可返回的 trait 视图来自已登记的编译类描述。下面将业务接口指向具体类的符号。

```rust
use openrust::container::ServiceKey;
use openrust::foundation::RegisterContext;

trait ReportStore: Send + Sync {}

async fn register(app: &RegisterContext<'_>) -> openrust::Result<()> {
    app.bind_class::<dyn ReportStore>(
        ServiceKey::typed::<dyn ReportStore>(),
        ServiceKey::named("app::ArchiveStore"),
    )
    .await
}
```

编译类符号与其类型键视为同一个类。同键绑定直接构造该类并读取当前构造参数帧，普通解析在外层应用所选生命周期和装饰器。异键绑定将本次工厂参数传给具体键的解析流程，具体键继续使用自己的生命周期、装饰器和解析历史。构造器的普通依赖各自使用空参数帧。

#### 按应用环境声明绑定

在接口描述符上调用 `.with_bind(Bind::for_environments(ServiceKey::typed::<LocalStore>(), ["local"]))` 声明实现。`Bind::new(ServiceKey::typed::<DefaultStore>())` 声明兜底。实现应提供编译描述和所需的 trait 投影。正式应用从启动配置的 `app.env` 设置环境，裸容器可调用 `set_binding_environment`。

环境模式按声明顺序匹配，采用大小写敏感的 `*` 通配。首个普通匹配优先于兜底，多个兜底取首个。显式工厂、已有实例和消费类的上下文绑定沿普通规则解析。带调用参数的解析会跳过仅有的实例，此时匹配的声明可登记真实工厂。

首次匹配后，`get_bindings()` 可看到该绑定，后续解析沿已登记工厂运行。构造失败后仍可重试同一工厂。未安装环境时可在安装后重试。已配置环境的一次无匹配查找会记住结果。示例模块的 `/api/example/environment-binding` 演示测试环境和兜底实现。 [Laravel Bind](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Bind.php) · [Laravel 容器](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php#L978-L1068)

#### 按条件声明绑定

在同一个接口描述符上附加 `BindWhen::new(ServiceKey::typed::<LocalStore>(), |resolver| async move { resolver.is_bound::<LocalFlag>() })`，并用 `.with_bind_when(...)` 登记。条件拿到当前作用域的 `Resolver`，可读取配置和异步解析其他服务。条件与普通环境 `Bind` 按声明顺序执行，首个命中项生效，通配兜底最后考虑。即使裸容器尚未安装环境，条件仍可执行。

条件全部未命中时，下一次独立解析可以重新判断。条件返回错误时，本次原样返回错误并记住已检查状态。条件内部用同一解析器重入当前服务时，继续查找普通构造来源。登记成功后，后续解析沿用真实工厂。并发首次求值共享一次在途检查，取消后等待者可以重新检查。

可调用 `resolver.bind::<T, _>(factory).await?` 在同一容器登记工厂。条件登记当前键后，回调内的普通解析可读取新工厂。若声明最终命中，以声明工厂为准。若未命中，本次尝试构造原抽象，后续解析可使用条件登记的工厂。若条件失败，本次返回原错误，已经登记的工厂仍可用。独立任务重绑会使旧条件结果失效。示例模块的 `/api/example/environment-binding` 在 `conditional` 环境演示同键登记后由命中声明选定实现。 [Laravel 容器](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php#L995-L1066)

#### 声明共享生命周期

在编译类描述符上调用 `.with_singleton()` 或 `.with_scoped()`，分别对应 Laravel 的 `Singleton` 和 `Scoped` 类属性。两者都声明时采用单例。单例归应用容器，作用域实例归当前请求、任务或显式作用域。一次关闭或重置只移除其管理的作用域实例。

```rust
use openrust::container::ClassDescriptor;
use std::sync::Arc;

struct CurrencyTable;
struct RequestAudit;

let descriptors = [
    ClassDescriptor::concrete::<CurrencyTable>("App/CurrencyTable", vec![], |_| {
        Ok(Arc::new(CurrencyTable))
    }).with_singleton(),
    ClassDescriptor::concrete::<RequestAudit>("App/RequestAudit", vec![], |_| {
        Ok(Arc::new(RequestAudit))
    }).with_scoped(),
];
```

通过 Provider 的类描述目录登记这些定义。直接解析具体类时，容器通过普通构造生成共享实例，`get_bindings()` 中只列实际登记的工厂。若接口通过 `Bind` 或 `BindWhen` 选中实现，容器按接口上的生命周期登记工厂。选中的具体类也可以独立声明自己的生命周期。

调用级参数和上下文覆盖使用当前构造，绕过该服务键的共享缓存。显式登记的共享工厂或实例沿自己的规则处理。普通显式工厂可以由具体类或抽象类的声明补足共享生命周期。示例模块的 `/api/example/environment-binding` 通过应用与请求计数展示两个生命周期。 [Laravel 容器](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php#L255-L315)

### 应用启动回调

在 `ApplicationDefinition::configure_providers` 中用 `builder.booting` 或 `builder.booted` 登记应用回调。所有 Provider 完成 `register` 后运行 `booting`，所有 Provider 完成 `boot` 后运行 `booted`。回调通过 `BootContext` 访问已登记服务、贡献命令及登记最终校验。`booted` 回调先于最终配置冻结和依赖验证。 [Laravel Application::boot](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Application.php#L1128-L1206)

`booting` 阶段追加的同阶段回调继续按顺序执行。`booted` 阶段追加的回调立即执行一次，随后按队列顺序再执行一次。阶段结束后，通过 `app.booted(...).await` 登记的回调只立即执行一次。`app.flush()` 清除已登记的启动回调，应用保留已启动状态。

### 按服务键延迟装载 Provider

让服务提供者实现 `openrust::foundation::DeferrableProvider`，并在 `provides()` 返回它提供的 `ServiceKey::typed::<T>()` 或 `ServiceKey::named("package.service")`。在应用装配中调用 `builder.deferred_provider(|| MyProvider)`。模块可在其 `register` 中调用 `app.deferred_provider(|| MyProvider)?`。工厂随应用编译，注册清单时无需创建 Provider 实例。

容器先展开别名。已有共享实例时可直接解析。首次装载移除触发键，并执行 Provider 的 `register`。应用已启动时，等待同一 Provider 的请求在 `boot` 完成后继续。将路由放在启动期 Provider 中，处理器可通过 `Inject<T>` 首次解析延迟服务。示例使用 `GET /api/example/deferred-greeting`。可用 `app.is_deferred_service(&key)` 查询精确清单键，或用 `app.load_deferred_providers().await?` 显式装载剩余项。

也可在 `when()` 返回事件的稳定名称或通配模式。照常通过 `Events` 派发。启动前触发会登记 Provider，并在正常启动遍历中执行一次 `boot`。启动后触发会立即登记并启动。事件保留服务键。派发或 `until` 返回的 `DataValue::Object` 可下转为 `RegisteredProvider`。示例使用 `POST /api/example/deferred-greeting-event`。

[Laravel ProviderRepository 的事件装载](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/ProviderRepository.php#L48-L115) · [Laravel Application 的延迟服务实现](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Application.php#L992-L1100)

### 编译服务清单

普通应用在 Provider 注册前读取 `bootstrap/cache/services.json`。首次启动或有序 Provider 声明变化时，应用原子重建文件。数据记录 eager Provider、延迟服务和事件。Rust 类型键和工厂从当前二进制取得，文件内容始终按编译声明校验。模块在自身 `register` 中登记的延迟 Provider 仍由当前启用的模块决定。

默认缓存目录随项目生成。可用 `APP_SERVICES_CACHE` 覆盖文件路径。相对路径以应用根目录为准，父目录须存在。应用自定环境变量前缀时使用同一前缀。损坏或无法读取的文件会使启动失败。

运行 `./artisan clear-compiled` 可清除选定的服务清单和包清单。包清单路径默认是 `bootstrap/cache/packages.json`，可用 `APP_PACKAGES_CACHE` 覆盖。配置缓存由 `config:clear` 单独管理。 [Laravel ProviderRepository](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/ProviderRepository.php#L48-L185)

在应用源码目录运行 `./artisan package:discover`，为当前 Cargo 目标和应用特性重建清单。`--preview` 输出候选 Rust 源码和 Facade 别名，`--check` 只读校验已安装清单。普通构建校验清单输入，并通过 `ApplicationDefinition::package_providers` 把包 Provider 加入同一次应用启动。 [CLI 用法](../../../tools/cli/README.md#provider-包发现) · [Laravel PackageDiscoverCommand](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Console/PackageDiscoverCommand.php)

包清单中的 `PackageProvider` 记录具体 Provider 及 Cargo 包来源。启动清单按框架默认项、发现包和显式应用项排列。注册前用 `ServiceProvider::dependencies()` 稳定排序包段。跨包的同一类型冲突会报告双方来源。显式清单的相对顺序及普通登记的同类型去重语义沿用原规则。 [Laravel Application](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Application.php#L821-L843)

### Provider 实例启动回调

在 `RegisterContext` 或当前 Provider 的 `BootContext` 上使用 `provider_booting`、`provider_booted`。每个实例依次执行自己的 `provider_booting` 队列、`boot` 和 `provider_booted` 队列。阶段内追加的回调排在队列末尾。强制重新登记使用新实例自己的回调。启动阶段与运行时登记遵循同一顺序。 [Laravel ServiceProvider](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Support/ServiceProvider.php#L103-L154)

回调接收 `BootContext`，可用 `context.resolve::<T>().await?` 从同一容器获取依赖。示例模块先在 `boot` 登记缓存驱动，再用 `provider_booted` 补充可选的默认 store。

### 应用终止回调

用 `builder.terminating`、Provider 上下文的 `terminating` 或 `app.terminating` 登记应用回调。异步回调接收 `ApplicationState`，用 `state.resolve::<T>().await?` 访问当前入口的服务。示例模块在 `boot` 登记回调，并在入口完成时写入 trace。 [Laravel Application::terminating 与 terminate](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Application.php#L1457-L1478)

回调按登记顺序执行，本轮中新登记的回调追加到队尾。首个错误中止该轮。显式 `app.terminate().await` 每次从队首执行，`app.flush()` 清空队列。

控制台在成功、帮助或错误退出后等待回调，命令本身已有的错误优先返回。HTTP 响应体交给传输层后，在原请求作用域启动受管回调，失败记入日志。流提前截断或报错时直接回收请求作用域。进程资源通过 `app.shutdown(...)` 回收。 [Laravel 请求和命令入口](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Application.php#L1224-L1251)

### 内核终止事件

HTTP 和控制台在应用回调前派发同一个 `openrust::foundation::events::Terminating`。可以用 `EventsServiceProvider::listen::<Terminating, _>` 监听，也可以用完整名称 `Illuminate\Foundation\Events\Terminating` 或通配模式监听。监听器报错会中止本轮终止流程。停止事件传播只影响后续监听器，应用回调仍会执行。显式调用 `app.terminate()` 直接运行应用回调。 [Laravel HTTP 内核](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Http/Kernel.php#L211-L218) [Laravel 控制台内核](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Console/Kernel.php#L222-L228)

### HTTP 终止中间件

实现 `openrust::http::middleware::TerminableMiddleware`，在 Provider 的 `register` 阶段绑定类型。用 `BootContext::middleware_alias_terminable::<M>` 登记路由别名，或用 `BootContext::http_middleware_terminable::<M>` 登记全局中间件。别名参数只传给 `handle`。`terminate` 再次通过容器解析实例，普通绑定重新构造，单例沿用原实例。 [Laravel 终止中间件](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/middleware.md#terminable-middleware)

响应正常完成后，内核依次派发 `Terminating`、执行选中路由的终止中间件、执行全局终止中间件，最后运行应用回调。选中路由的中间件即使在请求阶段被前一层短路，仍会进入终止阶段。未匹配路由时只执行全局项。任一步失败都会中止本轮，已完成的响应保留。 [Laravel HTTP 内核](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Http/Kernel.php#L211-L262)

`terminate` 接收请求和最终响应头。请求扩展包含 `RequestAttributes`，已匹配时包含 `Route`。正文按 Axum 的所有权模型交给传输层。全局项沿共享 HTTP layer 的执行顺序，后登记的层在外。示例模块的响应标记中间件保留原标记头，并在终止时记录方法与状态。

### 生命周期时长处理器

在 Provider 的 `boot` 中调用 `when_request_lifecycle_is_longer_than` 或 `when_command_lifecycle_is_longer_than` 登记处理器。也可在运行期通过 `ApplicationState` 登记。阈值接受 `std::time::Duration`、`LifecycleThreshold::Milliseconds` 或 `LifecycleThreshold::At`。绝对时刻按登记时距目标的 Unix 整秒差折算。

内核在终止事件、中间件和应用回调结束后采样一次耗时。超过各自阈值的处理器依登记顺序执行，首个错误中止后续处理器。开始时间按应用时区显示，耗时由单调时钟计量。请求处理器借用已交付的请求和响应头。命令处理器可读取入口命令名、参数与运行状态。两者执行时仍可解析入口作用域服务。显式调用 `app.terminate()` 只运行应用回调。

```rust,ignore
app.when_request_lifecycle_is_longer_than(std::time::Duration::from_secs(1), |_, request| {
    Box::pin(async move {
        tracing::debug!(status = request.response.status().as_u16(), elapsed = ?request.elapsed);
        Ok(())
    })
});
```

在活跃入口可用 `ApplicationState::request_started_at` 或 `command_started_at` 读取 UTC 开始时刻。并行入口各自计时，取消的请求或中断的响应流沿用资源回收路径。 [Laravel HTTP 内核](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Http/Kernel.php#L198-L278) [Laravel 控制台内核](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Console/Kernel.php#L207-L258)

### 排除中间件

用 `Router::without_middleware("audit")` 排除最近声明路由的中间件，或用 `RouteGroup::without_middleware("audit")` 排除组内各路由。裸别名匹配所有参数版本，`audit:read` 只匹配该版本。排除命名组时会展开其成员。排除同时作用于处理与终止阶段，全局层继续运行。示例路由 `/api/example/middleware/without-marker` 排除了组内的响应标记。 [Laravel 路由中间件排除](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/middleware.md#excluding-middleware)

隔离测试可在构建前调用 `TestAppBuilder::without_middleware()`，跳过路由中间件、应用全局层和 CORS。`with_middleware()` 恢复默认链。两种模式都保留协议校验、作用域回收、`Terminating` 事件和应用终止回调。

对通过容器登记的 `TerminableMiddleware`，测试可在 `TestAppBuilder` 或运行中的 `TestApp` 上调用 `without_middleware_type::<Audit>()`。同类型的全局登记、别名和参数版本都会直接进入下一层，省去该类型的解析、处理与终止。调用 `with_middleware_type::<Audit>()` 恢复后续请求的实际中间件。已返回的流式响应沿原决定完成，`reset` 重用蓝图配置。任意 Tower layer 仍使用各自的登记和排除方式。 [Laravel 测试中间件替换](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Testing/Concerns/MakesHttpRequests.php#L183-L228)


需要在已构建的应用中切换时，通过应用容器设置具名绑定 `middleware.disable`。只有布尔值 `true` 会跳过中间件，设置为 `false` 可恢复。这个开关作用于当前应用，适合由测试装配或可信应用代码控制。应用全局层（含 CORS）、匹配路由的中间件和终止中间件分别在各阶段读取该绑定。终止阶段在 `Terminating` 事件之后读取。作用域、协议校验及应用终止回调仍由内核负责。 [Laravel HTTP 内核](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Http/Kernel.php) [Laravel 路由分派](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Routing/Router.php#L824-L835)

```rust,ignore
use openrust::container::ServiceKey;
use std::sync::Arc;

state.container()
    .instance_key(ServiceKey::named("middleware.disable"), Arc::new(true))
    .await?;
```

### 通过容器调用 callable

给 `Container::call` 传入编译期参数声明、调用级实参和回调。回调在 `CallArguments::resolved` 中读取声明槽位，未消费的实参按输入顺序保存在 `remaining`。声明末尾的变参槽会先接收已解析值，再接收剩余实参。

```rust
use openrust::container::{ConstructorParam, ContainerBuilder, ParameterOverrides};
use std::sync::Arc;

# async fn example() -> openrust::Result<()> {
let mut builder = ContainerBuilder::new();
builder.instance(Arc::new(String::from("OpenRust")));
let container = builder.build();
let name = container
    .call(
        &[ConstructorParam::class::<String>("name")],
        ParameterOverrides::new(),
        |args| {
            let name = args.resolved.service::<String>(0);
            Box::pin(async move { name })
        },
    )
    .await?;
assert_eq!(name.as_str(), "OpenRust");
# Ok(())
# }
```

参数按同名实参、上下文属性、类名实参、服务或默认值的顺序选择。用 `ParameterValue::null()` 传递显式空值。类名实参的键取自 `ServiceKey::typed::<T>().name()`，类值通过 `ParameterValue::service(arc)` 提供。类默认值仅在对应服务未绑定时采用。实参不会继承到所解析服务的工厂。

在请求和命令中用 `ApplicationState::call`，在自管生命周期内用 `Scope::call`，在已有解析链上用 `Resolver::call`。Rust 回调可以调用普通函数或已取得实例的方法，方法和参数由编译器检查。示例路由 `/api/example/container-call` 注入模块的服务并提供命名实参。 [Laravel BoundMethod](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/BoundMethod.php)

### 调用与绑定对象方法

通过 `ContainerBuilder`、`Container` 或 Provider 的 `RegisterContext` 登记 `register_method::<Receiver, Output>`。登记项包含精确方法名、参数签名和 Rust 实现。用 `bind_method::<Receiver, Output>` 替换调用行为。替代回调接收对象和当前解析器，直接执行，无需解析默认签名。后登记的绑定对后续调用生效，正在执行的回调保持原登记。

- `MethodCall::on(object, "run")` 使用现有对象并检查方法绑定。
- `MethodCall::<Receiver>::class("run")` 先用容器解析对象，再分派指定方法。方法名也可以由调用方提供默认值。
- `MethodCall::<Receiver>::invokable()` 使用已声明的 `__invoke` 作为类入口。
- `MethodCall::direct(object, "run")` 在对象类型上下文中执行声明的方法，绕过替代绑定。

将入口传给 `call_method`。根容器、自管作用域、解析器和应用入口提供同一调用方式。显式对象入口可使用对象类型的上下文绑定。类入口沿用调用前的上下文。Rust 使用编译类型和方法名表达类入口，结果类型由调用方声明。示例路由 `/api/example/container-method` 展示同一个方法的直接调用和替代绑定。 [Laravel 方法绑定](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php#L391-L435) · [Laravel 类入口](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/BoundMethod.php#L23-L88)

### 静态调用的声明类型

关联函数通过 `call_static` 执行。传入 `StaticCall::direct::<Declaring>()` 后，参数在声明类型的上下文中解析。回调接收声明槽位及当前 `Resolver`，可用它继续嵌套调用。静态入口无需构造接收者。

需要模拟命名静态 callable 时，用 `StaticCall::named::<Called, Declaring>("render")`。命名入口以调用类型 `Called` 和方法名查找 `bind_static_method` 的登记，以声明类型 `Declaring` 解析上下文。两者通常是同一 Rust 类型。命中时替代回调取得调用类型键及解析器，并跳过默认参数解析。直接入口始终执行传入的回调。

同一请求可以依次使用直接和命名静态入口。示例路由 `/api/example/container-method` 分别返回两个入口的结果。 [Laravel 调用上下文](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php#L736-L770) · [Laravel 绑定分派](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/BoundMethod.php#L74-L100)

### 清空容器绑定

在隔离测试或重新装配容器时，调用 `container.flush()?` 清除显式绑定、别名、解析记录和全部作用域的共享实例缓存。环境、编译类描述、标签、上下文规则、持久装饰器与回调仍可用。清空后，首次解析可重新发现 `Bind` 和 `BindWhen` 声明。 [Laravel Container::flush](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php#L1800-L1811)

已取得的 `Arc` 和原工厂快照仍可使用。清空前已准入的普通工厂可以完成，结果只交给当时的调用方。新解析使用清空后的目录。作用域和受管任务仍由各自的生命周期关闭。运行中的应用依赖 Provider 登记的绑定，应在隔离测试或独立容器上使用 `flush`。

在可独占访问的应用上，`app.flush()?` 还会撤销有效的 Provider、普通解析和重绑回调，以及这些 Provider 贡献的命令和宿主扩展。应用保留已启动状态、配置快照和异步资源的所有权。可以显式重新登记 Provider 来执行 `register` 和 `boot`，旧的命令快照或路由器仍由原持有者管理，异步资源通过 `app.shutdown(deadline).await` 回收。 [Laravel Application::flush](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Application.php#L1699-L1717)

### 在解析阶段登记回调

Provider 的 `register` 可用 `before_resolving`、`resolving` 和 `after_resolving` 追加回调。传入 `None` 选择全局阶段，传入 `Some(ServiceKey)` 选择服务。登记时将别名展开为当前目标。每个阶段先执行全局回调，再按首次登记各个键的顺序执行匹配的分组。同一个键内按追加顺序执行。

`before_resolving` 在选择工厂和读取共享缓存之前执行。回调通过 `event.key()` 读取规范服务键，通过 `event.parameters()` 读取当前实参，也可通过 `event.container()` 在选择来源前登记绑定。回调中改写别名影响后续解析，本次查找继续使用进入回调前确定的目标。构造和装饰完成后依次执行 `resolving` 与 `after_resolving`。共享缓存命中仍执行前置回调。两个后置阶段由实际完成构造的解析执行一次。

```rust
use openrust::container::ServiceKey;
use openrust::foundation::RegisterContext;

struct Report;

fn register(app: &RegisterContext<'_>) -> openrust::Result<()> {
    app.after_resolving(Some(ServiceKey::typed::<Report>()), |event, _| {
        Box::pin(async move {
            if let Some(report) = event.value::<Report>()? {
                // 在交付前读取已构造的 Report。
                let _ = report;
            }
            Ok(())
        })
    })
}
```

后置回调可以匹配抽象服务键，也可以匹配对象的具体类型和编译期声明的 trait 视图。`event.value::<T>()` 返回同一个对象的可用视图。要在接口绑定的回调中读取具体类型，请为具体类登记构造描述和对应的 trait 投影。仅提供已擦除的 trait 对象时，可读取该对象声明的视图。回调返回错误时，当前解析返回该错误。

异键类包装器只静默具体服务这一层的三个解析阶段，外层和普通构造依赖仍运行各自的回调。直接调用原始工厂时也跳过原绑定键的解析回调，工厂主动解析的依赖仍使用普通阶段。[固定 Laravel 解析回调行为](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Container/ResolvingCallbackTest.php)

### 构造属性与自构造

通过 `ConstructorParam::with_attribute` 将属性附在编译参数上。`ClassAttribute::contextual` 接受创建新属性实例的函数，属性实现 `ContextualAttribute`。Provider 可用 `when_has_attribute::<A>` 覆盖 `A` 的 `resolve` 方法。`ClassAttribute::marker` 适合只参与后置回调的属性。

- 显式具名参数优先，并跳过该参数的属性。普通构造只调用首个上下文属性的处理器，返回 `AttributeValue::One(ParameterValue::null())` 时继续从上下文绑定、依赖和默认值选取。
- 选值后按声明顺序执行属性自身的 `after`，再执行 Provider 通过 `after_resolving_attribute::<A>` 登记的回调。构造变参作为一组传入 `after`。处理器使用 `AttributeValue::many` 返回多项。类属性使用 `ClassDescriptor::with_attribute`，在构造栈退出后、装饰和普通解析回调前执行。
- 回调可按属性类型或值的已编译服务视图匹配。读取属性用 `event.attribute().downcast::<A>()`，读取对象用 `event.service::<T>()`。处理器提供类对象时用 `ParameterValue::service(arc)` 保留对象的编译类型。回调可通过同一 `Resolver` 解析当前作用域内的服务。

类型实现 `SelfBuilding` 后，使用 `ClassDescriptor::self_building` 分别声明普通构造参数和 `new_instance` 方法参数。外层 `resolve_with` 的具名覆盖不参与方法参数选择。方法属性返回的显式 null 作为实参传入，方法类变参逐项执行 `after`。`AttributeParameter::is_new_instance()` 可区分两个入口。需要替换整个方法时，可用 `bind_new_instance::<T>`，该绑定接管方法及其参数注入，类属性仍照常派发。

活动的 `new_instance` 再次解析同一类时回退普通构造。若该调用会等到自己的祖先共享构造，内层直接构造一次，由外层持有的共享尝试统一发布。`Resolver::build_class` 在当前作用域直接构造编译类，跳过显式绑定和共享缓存，仍执行类属性。属性身份和实参类型来自 Rust 编译声明。 [Laravel 构造与属性源码](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php) · [Laravel 方法参数注入](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/BoundMethod.php)

#### 内置通用属性

从 `openrust::container::attributes` 导入 `Config`、`Give` 和 `Tag`，再通过 `ClassAttribute::contextual` 附到参数声明。Provider 的 `when_has_attribute::<A>` 仍可为指定类型覆盖内置处理器。

- `Config::<T>::new(key).with_default(value)` 从当前配置仓储读取配置，并按 `T` 解码。只有缺失键才使用属性默认值。显式 null 继续遵循构造参数或 `new_instance` 的选参规则。
- `Give::<T>::new(key).with_parameters(overrides)` 按服务键或别名解析实现。仅该属性的实参传给实现，非空实参独立构造。声明的同对象 trait 视图可作为 `T` 注入，后置回调仍可观察具体对象。
- `Tag::<T>::new(tag)` 注入 `Tagged<T>`。`len()` 返回取得时的计数。每次调用 `stream()` 都返回独立的异步流，首次轮询时读取标签列表，再按登记顺序逐个解析，重复项保留。取得时不存在的标签固定为空，已登记的标签可以在新一轮看到追加。服务错误会结束当前一轮，重试可重新调用 `stream()`。

下面声明一个从 `example.greeting` 读取的参数。

```rust
use openrust::container::{ClassAttribute, ConstructorParam, attributes::Config};

let greeting = ConstructorParam::scalar::<String>("greeting").with_attribute(
    ClassAttribute::contextual(|| Ok(Config::<String>::new("example.greeting"))),
);
```

Rust 服务构造可异步挂起，因此 `Tagged<T>` 使用异步流，并在每轮沿用创建它时的 Resolver 和作用域。固定来源见 [Config](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Config.php)、[Give](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Give.php) 和 [Tag](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Tag.php)。

#### 请求属性与路由参数

在 middleware 中通过 `RequestInfo::attributes().insert("source", ParameterValue::new(value))` 写入本次请求的服务端属性，再用 `RequestAttribute::new("source")` 注入构造参数。属性名按完整字符串匹配。类型化服务使用 `ParameterValue::service(arc)` 保存对象身份。查询参数、请求头和正文仍由各自的提取器读取。 [Laravel RequestAttribute](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/RequestAttribute.php)

`RouteParameter::default()` 读取同名构造参数的路由值，`RouteParameter::new("article")` 显式选择名称。属性使用当前匹配路由和可信 resolver，支持已解码的域名及路径捕获。可通过 `Route::with_bound_parameter` 提供已绑定对象，`bound_parameter` 读取该绑定，`parameter` 和 `parameters` 仍提供原始 JSON 捕获。 [Laravel RouteParameter](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/RouteParameter.php)

两种属性在实际 HTTP 请求的容器作用域中取值，响应体生成期间沿用同一作用域。缺少属性、参数或 HTTP 作用域时返回空值，由容器继续处理默认值或可空参数。

#### 日志属性

默认 Provider 在应用容器中登记 `LogManager`。用 `Log::default()` 注入默认频道的 `Logger`，用 `Log::new("single").named("orders")` 选择具名频道并为本次注入重命名。重命名复用相同输出目标和级别过滤，原频道仍使用自己的名称。把属性附到 `ConstructorParam::class::<Logger>("logger")` 即可。 [Laravel Log 属性](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Log.php)

`config/log.rs` 通过 `logging` 区段配置频道。`LOG_CHANNEL` 默认选择 `stack`，`LOG_STACK` 默认包含 `single`，`LOG_LEVEL` 默认是 `debug`。已支持 `single`、`stderr`、`null` 和顺序写入的 `stack`。默认文件为应用根的 `storage/logs/laravel.log`。频道创建失败会写入应急文件，并返回应急 logger；应急文件也无法写入时会返回错误。

日志通过 `Logger::log(level, message, context).await` 写入。文件按行记录时间、logger 名称、级别和消息，结构化上下文以 JSON 附在末尾。第三方通过 `LogManager::extend` 为频道配置提供 `LogSink`。框架诊断和遥测仍由 `tracing` 收集，受 `RUST_LOG` 控制。

#### 日志上下文属性

默认 `ContextServiceProvider` 在容器中按作用域登记 `openrust::logging::context::Repository`。用 `add` 写入普通 JSON 字段，用 `add_hidden` 写入隐藏字段。在构造参数上附加 `Context::<String>::new("request.trace")` 可按完整键读取普通字段，加 `.hidden()` 可改读隐藏字段。缺失值和 JSON `null` 使用 `with_default` 或 `with_default_using` 提供的默认值。`false`、`0` 和空字符串按声明类型读取。JSON 无法转换为声明类型时，容器返回类型错误。

日志记录的 `extra()` 读取同一应用当前入口已初始化的普通字段，独立于 `context()` 接收的单次日志参数。占位符使用单次日志参数。单文件和标准错误频道分别序列化两组非空字段，连续数字键的 `extra` 写为 JSON 数组。隐藏字段可通过仓储和属性显式读取。 [Laravel Context](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Context.php) · [Laravel ContextLogProcessor](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Log/Context/ContextLogProcessor.php)

Rust 接口使用 JSON 表示可序列化值。规范数字键按 Laravel 合并规则重新编号，其他字符串键按字面保存。队列与命令传播、生命周期事件、栈操作和完整仓储方法仍待对应。

#### 缓存属性

`container::attributes::Cache` 将应用缓存管理器中的 `CacheStore` 注入构造参数。在 `ConstructorParam::class::<CacheStore>("cache")` 上附加 `ClassAttribute::contextual(|| Ok(Cache::default()))` 使用默认 store，`Cache::new("local")` 则按配置的精确名称选择。空字符串和未知名称沿缓存管理器报错。

在属性上调用 `.memoized()` 后，同一作用域内对同一 store 的注入共享命中与未命中。通过该视图写入或删除时，下一次读取重新访问后端。HTTP 请求、队列任务和显式容器作用域各自解析自己的 memo 实例。显式使用解析器时也可调用 `openrust::cache::Cache::memo(&resolver, Some("local"))`。[Laravel Cache 属性](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Cache.php)

#### 数据库与文件系统属性

`Database` 和 `DB` 注入由 `Databases` 管理的 `Db`，`Storage` 注入由 `FilesystemManager` 管理的 `FilesystemAdapter`。在应用中启用对应 Provider，然后把属性附到 `ConstructorParam::class::<Db>` 或 `ConstructorParam::class::<FilesystemAdapter>` 上。`Database` 与 `DB` 可分别登记属性处理器和后置回调。

`::default()` 选择当前管理器的默认连接或磁盘。`::new("reporting")`、`::new("local")` 选择具名资源。空字符串和字符串 `0` 也会选择默认项，其他名称原样传给管理器。未知名称沿管理器原错误返回。注入的 Rust 句柄共享已有连接池或磁盘 backend。Rust 枚举由调用方转换为名称。

数据库名称的 `::read`、`::write` 和 `::direct` 路由属于数据库管理器的后续对应项。当前属性使用已配置的普通连接名。 [Laravel Database 属性](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Database.php) · [Laravel DB 属性](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/DB.php) · [Laravel Storage 属性](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Storage.php)

#### 认证属性

启用 `auth` feature 并注册 `AuthServiceProvider` 后，可将 `Auth::default()` 或 `Auth::new("web")` 附到 `ConstructorParam::class::<dyn Guard>("guard")`。同一请求内按名复用 Guard，可通过 `Guard::as_stateful` 获取有状态能力。

`Authenticated` 和 `CurrentUser` 将 `Arc<dyn Authenticatable>` 注入类参数。`::default()` 省略 Guard 名称，`::new("web")` 显式传入名称。两种属性调用同一个请求级用户解析器，但各自可以登记上下文处理器和后置回调。访客返回 null，构造器按普通依赖和默认值继续解析。可空账户参数可声明 `.nullable().with_default(|| Ok(ParameterValue::null()))`。未知 Guard 或读取错误沿解析器传播。

`AuthManager::resolve_users_using` 接收属性声明中的原始选择值。内置属性选择将省略、空字符串和字符串 `0` 交给默认 Guard，`should_use` 同时恢复内置解析器。显式配置名由 `AuthLayer::guard` 按原名选择。自定义无状态 Guard 可直接读取当前 HTTP 请求。Session Guard 需要路由实际启用 Web/Session 中间件。示例模块的 `/api/example/account/session/status` 在 Web profile 展示三种属性。 [Laravel Auth](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Auth.php) · [Laravel Authenticated](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/Authenticated.php) · [Laravel CurrentUser](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Attributes/CurrentUser.php)

Laravel 的 `AuthServiceProvider` 还默认绑定可空的 `Authenticatable` 接口。OpenRust 的容器服务绑定返回 `Arc<T>`，这项可空接口绑定仍待对应。Rust 枚举选择由调用方显式转为名称。

### 读取与调用原始绑定工厂

`Container::get_bindings()` 按登记顺序返回显式工厂条目的键、共享声明和 `FactoryHandle`。实例覆盖保留原工厂，绑定替换后已取得的句柄仍调用快照里的闭包或类绑定包装器。

```rust
use openrust::container::{Container, ServiceKey};
use std::sync::Arc;

async fn invoke(container: &Container) -> openrust::Result<Option<Arc<String>>> {
    let key = ServiceKey::named("reports.title");
    let Some(binding) = container.get_bindings()?.into_iter().find(|item| item.key == key) else {
        return Ok(None);
    };
    binding.concrete.invoke::<String>(container.factory_resolver()?).await.map(Some)
}
```

在已持有的 `Scope` 中可用 `scope.factory_resolver()` 取得该作用域的解析器。直接调用使用所传解析器的容器和资源作用域，跳过原绑定键的装饰、共享实例和普通解析记录。请求类型须匹配工厂声明或类描述的可用视图，容器在运行工厂或构造器前检查。

`invoke_with` 把实参交给原用户闭包的 `Resolver::parameters()`。同键类包装器直接构造，只读调用时已有的构造参数帧。异键包装器把实参交给具体键的解析，仍可能复用该键的共享实例或写入该键的解析历史。构造器或闭包主动解析的依赖继续走通常流程。

应用 Controller 放在 `app/http/controllers/`，通过 `routes/api.rs` 或 `routes/web.rs` 声明访问路径；应用 Provider 类型放在 `app/providers/`，由 `bootstrap/providers.rs` 登记。不要把这些应用职责放入框架 `src/`。

API 请求匹配当前配置前缀的无状态路由组，再进入处理函数。异常经公开 Handler 扩展渲染；根应用对配置的 API 范围选择 JSON 包装，Web 请求继续使用其内容协商与异常渲染流程。

Provider 顺序、模块加载和配置错误可阻止应用启动。修改模块持久状态只影响后续应用实例，不会热卸载当前 Provider 或路由；详见[模块状态](../modules/file-state.md)。

对应固定 Laravel 13.x 官方章节：[lifecycle](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/lifecycle.md)。本章仅说明已落地的当前入口及边界，尚未覆盖该官方章节全部能力。
