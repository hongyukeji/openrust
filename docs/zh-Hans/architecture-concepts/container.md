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

本节完成工厂参数覆盖。自动构造描述、原始值/默认值/变参注入以及容器方法调用仍待后续单元，不能据此认为反射式构造已经完成。
