# 服务容器

## 绑定

在 Provider 的 `register` 中通过 `app.container()` 注册服务。`bind::<T, _>` 每次解析都会调用工厂，`singleton::<T, _>` 共享首次成功结果，`instance::<T>` 提供已有的 `Arc<T>`。工厂接收同一个容器，可显式解析依赖。

Rust 没有 PHP 的构造函数反射。将接口绑定为 `dyn Trait`，由工厂返回具体实现。Rust 在编译时校验接口实现，容器失败通过 `BindingResolutionError` 返回。没有绑定或编译构造元数据的类型不会自动构造。

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
    app.singleton::<dyn Mailer, _>(|_| Ok(Arc::new(SmtpMailer)))?;
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

使用 `make::<T>()` 按 Rust 类型解析，使用 `get::<T>(key)` 按名称或显式键解析。引导时，配置仓储绑定为 `config`，`dyn openrust::contracts::config::Repository` 是其类型别名。每个应用拥有自己的容器，绑定和实例不是进程全局状态。

上下文绑定、扩展器、解析回调及重绑定见下文。延迟 Provider、动态构造覆盖和属性投影仍待完成，01-02 尚未整体关闭。

## 条件绑定与实例状态

`bindIf`、`singletonIf` 仅在类型键没有绑定、实例或别名时注册工厂，命名形式为 `bindIfNamed`、`singletonIfNamed`。存在性检查和写入是原子操作，被拒绝的工厂不执行，并在注册表解锁后释放。即使别名指向尚不存在的服务，也视为已有条目。

`isShared(key)` 检查原始键，不跟随别名。`resolved(key)` 跟随别名，在存在实例或工厂已经成功解析时返回 true。清除实例不会清除解析历史。提供实例不会执行或丢弃原工厂。

`forgetInstance(key)` 清除精确键对应的实例，不跟随别名。已有单例工厂随后可重新构造实例。`forgetInstances()` 清除全部现存实例，保留工厂、别名和解析历史。没有工厂的实例条目会变为未绑定；已经返回的 `Arc` 仍然有效。

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{Container as _, BindingResolutionError}};

fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonIfNamed::<u32, _>("counter", |_| Ok(Arc::new(7))).unwrap();
    app.bindIfNamed::<u32, _>("counter", |_| Ok(Arc::new(8))).unwrap();
    assert!(app.isShared("counter"));
    assert!(!app.resolved("counter")?);
    let original = app.get::<u32>("counter")?;
    app.instanceNamed("counter", Arc::new(99_u32)).unwrap();
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
    app.scoped::<u32, _>(|_| Ok(Arc::new(7))).unwrap();
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
use openrust::{container::Container, contracts::container::Container as _};
struct Report {
    channel: Arc<String>,
    port: Arc<u16>,
    services: Arc<Vec<Arc<String>>>,
}
fn main() {
    let container = Container::default();
    let config: Arc<dyn openrust::contracts::config::Repository> = Arc::new(openrust::config::Repository::new(Default::default()));
    container.instanceNamed("config", config.clone()).unwrap();
    config.set("reports.port", 8080.into()).unwrap();
    container.singleton::<String, _>(|_| Ok(Arc::new("default".into()))).unwrap();
    container.instanceNamed("local", Arc::new(String::from("local"))).unwrap();
    container.tag(["local"], ["reports"]);
    container.bind::<Report, _>(|c| Ok(Arc::new(Report {
        channel: c.make()?, port: c.get("$port")?, services: c.make()?,
    }))).unwrap();
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
    app.singleton::<String, _>(|_| Ok(Arc::new("service".into()))).unwrap();
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

## 扩展绑定

`extend::<T, _>(key, |service, container| ...)` 跟随别名，装饰已登记的返回契约。闭包返回配置后的 `Arc<T>`，或实现同一契约的包装对象；可通过容器参数解析其他服务。契约不匹配时，在不兼容的工厂或装饰器执行前返回错误。

尚无缓存实例时，`extend` 保存闭包供未来构造，不提前解析服务。装饰器按登记顺序在构造完成后、解析回调前执行，上下文构造同样适用。共享服务缓存装饰后的值。显式 `instance` 跳过待执行链；清掉实例后恢复原工厂及该链。

已有缓存实例时，`extend` 立即替换缓存值，这次闭包不加入未来构造链。已有调用方保留原 `Arc`，下一次查找取得新值。与 Laravel 一样，清掉缓存后不保留这种仅即时生效的装饰。`forgetExtenders(key)` 移除未来装饰器，不撤销已经装饰的缓存实例。

构造和缓存替换在注册表锁外执行用户代码；构造等待期间登记的装饰器在发布前纳入。同一缓存槽的更新串行处理，其他读取方等待更新完成，装饰器可读取自身槽的旧值。已退役槽上的更新不会覆盖新绑定。递归扩展或依赖循环返回循环依赖错误。发布前失败或 panic 的共享构造可重试；缓存更新失败保留旧指针，不回滚共享对象内部已经发生的修改。

待执行链属于应用登记。对已缓存作用域服务的即时扩展只修改当前生命周期实例，下一生命周期继续使用原工厂和待执行链。门面自己缓存的根对象不会自动替换，需要清理门面缓存或使用不缓存的门面重新解析。装饰器及移除的捕获对象在内部锁外释放，退役装饰器的捕获对象在新构造发布前释放。重绑定监听器与 `refresh` 见下文。

```rust
use std::sync::Arc;
use openrust::{container::{Container, ServiceKey}, contracts::container::Container as _};
trait Service: Send + Sync { fn name(&self) -> String; }
struct Base;
impl Service for Base { fn name(&self) -> String { "base".into() } }
struct Decorated(&'static str, Arc<dyn Service>);
impl Service for Decorated {
    fn name(&self) -> String { format!("{}:{}", self.0, self.1.name()) }
}
fn main() {
    let app = Container::default();
    let key = ServiceKey::typed::<dyn Service>();
    app.singleton::<dyn Service, _>(|_| Ok(Arc::new(Base))).unwrap();
    app.extend::<dyn Service, _>(key.clone(), |service, _| {
        Ok(Arc::new(Decorated("decorated", service)))
    }).unwrap();
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "decorated:base");
    app.extend::<dyn Service, _>(key.clone(), |service, _| {
        Ok(Arc::new(Decorated("cached", service)))
    }).unwrap();
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "cached:decorated:base");
    app.forgetInstance(key.clone());
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "decorated:base");
    app.forgetExtenders(key.clone()).unwrap();
    app.forgetInstance(key);
    assert_eq!(app.make::<dyn Service>().unwrap().name(), "base");
}
```

## 重绑定与刷新消费者

`rebinding::<T, _>(key, |container, service| ...)` 在登记监听器时跟随别名。服务已绑定时，登记会解析并返回 `Some(Arc<T>)`，但不调用该监听器；尚未绑定时返回 `None`。初始解析失败也保留已经登记的监听器。

替换已解析的绑定时触发监听器；提供实例时，只有该原始键在替换前已经绑定才通知。首次绑定、被拒绝的条件绑定以及清除实例不通知。为已解析瞬态服务或已清除缓存的共享服务追加未来装饰器时，会为监听器重新构造服务；装饰已有缓存则通知替换后的值。每次事件持有自己的已提交值和有序回调快照，并发更新不会把后一次值错交给前一次事件。派发期间新登记的监听器参与后续事件。

回调在新绑定或缓存提交后执行，错误和 panic 向调用方传播，不回滚已提交状态，失败事件中后续监听器不再执行。同服务递归通知在嵌套变更提交后返回循环错误。因此绑定登记方法返回 `Result`，应通过 `?` 传播错误；`instance` 和 `instanceNamed` 成功时返回传入的 `Arc`。

`refresh::<T, _, _>(key, target, setter)` 对应 Laravel 的目标对象/方法刷新，接收明确的 `Arc` 目标与 setter，替代 PHP 方法名字符串。登记返回当前服务，不立即调用 setter，后续重绑定才调用。回调使用显式返回契约，在注册表锁外执行，保持应用及作用域隔离。门面 swap 先更新门面缓存，再绑定容器；监听器失败时两边的新值都保留。

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::Container, contracts::container::{BindingResolutionError, Container as _}};
fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.instanceNamed("service", Arc::new(String::from("first")))?;
    let target = Arc::new(Mutex::new(String::new()));
    let current = app.refresh::<String, _, _>("service", target.clone(), |target, value| {
        *target.lock().unwrap() = (*value).clone();
        Ok(())
    })?.unwrap();
    assert_eq!(&*current, "first");
    assert!(target.lock().unwrap().is_empty());
    app.instanceNamed("service", Arc::new(String::from("second")))?;
    assert_eq!(&*target.lock().unwrap(), "second");
    Ok(())
}
```

## 异步工厂

服务构造需要 I/O 或延迟依赖时，使用异步工厂。这些接口共用现有绑定表和生命周期，不创建第二个容器；将 PHP 的闭包执行转换为 Rust 显式等待的 Future。原同步接口继续可用。

| 生命周期 | 类型工厂 | 命名工厂 |
| --- | --- | --- |
| 每次构造 | `bindAsync` | `bindAsyncNamed` |
| 单例 | `singletonAsync` | `singletonAsyncNamed` |
| 作用域 | `scopedAsync` | `scopedAsyncNamed` |

条件变体将 `If` 放在 `Async` 前，例如 `bindIfAsync`、`singletonIfAsyncNamed` 和 `scopedIfAsync`。注册及其重绑定通知都需要 await；条件不满足时不执行工厂。工厂接收当前容器，返回 `Box::pin(async move { ... })`，结果为 `Result<Arc<T>, BindingResolutionError>`；公开类型别名为 `openrust::container::FactoryFuture`。

```rust
use std::sync::Arc;
use openrust::{container::Container, contracts::container::{BindingResolutionError, Container as _}};
struct Client;
struct Worker(Arc<Client>);
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonAsync::<Client, _>(|_| Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Arc::new(Client))
    })).await?;
    app.scopedAsync::<Worker, _>(|container| Box::pin(async move {
        Ok(Arc::new(Worker(container.makeAsync::<Client>().await?)))
    })).await?;
    app.scope(async {
        let a = app.makeAsync::<Worker>().await?;
        let b = app.makeAsync::<Worker>().await?;
        assert!(Arc::ptr_eq(&a, &b));
        assert!(Arc::ptr_eq(&a.0, &app.makeAsync::<Client>().await?));
        Ok::<(), BindingResolutionError>(())
    }).await?;
    Ok(())
}
```

工厂内部的 `makeAsync`、`getAsync` 自动加载依赖的延迟 Provider 并等待就绪，无需提前预热。同步和异步绑定共用消费者上下文、别名、输出类型、装饰器与解析回调。同步解析尚未构造的异步工厂返回 `AsyncResolutionRequired`；共享缓存就绪后可以同步读取。

上下文选择需要异步解析时，使用 `giveAsync`、`giveServiceAsync`、`giveConfigAsync` 或 `giveTaggedAsync`。`tagged(...).collectAsync().await` 解析集合；`iter().nextAsync().await` 逐项解析，取消后该项仍留给下一次调用。`rebindingAsync`、`refreshAsync`、`extendAsync` 等待必要的构造与通知，但它们的回调和装饰器仍是同步函数；需要 await 的初始化放在工厂中。

```rust
use std::sync::{Arc, Mutex};
use openrust::{container::{Container, ServiceKey}, contracts::container::{BindingResolutionError, Container as _}};
struct Worker(Arc<String>);
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.bindAsyncNamed::<String, _>("message", |_| Box::pin(async {
        Ok(Arc::new(String::from("first")))
    })).await?;
    app.when::<Worker>()?.needs::<String>().giveServiceAsync("message")?;
    app.bindAsync::<Worker, _>(|container| Box::pin(async move {
        Ok(Arc::new(Worker(container.makeAsync::<String>().await?)))
    })).await?;
    app.tag([ServiceKey::typed::<Worker>()], ["workers"]);
    let workers = app.tagged::<Worker>("workers").collectAsync().await?;
    assert_eq!(&*workers[0].0, "first");
    let target = Arc::new(Mutex::new(String::new()));
    app.refreshAsync::<String, _, _>("message", target.clone(), |target, value| {
        *target.lock().unwrap() = (*value).clone();
        Ok(())
    }).await?;
    app.bindAsyncNamed::<String, _>("message", |_| Box::pin(async {
        Ok(Arc::new(String::from("second")))
    })).await?;
    assert_eq!(&*target.lock().unwrap(), "second");
    Ok(())
}
```

并发共享解析不阻塞执行器，同一次构造的等待者共享结果。构造失败或被取消时唤醒已有等待者并返回错误，后续新调用可以重试；依赖环返回错误。若解析回调在值发布后失败，已经发布的共享值仍保留缓存，沿用现有容器行为。

框架每次 poll 恢复构造上下文，挂起工厂不会把消费者泄漏给其他 Future。请求和命令结束时关闭作用域并清空缓存，即使外部还保留挂起的 Future。通过已关闭作用域继续解析返回 `ScopeClosed`；把门面 Future 移到另一应用执行中返回 `FacadeNotSet`。销毁挂起工作时恢复尚存活的原上下文；原上下文关闭后不提供环境门面。应用主动持有的 Future 和服务引用仍由应用负责，drop 时释放。这些显式异步错误和生命周期保护，是 PHP 顺序机制在 Rust 并发运行时中的适配。

## 参数覆盖

使用 `Application::makeWith::<T>(parameters).await` 提供显式构造参数。直接调用容器时，同步使用 `makeWith`，异步使用 `makeWithAsync`；命名键对应 `makeWithNamed` 和 `makeWithAsyncNamed`。应用提供 `makeWithNamed(...).await` 解析命名键。

空参数集合沿用普通解析，可以复用共享实例。非空集合选择已注册工厂，绕过显式实例和共享缓存，单独构造结果；仍应用上下文选择、装饰器和解析回调，但不替换普通共享值，也不写入普通解析历史。仅登记实例而无工厂的服务收到非空参数时返回 `NotBound`。

PHP 工厂接收参数数组；Rust 工厂保留显式容器实参，通过 `getLastParameterOverride()` 读取当前不可变快照。解析前回调通过 `ResolutionEvent::parameters()` 获取快照。普通嵌套解析使用空参数，需要转交时显式传递快照。异步解析每次 poll 恢复自己的参数帧，包括在另一带参构造中恢复普通挂起解析的情况。

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ParameterKey, ParameterOverrides, ParameterValue, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
fn arguments(number: u32) -> ParameterOverrides {
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("number"), ParameterValue::new(number));
    parameters
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.singletonAsync::<u32, _>(|container| Box::pin(async move {
        tokio::task::yield_now().await;
        let value = container.getLastParameterOverride()
            .get(&ParameterKey::name("number"))
            .map(|value| value.downcast::<u32>()).transpose()?;
        Ok(Arc::new(value.map_or(0, |value| *value)))
    })).await?;
    assert_eq!(*app.makeWithAsync::<u32>(arguments(7)).await?, 7);
    assert!(!app.resolved(ServiceKey::typed::<u32>())?);
    let normal = app.makeAsync::<u32>().await?;
    assert_eq!(*normal, 0);
    assert_eq!(*app.makeWithAsync::<u32>(arguments(9)).await?, 9);
    assert!(Arc::ptr_eq(&normal, &app.makeAsync::<u32>().await?));
    Ok(())
}
```

`ParameterOverrides` 保留插入顺序；覆盖同键保持位置并返回旧值。克隆共享不可变数据，后续修改复制集合。`ParameterKey::Name` 与 `Index` 是显式身份，数字字符串不隐式转换成整数键。`ParameterValue::null()` 区别于缺少参数。值需要 `Send + Sync + 'static`，无需实现 Clone 或序列化；`from_arc` 保留已有对象身份。保存 `Arc<dyn Trait>` 时使用 `new` 包装，并按相同 `Arc<dyn Trait>` 类型读取。

`downcast` 要求声明的 Rust 类型；类型错误或 null 返回 `ParameterTypeMismatch`，不暴露载荷内容。此载体不执行 PHP 标量隐式转换。错误、panic、取消后恢复原参数帧。挂起工厂清理可读取自己的参数；退役参数载荷在空参数帧内释放，不借用另一调用的参数。调用者明确保留的快照和服务引用仍由调用者持有。

## 自动构造器注入

Rust 没有 PHP 的运行时构造函数反射。可在 Provider 的 `register` 中通过 `app.container().registerClass(...)` 登记 `ClassDescriptor`。描述器声明具名参数类型和编译后的构造函数，替代反射检查与动态调用。登记后，普通 `make` / `makeAsync` 自动解析其依赖，无需额外绑定服务工厂。既无元数据也无绑定的类型返回 `NotBound`。

`ClassDescriptor::concrete::<T, _>` 接收同步构造函数，`concreteAsync` 接收可等待的构造函数。同步解析无法初始化异步构造器或依赖，会返回已有的异步解析错误；异步通道自动加载依赖的延迟 Provider。元数据归属当前应用且不可变：重复登记同一描述器克隆是幂等的，同类型的不同定义则拒绝。元数据本身不会让 `bound` 或 `isShared` 变为 true。普通成功解析记录历史，显式非空参数覆盖则不记录。

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::{BindingResolutionError, Container as _},
};
struct Client;
struct Service { client: Arc<Client>, name: String }
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.registerClass(ClassDescriptor::concreteAsync::<Client, _>(vec![], |_| Box::pin(async {
        tokio::task::yield_now().await;
        Ok(Arc::new(Client))
    }))?)?;
    app.registerClass(ClassDescriptor::concrete::<Service, _>(vec![
        ConstructorParam::class::<Client>("client"),
        ConstructorParam::scalar::<String>("name")
            .withDefault(|| Ok(ParameterValue::new(String::from("default")))),
    ], |args| Ok(Arc::new(Service {
        client: args.service(0)?, name: (*args.get::<String>(1)?).clone(),
    })))?)?;
    assert!(!app.bound::<Service>());
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("name"), ParameterValue::new(String::from("custom")));
    assert_eq!(app.makeWithAsync::<Service>(parameters).await?.name, "custom");
    app.singletonAsync::<Service, _>(|container| Box::pin(async move {
        container.buildAsync::<Service>().await
    })).await?;
    let service = app.makeAsync::<Service>().await?;
    assert_eq!(service.name, "default");
    let cached = app.makeAsync::<Service>().await?;
    assert!(Arc::ptr_eq(&service.client, &cached.client));
    assert!(Arc::ptr_eq(&service, &cached));
    Ok(())
}
```

`ConstructorParam::scalar::<T>("name")` 选择声明的 Rust 类型值；`class::<T>("name")` 选择 `Arc<T>`，包括 `dyn Trait` 契约。`ResolvedArgs::get` 读取原始值槽，`service` 返回原服务 Arc，`value` 暴露类型化载体。槽位按声明顺序排列；空参数名、重复名称在登记前拒绝。

选参遵循固定基线：同名显式覆盖优先。原始值随后选择 `$name` 上下文绑定、`withDefault`、启用 `nullable` 时的空值，否则失败。类默认值仅在该类没有显式绑定或直接上下文绑定时使用；否则通过容器解析类。没有默认值的可空类仍尝试解析依赖，不会把无法解析的类静默改成 null。可选默认值使用 `nullable().withDefault(|| Ok(ParameterValue::null()))`，通过 `optional` 或 `optionalService` 读取可空槽。

依赖和默认值按声明顺序求值，再对已选参数校验类型后调用构造函数。默认函数只在选中时运行，每次重新求值。参数保持严格 Rust 类型，不实现 PHP 弱标量隐式转换。普通嵌套依赖使用空覆盖；现有上下文、装饰器、解析回调及就绪保护继续有效。

`build::<T>()` / `buildAsync::<T>().await` 直接调用编译构造函数，绕过目标的绑定、缓存、装饰器、解析回调和历史；其依赖仍使用普通容器解析。可用 `singleton(|container| container.build::<T>())`、`scoped` 或相应异步接口为描述类设置共享生命周期。`build` 使用当前参数快照，带参工厂可直接转交，不把参数写入全局注册表。

方法调用、SelfBuilding、上下文/类属性和自动接口投影仍待交付，登记元数据不代表这些能力已经可用。

## 变参注入

为末尾的 `ConstructorParam` 添加 `variadic()`。变参不能有默认值，且只能有一个声明。`ResolvedArgs::values(index)` 读取有序条目，包括空列表；`services::<T>(index)` 返回服务引用。`len()` 统计声明槽位；普通 `value` / `get` / `service` 读取变参槽会返回 `InvalidArgumentShape`。

具名覆盖恰好传入一项，值本身是向量时也不会展开。整数索引覆盖不会替代同名槽。每项必须符合声明的 Rust 类型，`nullable()` 允许显式 null 条目。缺少原始值变参或无法解析的类变参都产生空列表，不注入 null。可构造的类没有上下文列表时，贡献一个普通解析实例。

```rust
use std::sync::Arc;
use openrust::{
    container::{Container, ClassDescriptor, ConstructorParam},
    contracts::container::{BindingResolutionError, Container as _},
};
trait Filter: Send + Sync { fn name(&self) -> &str; }
struct Named(&'static str);
impl Filter for Named { fn name(&self) -> &str { self.0 } }
struct Pipeline(Vec<Arc<dyn Filter>>);
fn main() -> Result<(), BindingResolutionError> {
    let app = Container::default();
    app.bindNamed::<dyn Filter, _>("first", |_| Ok(Arc::new(Named("first"))))?;
    app.bindNamed::<dyn Filter, _>("second", |_| Ok(Arc::new(Named("second"))))?;
    app.registerClass(ClassDescriptor::concrete::<Pipeline, _>(
        vec![ConstructorParam::class::<dyn Filter>("filters").variadic()],
        |args| Ok(Arc::new(Pipeline(args.services(0)?))),
    )?)?;
    app.when::<Pipeline>()?.needs::<dyn Filter>()
        .giveServices(["first", "second", "first"])?;
    let pipeline = app.make::<Pipeline>()?;
    let names: Vec<_> = pipeline.0.iter().map(|filter| filter.name()).collect();
    assert_eq!(names, ["first", "second", "first"]);
    Ok(())
}
```

Laravel 的类服务 `give(array)` 对应 `needs::<T>().giveServices(keys)`。服务键在原消费者上下文中按顺序解析，保留重复项；登记不提前构造，依赖别名会归一化，进行中的构造持有不可变键快照。异步构造通道会等待延迟或异步服务，不需要另一套列表登记接口。

标签或计算型集合工厂沿用现有类型化集合契约：`needs::<Vec<Arc<T>>>().giveTagged(tag)` 或 `giveTaggedAsync(tag)`，变参构造器可消费该集合。`T` 的直接上下文绑定优先于集合适配。集合工厂作为自己的解析执行，条目不会继承消费者类的上下文绑定；直接 `giveServices` 数组则保留该类上下文。集合回调使用 `Vec<Arc<T>>` 身份，这是 Rust 列表输出契约的适配。

原始值的 `$name` 上下文可提供 `Vec<Arc<T>>` 或 `Vec<ParameterValue>`。例如 `needsNamed::<Vec<Arc<String>>>("$names").giveConfig("app.names", None)` 读取当前配置数组；框架启用 Serde 的 `rc` 特性支持这些类型化列表。共享条目无需实现 Clone，异构载体保留 null 并逐项校验。所有权表示有参数包装与引用计数开销，未宣称零成本或数值性能收益。

原始值上下文闭包在构造上下文内直接执行，对应 Laravel 的原始值解析器；保留构造参数快照，不为 `$name` 增加服务事件、缓存或解析历史。闭包内的普通依赖仍按容器机制解析，异步闭包可在相同上下文等待初始化。

类变参遇到缺少绑定或依赖中的原始值不可解析错误时，将整个参数回退为空列表，包括已经部分解析的服务数组。业务失败、类型错误、依赖/别名环、异步解析要求、Provider 失败和生命周期关闭仍传播；原始值变参的错误也传播。取消在原上下文释放部分条目，共享服务和应用主动保留的值仍按原所有权存活。

## 方法调用

在 Container 或 Application 上使用 `call(target, parameters).await`。Rust 没有 PHP 运行时反射：`Callback` 提供闭包参数元数据，`registerMethod` 和 `registerStaticMethod` 登记编译后的方法适配器。闭包可以借用数据并返回普通 Future。登记方法的返回值要求 `Send + 'static`，不要求 `Sync`；匿名闭包可返回借用值。

```rust
use openrust::{
    container::{Callback, ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::Container as _,
};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    let signature = [ConstructorParam::scalar::<u32>("number")];
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("number"), ParameterValue::new(7u32));
    let mut result = String::new();
    let result = &mut result;
    let value = container.call(Callback::new(&signature, move |args| async move {
        result.push_str(&args.resolved.get::<u32>(0)?.to_string());
        Ok(result.as_str())
    }), parameters).await.unwrap();
    assert_eq!(value, "7");
}
```

BoundMethod 使用独立的选参规则。同名覆盖优先，其次 `ParameterKey::class::<T>()`，再解析类依赖，每个显式键仅消费一次。类未绑定时使用默认值，即使存在仅上下文绑定；原始值使用默认值，nullable 本身不补缺失值，也不读取 `$name` 上下文绑定。整数键及其他未消费输入按插入顺序保留在 `CallArguments.remaining`，不替代缺失的命名参数。末尾变参接收这些剩余值。显式数组仍是一项，计算类集合会展开，构造用的 `giveServices` 数组不适用于方法注入；缺失类依赖继续报错。

```rust
use openrust::{
    container::{ConstructorParam, Container, MethodCall, ParameterOverrides, StaticCall},
    contracts::container::Container as _,
};
use std::sync::Arc;

struct Worker;
struct Client(&'static str);

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    container.instance(Arc::new(Worker)).unwrap();
    container.instance(Arc::new(Client("ready"))).unwrap();
    container.registerMethod::<Worker, String, _>("run",
        vec![ConstructorParam::class::<Client>("client")],
        |_, args, _| Box::pin(async move {
            Ok(args.resolved.service::<Client>(0)?.0.to_owned())
        }),
    ).unwrap();
    let value: String = container.call(
        MethodCall::<Worker>::class("run"), ParameterOverrides::new(),
    ).await.unwrap();
    assert_eq!(value, "ready");
    container.bindMethod::<Worker, String, _>("run", |_, _| {
        Box::pin(async { Ok("replacement".to_owned()) })
    }).unwrap();
    assert!(container.hasMethodBinding::<Worker>("run"));
    let value: String = container.call(
        MethodCall::on(Arc::new(Worker), "run"), ParameterOverrides::new(),
    ).await.unwrap();
    assert_eq!(value, "replacement");
    container.registerStaticMethod::<Worker, u32, _>("version", vec![], |_, _| {
        Box::pin(async { Ok(1) })
    }).unwrap();
    container.bindStaticMethod::<Worker, u32, _>("version", |_, _| {
        Box::pin(async { Ok(2) })
    }).unwrap();
    assert_eq!(container.call::<u32, _>(StaticCall::<Worker>::qualified("version"),
        ParameterOverrides::new()).await.unwrap(), 1);
    assert_eq!(container.call::<u32, _>(StaticCall::<Worker>::array("version"),
        ParameterOverrides::new()).await.unwrap(), 2);
}
```

`MethodCall::on(receiver, method)` 对应对象数组调用；`class(method)` 对应 `Class@method`，先构造接收者再读取替代绑定。`defaultMethod(method)` 与 `invokable()` 对应类默认方法和已声明的 `__invoke`，仅登记替代绑定不算方法声明。空字符串和 "0" 默认名走 invokable。方法声明按 ASCII 忽略大小写查找，替代绑定按调用者原始拼写匹配。

`bindMethod` 替代原方法体，并跳过原参数注入与校验。`hasMethodBinding` 检查替代绑定，`callMethodBinding` 直接执行它，不额外建立类上下文。静态重载为 `bindStaticMethod`、`callStaticMethodBinding`。`StaticCall::qualified` 对应 `Class::method` 字符串，不查询方法绑定；`StaticCall::array` 对应类数组调用，会查询绑定。错误的接收者形式或返回类型返回类型化错误。

实际可调用的对象或静态方法，只在逻辑构造栈尚无该类时添加类上下文。`Class@method` 按 Laravel BoundMethod.callClass 保留父上下文；依赖仍建立自己的正常构造上下文。逐次异步 poll 和取消时恢复上下文，保证并发与多应用隔离。Application 入口同时建立该应用的 Facade 上下文。进行中的调用保留选定绑定、原始参数与接收者，直到完成或取消。

编译类型 `C` 替代 PHP 类名反射；Rust trait 别名与具体类之间不自动投影方法。适配器以 `Result<R, BindingResolutionError>` 传播注入错误，`R` 本身可为业务 Result。为支持 Rust 1.94 的异步借用，存储回调和调用分派 Future 使用 Box。未做性能基准，不宣称零分配。本节尚不包含参数属性或 SelfBuilding。

## 延迟闭包

`factory::<T>()` 返回每次执行 `make::<T>()` 的闭包。`factoryNamed::<T>(key)` 保存原始、拥有所有权的 ServiceKey，每次实时查找别名。创建时不检查或解析服务，后续登记、替换和实例驱逐立即可见；singleton/scoped 与普通解析的缓存行为一致。容器异步重载为 `factoryAsync`、`factoryAsyncNamed`。Application 提供异步 `factory`、`factoryNamed`，每次解析均建立该应用的 Facade 上下文。

`wrap(Callback::reusable(signature, callback), parameters)` 返回重复执行 `call` 的异步闭包。输入参数保存为写时复制快照，依赖和默认值每次重新选择。同一个回调状态被共享，不复制回调或缓存结果；失败与取消不消耗包装器。作用域与类上下文按执行时确定，不在创建包装器时冻结。

```rust
use openrust::{
    container::{Callback, ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue},
    contracts::container::Container as _,
};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    let factory = container.factory::<u32>();
    container.instance(Arc::new(3u32)).unwrap();
    assert_eq!(*factory().unwrap(), 3);
    container.instance(Arc::new(9u32)).unwrap();
    assert_eq!(*factory().unwrap(), 9);

    let signature = [ConstructorParam::class::<u32>("number"),
        ConstructorParam::scalar::<u32>("increment")];
    let mut parameters = ParameterOverrides::new();
    parameters.insert(ParameterKey::name("increment"), ParameterValue::new(2u32));
    let wrapped = container.wrap(Callback::reusable(&signature, |args| async move {
        Ok(*args.resolved.service::<u32>(0)? + *args.resolved.get::<u32>(1)?)
    }), parameters.clone());
    parameters.insert(ParameterKey::name("increment"), ParameterValue::new(7u32));
    assert_eq!(wrapped().await.unwrap(), 11);
    container.instance(Arc::new(5u32)).unwrap();
    assert_eq!(wrapped().await.unwrap(), 7);

    let async_factory = container.factoryAsyncNamed::<u32>(
        openrust::container::ServiceKey::typed::<u32>(),
    );
    assert_eq!(*async_factory().await.unwrap(), 5);
}
```

`Callback::reusable` 向 Rust 1.94 的闭包推断提供可重复执行的 `Fn` 契约，普通调用的 `Callback::new` 继续支持 `FnOnce`。包装回调需要 `Send + Sync`，可变共享状态使用同步机制。返回闭包借用 Container/Application，不能比它存活更久。每次返回独立 Send Future，可返回外部借用值。具体闭包类型避免包装器本身的 Box，共享回调使用 Arc，异步分派保留前述 Box Future 适配；不宣称实测性能收益。

## 绑定目录与清理

`getBindings()` 返回拥有所有权、保留插入顺序的 `Bindings` 快照。`get(&ServiceKey)` 精确查键，`iter`、`keys` 和所有权迭代保留登记顺序；替换绑定不移动位置。每个 `BindingDefinition` 暴露原始 `concrete` 工厂及声明的 `shared`，显式实例覆盖不改变该标志。别名单独键、仅实例键、上下文绑定和自动类元数据不在其中。读取或修改快照不会解析服务或修改容器。

保留的 `FactoryHandle` 对应直接执行 Laravel 绑定数组中的 concrete 闭包。`call::<T>(&container, parameters)` 与 `callAsync::<T>(...)` 使用传入容器及显式参数帧，不添加目标类上下文。它们绕过目标缓存、装饰器、事件和解析历史；工厂内部依赖仍正常解析，使用新参数帧。因此共享原工厂每次也产生新结果。输出类型错误在执行用户代码前拒绝；同步调用异步工厂会失败，即使普通解析已缓存结果。`sameFactory` 比较保留配方的身份。

```rust
use openrust::{
    container::{Container, ParameterOverrides, ServiceKey},
    contracts::container::Container as _,
};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    container.singleton::<u32, _>(|_| Ok(Arc::new(1))).unwrap();
    container.extend::<u32, _>(ServiceKey::typed::<u32>(), |value, _| {
        Ok(Arc::new(*value + 10))
    }).unwrap();
    let definitions = container.getBindings();
    let definition = definitions.get(&ServiceKey::typed::<u32>()).unwrap();
    assert!(definition.shared);
    assert_eq!(*definition.concrete.callAsync::<u32>(
        &container, ParameterOverrides::new(),
    ).await.unwrap(), 1);
    assert!(!container.resolved(ServiceKey::typed::<u32>()).unwrap());
    assert_eq!(*container.make::<u32>().unwrap(), 11);

    container.flush();
    assert!(container.getBindings().is_empty());
    assert!(!container.bound::<u32>());
    assert_eq!(*definition.concrete.call::<u32>(
        &container, ParameterOverrides::new(),
    ).unwrap(), 1);
    container.bind::<u32, _>(|_| Ok(Arc::new(2))).unwrap();
    assert_eq!(*container.make::<u32>().unwrap(), 12);
}
```

`flush()` 清空绑定、别名及反向别名、显式/共享/scoped 实例、scoped 登记和解析历史。按 Laravel 基础 Container 保留上下文规则、标签、装饰器、解析/重绑定回调、方法声明/绑定及编译类元数据。同时解除本容器所有活动生命周期的缓存，不关闭这些生命周期。用户值在状态提交后、注册表与缓存锁外释放，允许析构重入。flush 和正常生命周期结束均在中立的 Facade、参数及类上下文中释放 scoped 缓存值。析构 panic 不回滚清理，也不污染注册表锁。

应用自行持有的对象及原工厂快照仍有效，已准入构造可返回旧值，但不能回填已移除或替换的绑定及其历史。此基础容器操作不重新启动 Application Provider，也不清除独立 Facade 缓存；应用终止由生命周期职责负责。

Rust 使用类型化定义和拥有所有权的 ServiceKey 替代动态 PHP 数组。快照查询按定义数量线性查找，属于显式检查，不加入普通请求解析路径。为支持并发请求的全局 flush，每个生命周期在登记/注销时以短 Mutex 操作维护 Weak 引用，flush 遍历活动记录。读取已预热共享绑定仅保留配方，不保留缓存对象。未做性能基准，不宣称零成本。

## 环境解析回调

`resolveEnvironmentUsing(Some(Arc::new(callback)))` 安装基础容器环境解析回调，`None` 清除。无回调时，`currentEnvironmentIs(query)` 返回 `Ok(EnvironmentResult::Boolean(false))`。基础容器不解析环境文件、不匹配通配符，也不重新解释空参数，而是原样转交 `EnvironmentQuery::Name` 或 `Names`，保留数组顺序、重复项与空字符串。同步回调借用查询字符串，不复制参数。

回调可返回 `EnvironmentResult::Boolean` 或 `String`，保留 PHP 支持的两种结果形状；`Result` 将回调异常作为容器错误传播。`bool::from(result)` 或 `bool::from(&result)` 按 PHP 真值转换，false、空字符串和字符串 "0" 为假。示例刻意采用显式自定义策略：空数组返回名称，并不代表 Laravel Application 默认环境匹配规则。

```rust
use openrust::{
    container::{Container, EnvironmentQuery, EnvironmentResult},
    contracts::container::Container as _,
};
use std::sync::Arc;

fn main() {
    let container = Container::default();
    assert_eq!(container.currentEnvironmentIs("local").unwrap(), false.into());
    container.resolveEnvironmentUsing(Some(Arc::new(|query| {
        Ok(match query {
            EnvironmentQuery::Name(name) => (name == "testing").into(),
            EnvironmentQuery::Names([]) => "testing".into(),
            EnvironmentQuery::Names(names) => names.contains(&"testing").into(),
        })
    })));
    assert!(bool::from(container.currentEnvironmentIs("testing").unwrap()));
    assert!(bool::from(container.currentEnvironmentIs(&["local", "testing"]).unwrap()));
    assert_eq!(container.currentEnvironmentIs(&[]).unwrap(),
        EnvironmentResult::from("testing"));
    container.flush();
    assert!(bool::from(container.currentEnvironmentIs("testing").unwrap()));
    container.resolveEnvironmentUsing(None);
    assert_eq!(container.currentEnvironmentIs("testing").unwrap(), false.into());
}
```

回调在注册表锁外执行，可以重入或替换解析器；进行中的调用保留原回调直到返回。旧捕获在锁外释放，错误与 panic 不污染注册表锁。更换或清除解析器不驱逐服务缓存、不触发解析事件，`flush` 保留解析器，各容器策略独立。

Rust 用借用查询枚举、结果枚举、Result 与可选 Arc 回调对应 PHP 联合类型、异常和可空 callable。回调要求 Send + Sync，以支持同容器并发请求。环境检查仅在短读锁内克隆回调，未做性能基准。

应用在加载配置时安装默认匹配器。参见[确定当前环境](../getting-started/configuration.md#determining-the-current-environment)。

## 当前容器和键访问

在服务提供者启动、HTTP 处理、Artisan 执行及应用作用域内解析服务时，`Container::getInstance()` 返回当前应用的容器。在应用作用域之外，首次调用会创建进程默认容器。`Container::setInstance(Some(container))` 替换默认容器并返回传入的句柄，`Container::setInstance(None)` 清除默认容器。运行中的应用各自使用自己的容器。

`offsetExists` 检查键是否已绑定，`offsetGet::<T>` 将服务解析为 `Arc<T>`。`offsetSet` 绑定已有值，`offsetSetFactory` 注册每次解析都执行的工厂。异步接口为 `offsetGetAsync` 和 `offsetSetFactoryAsync`。

```rust
use openrust::container::Container;
use std::sync::Arc;

fn main() {
    let container = Container::getInstance();
    let greeting: Arc<str> = Arc::from("hello");
    container.offsetSet("greeting", greeting).unwrap();
    assert!(container.offsetExists("greeting"));
    assert_eq!(&*container.offsetGet::<str>("greeting").unwrap(), "hello");

    container.offsetSetFactory("answer", |_| Ok(Arc::new(42_u32))).unwrap();
    assert_eq!(*container.offsetGet::<u32>("answer").unwrap(), 42);
    container.offsetUnset("answer");
    assert!(!container.offsetExists("answer"));
}
```

`offsetUnset` 按原始键移除绑定、共享实例和解析记录，同时清理活动作用域中的缓存值。别名保留，读取目标已删除的别名会返回解析错误，直至重新绑定目标。Rust 使用具名方法表达可失败的类型化读取和修改。删除前已在执行的解析仍可向发起者返回结果，后续解析使用当前注册表。

## 自构造类

类型需要静态构造方法时，实现 `SelfBuilding::newInstance` 并注册 `ClassDescriptor::selfBuilding`。描述符分别声明普通构造函数和 `newInstance` 的参数。容器按静态方法的参数声明注入依赖并调用 `newInstance`。

```rust
use openrust::{
    container::{ClassDescriptor, ConstructorParam, Container, FactoryFuture, ResolvedArgs},
    contracts::container::{Container as _, SelfBuilding},
};
use std::sync::Arc;

struct Report(u32);

impl SelfBuilding for Report {
    #[allow(non_snake_case)]
    fn newInstance(args: ResolvedArgs, container: &Container) -> FactoryFuture<'_, Self> {
        Box::pin(async move {
            let base = container.build::<Self>()?;
            Ok(Arc::new(Self(base.0 + *args.service::<u32>(0)?)))
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let container = Container::default();
    container.instance(Arc::new(7_u32)).unwrap();
    container
        .registerClass(
            ClassDescriptor::selfBuilding::<Report>(
                vec![],
                |_| Ok(Arc::new(Report(1))),
                vec![ConstructorParam::class::<u32>("increment")],
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(container.makeAsync::<Report>().await.unwrap().0, 8);
}
```

使用 `makeAsync` 或 `buildAsync` 解析自构造类。`makeWithAsync` 的构造参数覆盖不会传给静态方法。若 `newInstance` 在构造期间再次解析同一类型，内层构建走普通构造函数。普通构造中的循环依赖仍会返回解析错误。可用 `bindStaticMethod` 绑定 `newInstance`，替换容器构建时调用的静态方法。

## 上下文属性

使用 `ConstructorParam::withAttribute` 为构造函数、方法或 `newInstance` 的参数声明上下文属性。实现 `ContextualAttribute` 定义如何解析值。可通过 `whenHasAttribute` 为指定容器覆盖解析器。

```rust
use openrust::{
    container::{
        AttributeParameter, AttributeValue, ClassAttribute, ClassDescriptor,
        ConstructorParam, Container, ParameterValue,
    },
    contracts::container::{BindingResolutionError, Container as _, ContextualAttribute},
};
use std::sync::Arc;

struct Port;
impl ContextualAttribute for Port {
    fn resolve(
        &self,
        _: &Container,
        _: AttributeParameter,
    ) -> Result<AttributeValue, BindingResolutionError> {
        Ok(ParameterValue::new(8080_u16).into())
    }
}

struct Server(u16);
fn main() {
    let container = Container::default();
    let port = ConstructorParam::scalar::<u16>("port")
        .withAttribute(ClassAttribute::contextual(|| Ok(Port)));
    let descriptor = ClassDescriptor::concrete(vec![port], |args| {
        Ok(Arc::new(Server(*args.get::<u16>(0)?)))
    })
    .unwrap();

    container.registerClass(descriptor).unwrap();
    assert_eq!(container.make::<Server>().unwrap().0, 8080);
}
```

构造函数先查显式命名参数，再使用第一个上下文属性。属性返回空值时回退到普通构造参数解析。方法调用依次检查命名参数、属性、类名参数或普通解析。方法的可空参数会保留属性返回的空值。

使用 `ClassAttribute::marker` 声明标记，使用 `afterResolvingAttribute` 按属性类型或声明的服务类型观察解析结果。参数回调在构造或调用前执行，显式构造参数会跳过其回调。类属性回调在构造完成后、扩展器和服务解析回调之前执行。属性回调使用同步 Rust 函数，也会在异步解析中执行。

## 条件绑定属性

通过 `registerBindingAttributes` 登记按顺序排列的绑定元数据。`Bind` 匹配应用环境，`BindWhen` 使用当前容器执行同步条件。通过 Rust 类型转换声明具体类实现的服务接口。

```rust
use openrust::{
    container::{Bind, BindWhen, ClassDescriptor, Container},
    contracts::container::Container as _,
};
use std::sync::Arc;

trait Reporter: Send + Sync {
    fn label(&self) -> &'static str;
}

struct LocalReporter;
impl Reporter for LocalReporter {
    fn label(&self) -> &'static str {
        "local"
    }
}

fn main() {
    let container = Container::default();
    container
        .registerClass(
            ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(LocalReporter))).unwrap(),
        )
        .unwrap();
    container
        .registerBindingAttributes::<dyn Reporter>(vec![
            BindWhen::new(
                Bind::new::<dyn Reporter, LocalReporter>(|reporter| reporter),
                |_| Ok(true),
            )
            .into(),
        ])
        .unwrap();
    assert_eq!(container.make::<dyn Reporter>().unwrap().label(), "local");
}
```

容器使用第一个命中的声明。通配 `Bind` 在检查完条件和具体环境后作为回退。环境声明依赖应用启动时安装的环境解析器。已有的显式绑定优先。选择成功后登记并复用绑定，直至该绑定被移除。`BindWhen` 未命中时，下次解析会重新检查。
