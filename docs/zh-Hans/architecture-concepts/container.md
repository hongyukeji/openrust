# 服务容器

## 绑定

在 Provider 的 `register` 中通过 `app.container()` 注册服务。`bind::<T, _>` 每次解析都会调用工厂，`singleton::<T, _>` 共享首次成功结果，`instance::<T>` 提供已有的 `Arc<T>`。工厂接收同一个容器，可显式解析依赖。

Rust 没有 PHP 的构造函数反射。将接口绑定为 `dyn Trait`，由工厂返回具体实现。Rust 在编译时校验接口实现，容器失败通过 `BindingResolutionError` 返回。未注册类型不会自动构造。

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};

trait Mailer: Send + Sync { fn send(&self) -> &'static str; }
struct SmtpMailer;
impl Mailer for SmtpMailer { fn send(&self) -> &'static str { "sent" } }

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singleton::<dyn Mailer, _>(|_| Ok(Arc::new(SmtpMailer)));
    app.alias(ServiceKey::typed::<dyn Mailer>(), "mailer")?;
    let mailer = app.get::<dyn Mailer>("mailer")?;
    assert_eq!(mailer.send(), "sent");
    assert!(Arc::ptr_eq(&mailer, &app.make::<dyn Mailer>()?));
    Ok(())
}
```

### 命名绑定与别名

使用 `bindNamed::<T, _>(name, factory)`、`singletonNamed::<T, _>(name, factory)` 或 `instanceNamed::<T>(name, instance)` 注册命名项。这些是同一操作的命名形式，因为 Rust 不能按参数数量重载方法。`get::<T>(name)` 解析命名项，在调用工厂前校验输出类型。类型键与命名键相互独立，即使显示名称相同也不会混淆。

`alias(abstract, alias)` 接收 `ServiceKey::typed::<T>()`、`ServiceKey::named(name)` 或字符串。别名可以指向其他别名或稍后才注册的项。`isAlias` 检查原始键，`getAlias` 跟随别名链，`has` 检查绑定或别名而不构造服务。名称精确匹配且区分大小写。在别名键上注册绑定或实例会替换该别名。重新绑定目标影响后续解析，已返回的 `Arc` 保持有效。

直接指向自身的别名会被拒绝。别名循环和工厂依赖循环返回错误。单例构造失败或 panic 不会缓存结果，后续调用可以重试。并发解析共享首次成功的单例。调用用户工厂前释放容器锁。

## 解析

使用 `make::<T>()` 按 Rust 类型解析，使用 `get::<T>(key)` 按名称或显式键解析。默认配置仓库绑定为 `dyn openrust::contracts::config::Repository`，并有 `config` 别名。每个应用拥有自己的容器，绑定和实例不是进程全局状态。

延迟服务、上下文绑定、扩展器和解析回调仍在 01-02 容器单元推进。上述绑定与解析能力不代表这些功能已经实现。

## 条件绑定与实例状态

`bindIf`、`singletonIf` 仅在类型键没有绑定、实例或别名时注册工厂，命名形式为 `bindIfNamed`、`singletonIfNamed`。存在性检查和写入是原子操作，被拒绝的工厂不执行，并在注册表解锁后释放。即使别名指向尚不存在的服务，也视为已有条目。

`isShared(key)` 检查原始键，不跟随别名。`resolved(key)` 跟随别名，在存在实例或工厂已经成功解析时返回 true。清除实例不会清除解析历史。提供实例不会执行或丢弃原工厂。

`forgetInstance(key)` 清除精确键对应的实例，不跟随别名。已有单例工厂随后可重新构造实例。`forgetInstances()` 清除全部现存实例，保留工厂、别名和解析历史。没有工厂的实例条目会变为未绑定；已经返回的 `Arc` 仍然有效。

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{Container as _, BindingResolutionError}};

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonIfNamed::<u32, _>("counter", |_| Ok(Arc::new(7)));
    app.bindIfNamed::<u32, _>("counter", |_| Ok(Arc::new(8)));
    assert!(app.isShared("counter"));
    assert!(!app.resolved("counter")?);
    let original = app.get::<u32>("counter")?;
    app.instanceNamed("counter", Arc::new(99_u32));
    assert_eq!(*app.get::<u32>("counter")?, 99);
    app.forgetInstance("counter");
    let rebuilt = app.get::<u32>("counter")?;
    assert_eq!(*rebuilt, 7);
    assert!(!Arc::ptr_eq(&original, &rebuilt));
    Ok(())
}
```

这些操作不会清除门面自己的根对象缓存，需要时另行调用门面缓存清除接口。作用域服务清理见下文。

## 作用域绑定

`scoped::<T, _>` 在一次执行生命周期中共享服务，命名形式为 `scopedNamed::<T, _>(name, factory)`。使用 `scopedIf`、`scopedIfNamed` 仅在原始键未绑定时注册。HTTP 处理器执行及每次 Artisan 分派自动进入生命周期，普通 `singleton` 实例仍由应用共享。

手动执行使用 Rust 生命周期辅助方法 `Container::scope(future)`。同一容器中的嵌套调用复用当前作用域，并发执行拥有独立缓存，即使运行在同一个 Tokio 线程中也不会混用。不同应用各自保存服务绑定及实例。

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::Container as _};

fn main() {
    let app = Container::default();
    app.scoped::<u32, _>(|_| Ok(Arc::new(7)));
    let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    runtime.block_on(async {
        let first = app.scope(async {
            let instance = app.make::<u32>().unwrap();
            assert!(Arc::ptr_eq(&instance, &app.make::<u32>().unwrap()));
            instance
        }).await;
        let second = app.scope(async { app.make::<u32>().unwrap() }).await;
        assert!(!Arc::ptr_eq(&first, &second));
    });
}
```

生命周期 Future 持有缓存引用，在执行完成、失败或取消并释放关联 Future 后释放。调用方可以持有超过作用域的 `Arc`，对象在最后一个引用释放后销毁。新作用域使用新的作用域实例，不复用容器根生命周期中已缓存的实例。

`forgetScopedInstances()` 清除当前生命周期的作用域缓存，保留其他活动生命周期及普通单例。在执行作用域之外调用，则清除根作用域实例。`forgetInstance(key)` 仍按原始键操作；`forgetInstances()` 同时清除当前作用域实例及应用的普通实例缓存。作用域服务的 `instance` 覆盖仅在当前生命周期生效，重新绑定仍是应用级注册操作。

容器清理不会清除门面独立缓存的根对象；需要在同一次执行中重新解析时，同时清除门面缓存。应用自行创建的 Tokio 任务不继承环境作用域，应显式传入服务，或为任务设置所需生命周期。流式正文可以通过持有已解析服务的 `Arc` 延续其使用。流式及终止回调接线仍属于 HTTP 交付单元。

## 上下文绑定与标签服务

`when::<Consumer>()?.needs::<Dependency>().give(factory)?` 为当前构造的消费者选择依赖。`whenNamed(name)` 和 `whenMany(keys)` 对应具名及多个消费者；`needsNamed::<T>("$parameter")` 对应显式命名的构造输入。Rust 没有 PHP 构造反射，工厂需显式解析依赖，登记的消费者身份对应 PHP 反射得到的类。

上下文规则只作用于最近的消费者，优先于默认实例、单例和作用域缓存，但不替换默认缓存，也不把依赖记为全局已解析。上下文工厂可以再次解析同一依赖取得默认绑定；实际递归工厂仍返回循环依赖错误。消费者别名在调用 `when` 时规范化，依赖别名在调用 `give` 时规范化，解析时还会检查后续增加的直接别名。

`give` 接收类型明确的工厂。`giveService(key)` 对应 Laravel 中传服务名的 `give`，目标须按预期返回契约登记；`Arc<Concrete>` 到 `Arc<dyn Trait>` 的转换在工厂中显式完成。`giveConfig(key, default)` 在解析依赖时读取当前 `config` 服务，`None` 对应 Laravel 省略默认值。键缺失时使用默认值，显式 JSON null 保持 null。反序列化会检查 Rust 声明类型，不匹配返回错误，不隐式执行 PHP 标量转换。

`tag(keys, tags)` 保留登记顺序和重复项。`tagged::<T>(tag)` 迭代到某项时才解析服务，每次迭代按原服务生命周期重新解析。未知标签得到空序列；已有序列的下一次迭代可看到之前追加的服务。`len()` 返回创建序列时固定的数量，对应 Laravel 可重复生成器的行为。`giveTagged` 收集为 `Vec<Arc<T>>`，未绑定或类型不符的项传播错误。

```rust
use std::sync::Arc;
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
};
struct Report {
    channel: Arc<String>,
    port: Arc<u16>,
    services: Arc<Vec<Arc<String>>>,
}
fn main() {
    let app = Application::configure(std::env::current_dir().unwrap()).create().unwrap();
    let container = app.container();
    let config = container.get::<dyn openrust::contracts::config::Repository>("config").unwrap();
    config.set("reports.port", 8080.into()).unwrap();
    container.singleton::<String, _>(|_| Ok(Arc::new("default".into())));
    container.instanceNamed("local", Arc::new(String::from("local")));
    container.tag(["local"], ["reports"]);
    container.bind::<Report, _>(|c| Ok(Arc::new(Report {
        channel: c.make()?, port: c.get("$port")?, services: c.make()?,
    })));
    container.when::<Report>().unwrap()
        .needs::<String>().giveService("local").unwrap()
        .needsNamed::<u16>("$port").giveConfig("reports.port", None).unwrap()
        .needs::<Vec<Arc<String>>>().giveTagged("reports").unwrap();
    let report = container.make::<Report>().unwrap();
    assert_eq!(&*report.channel, "local");
    assert_eq!(*report.port, 8080);
    assert_eq!(report.services.len(), 1);
    assert_eq!(&*container.make::<String>().unwrap(), "default");
}
```

## 容器解析事件

`beforeResolving`、`resolving` 和 `afterResolving` 用 `None` 登记全局回调，用 `Some(ServiceKey)` 选择服务身份。登记时规范化别名。回调接收 `ResolutionEvent` 与当前容器，返回 `Result<(), BindingResolutionError>`。实现提取旧版的事件视图与有序选择器设计，适配当前同步工厂。

执行顺序为全局 before → 指定 before → 构造 → 全局 resolving → 指定 resolving → 全局 after → 指定 after。before 回调可以登记尚未绑定的服务，每次查找都执行，缓存命中也执行。显式实例及共享缓存命中跳过构造后回调。每阶段先取得全局回调快照，全局执行完再读取指定回调；因此全局回调中新登记的指定回调可以参与当前阶段，登记到后续阶段的回调也可在本次构造中执行。

`event.key()` 是执行 before 前确定的规范服务身份。`event.value::<T>()` 在构造前返回 `None`，构造后按已声明的返回契约取得同一 `Arc<T>`。构造后的指定回调按服务键或 Rust 声明的返回类型匹配。Rust 无法运行时发现任意 PHP 继承或接口关系，须显式登记预期的返回契约；需要配置共享对象时使用内部可变性。动态构造参数覆盖和属性投影尚未纳入本回调接口。

共享对象先发布再执行构造后回调，回调可重入解析同一共享对象；其他线程等待回调结束，不读取配置到一半的对象。构造后回调失败或 panic 时，已发布对象仍保留缓存，命中缓存不重复执行回调，但不记录成功解析历史。瞬态对象失败时没有缓存，下次重新构造。回调错误沿现有 HTTP/Artisan 错误处理传播。没有终止条件的同服务回调递归返回循环依赖错误。执行用户回调时不持有注册表锁。

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::{Container, ServiceKey}, contracts::container::Container as _};
fn main() {
    let app = Container::default();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let before = trace.clone();
    app.beforeResolving(None, move |event, _| {
        assert!(event.value::<String>().is_none());
        before.lock().unwrap().push("before");
        Ok(())
    }).unwrap();
    app.singleton::<String, _>(|_| Ok(Arc::new("service".into())));
    let resolving = trace.clone();
    app.resolving(Some(ServiceKey::typed::<String>()), move |event, container| {
        assert_eq!(&*event.value::<String>().unwrap(), "service");
        assert!(container.resolved(event.key().clone())?);
        resolving.lock().unwrap().push("resolving");
        Ok(())
    }).unwrap();
    let after = trace.clone();
    app.afterResolving(None, move |_, _| {
        after.lock().unwrap().push("after");
        Ok(())
    }).unwrap();
    let first = app.make::<String>().unwrap();
    assert!(Arc::ptr_eq(&first, &app.make::<String>().unwrap()));
    assert_eq!(*trace.lock().unwrap(), ["before", "resolving", "after", "before"]);
}
```
