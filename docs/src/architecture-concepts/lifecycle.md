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

### 读取与调用原始绑定工厂

`Container::get_bindings()` 按登记顺序返回显式工厂条目的键、共享声明和 `FactoryHandle`。实例覆盖保留原工厂，绑定替换后已取得的句柄仍调用快照里的闭包。

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

在已持有的 `Scope` 中可用 `scope.factory_resolver()` 取得该作用域的解析器。`invoke_with` 把实参交给原闭包的 `Resolver::parameters()`。句柄在执行前按登记的输出类型校验请求类型，跳过原键的装饰、共享实例和普通解析记录。闭包内主动解析的依赖继续走通常的容器流程。

应用 Controller 放在 `app/http/controllers/`，通过 `routes/api.rs` 或 `routes/web.rs` 声明访问路径；应用 Provider 类型放在 `app/providers/`，由 `bootstrap/providers.rs` 登记。不要把这些应用职责放入框架 `src/`。

API 请求匹配当前配置前缀的无状态路由组，再进入处理函数。异常经公开 Handler 扩展渲染；根应用对配置的 API 范围选择 JSON 包装，Web 请求继续使用其内容协商与异常渲染流程。

Provider 顺序、模块加载和配置错误可阻止应用启动。修改模块持久状态只影响后续应用实例，不会热卸载当前 Provider 或路由；详见[模块状态](../modules/file-state.md)。

对应固定 Laravel 13.x 官方章节：[lifecycle](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/lifecycle.md)。本章仅说明已落地的当前入口及边界，尚未覆盖该官方章节全部能力。
