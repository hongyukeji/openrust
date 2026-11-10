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

作用域绑定、延迟服务、上下文绑定、扩展器和解析回调属于下一容器单元。上述绑定与解析能力不代表这些功能已经实现。

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

这些操作不会清除门面自己的根对象缓存，需要时另行调用门面缓存清除接口。作用域服务清理由容器单元中的作用域生命周期部分交付。
