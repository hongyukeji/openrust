# 开发者文档边界整理：docs/zh_CN/architecture-concepts/container.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_CN/architecture-concepts/container.md (迁移前)
+++ docs/zh_CN/architecture-concepts/container.md (开发者正文)
@@ -103,7 +103,7 @@

 `forgetScopedInstances()` 清除当前生命周期的作用域缓存，保留其他活动生命周期及普通单例。在执行作用域之外调用，则清除根作用域实例。`forgetInstance(key)` 仍按原始键操作；`forgetInstances()` 同时清除当前作用域实例及应用的普通实例缓存。作用域服务的 `instance` 覆盖仅在当前生命周期生效，重新绑定仍是应用级注册操作。

-容器清理不会清除门面独立缓存的根对象；需要在同一次执行中重新解析时，同时清除门面缓存。应用自行创建的 Tokio 任务不继承环境作用域，应显式传入服务，或为任务设置所需生命周期。流式正文可以通过持有已解析服务的 `Arc` 延续其使用。流式及终止回调接线仍属于 HTTP 交付单元。
+容器清理不会清除门面独立缓存的根对象；需要在同一次执行中重新解析时，同时清除门面缓存。应用自行创建的 Tokio 任务不继承环境作用域，应显式传入服务，或为任务设置所需生命周期。流式正文可以通过持有已解析服务的 `Arc` 延续其使用。

 ## 上下文绑定与标签服务

@@ -148,7 +148,7 @@

 ## 容器解析事件

-`beforeResolving`、`resolving` 和 `afterResolving` 用 `None` 登记全局回调，用 `Some(ServiceKey)` 选择服务身份。登记时规范化别名。回调接收 `ResolutionEvent` 与当前容器，返回 `Result<(), BindingResolutionError>`。实现提取旧版的事件视图与有序选择器设计，适配当前同步工厂。
+`beforeResolving`、`resolving` 和 `afterResolving` 用 `None` 登记全局回调，用 `Some(ServiceKey)` 选择服务身份。登记时规范化别名。回调接收 `ResolutionEvent` 与当前容器，返回 `Result<(), BindingResolutionError>`。

 执行顺序为全局 before → 指定 before → 构造 → 全局 resolving → 指定 resolving → 全局 after → 指定 after。before 回调可以登记尚未绑定的服务，每次查找都执行，缓存命中也执行。显式实例及共享缓存命中跳过构造后回调。每阶段先取得全局回调快照，全局执行完再读取指定回调；因此全局回调中新登记的指定回调可以参与当前阶段，登记到后续阶段的回调也可在本次构造中执行。

@@ -468,7 +468,7 @@

 标签或计算型集合工厂沿用现有类型化集合契约：`needs::<Vec<Arc<T>>>().giveTagged(tag)` 或 `giveTaggedAsync(tag)`，变参构造器可消费该集合。`T` 的直接上下文绑定优先于集合适配。集合工厂作为自己的解析执行，条目不会继承消费者类的上下文绑定；直接 `giveServices` 数组则保留该类上下文。集合回调使用 `Vec<Arc<T>>` 身份，这是 Rust 列表输出契约的适配。

-原始值的 `$name` 上下文可提供 `Vec<Arc<T>>` 或 `Vec<ParameterValue>`。例如 `needsNamed::<Vec<Arc<String>>>("$names").giveConfig("app.names", None)` 读取当前配置数组；框架启用 Serde 的 `rc` 特性支持这些类型化列表。共享条目无需实现 Clone，异构载体保留 null 并逐项校验。所有权表示有参数包装与引用计数开销，未宣称零成本或数值性能收益。
+原始值的 `$name` 上下文可提供 `Vec<Arc<T>>` 或 `Vec<ParameterValue>`。例如 `needsNamed::<Vec<Arc<String>>>("$names").giveConfig("app.names", None)` 读取当前配置数组；框架启用 Serde 的 `rc` 特性支持这些类型化列表。共享条目无需实现 Clone，异构载体保留 null 并逐项校验。所有权表示有参数包装与引用计数开销。

 原始值上下文闭包在构造上下文内直接执行，对应 Laravel 的原始值解析器；保留构造参数快照，不为 `$name` 增加服务事件、缓存或解析历史。闭包内的普通依赖仍按容器机制解析，异步闭包可在相同上下文等待初始化。

@@ -554,7 +554,7 @@

 实际可调用的对象或静态方法，只在逻辑构造栈尚无该类时添加类上下文。`Class@method` 按 Laravel BoundMethod.callClass 保留父上下文；依赖仍建立自己的正常构造上下文。逐次异步 poll 和取消时恢复上下文，保证并发与多应用隔离。Application 入口同时建立该应用的 Facade 上下文。进行中的调用保留选定绑定、原始参数与接收者，直到完成或取消。

-编译类型 `C` 替代 PHP 类名反射；Rust trait 别名与具体类之间不自动投影方法。适配器以 `Result<R, BindingResolutionError>` 传播注入错误，`R` 本身可为业务 Result。为支持 Rust 1.94 的异步借用，存储回调和调用分派 Future 使用 Box。未做性能基准，不宣称零分配。本节尚不包含参数属性或 SelfBuilding。
+编译类型 `C` 替代 PHP 类名反射；Rust trait 别名与具体类之间不自动投影方法。适配器以 `Result<R, BindingResolutionError>` 传播注入错误，`R` 本身可为业务 Result。为支持 Rust 1.94 的异步借用，存储回调和调用分派 Future 使用 Box。

 ## 延迟闭包

@@ -597,7 +597,7 @@
 }
 ```

-`Callback::reusable` 向 Rust 1.94 的闭包推断提供可重复执行的 `Fn` 契约，普通调用的 `Callback::new` 继续支持 `FnOnce`。包装回调需要 `Send + Sync`，可变共享状态使用同步机制。返回闭包借用 Container/Application，不能比它存活更久。每次返回独立 Send Future，可返回外部借用值。具体闭包类型避免包装器本身的 Box，共享回调使用 Arc，异步分派保留前述 Box Future 适配；不宣称实测性能收益。
+`Callback::reusable` 向 Rust 1.94 的闭包推断提供可重复执行的 `Fn` 契约，普通调用的 `Callback::new` 继续支持 `FnOnce`。包装回调需要 `Send + Sync`，可变共享状态使用同步机制。返回闭包借用 Container/Application，不能比它存活更久。每次返回独立 Send Future，可返回外部借用值。具体闭包类型避免包装器本身的 Box，共享回调使用 Arc，异步分派保留前述 Box Future 适配。

 ## 绑定目录与清理

@@ -643,7 +643,7 @@

 应用自行持有的对象及原工厂快照仍有效，已准入构造可返回旧值，但不能回填已移除或替换的绑定及其历史。此基础容器操作不重新启动 Application Provider，也不清除独立 Facade 缓存；应用终止由生命周期职责负责。

-Rust 使用类型化定义和拥有所有权的 ServiceKey 替代动态 PHP 数组。快照查询按定义数量线性查找，属于显式检查，不加入普通请求解析路径。为支持并发请求的全局 flush，每个生命周期在登记/注销时以短 Mutex 操作维护 Weak 引用，flush 遍历活动记录。读取已预热共享绑定仅保留配方，不保留缓存对象。未做性能基准，不宣称零成本。
+Rust 使用类型化定义和拥有所有权的 ServiceKey 替代动态 PHP 数组。快照查询按定义数量线性查找，属于显式检查，不加入普通请求解析路径。为支持并发请求的全局 flush，每个生命周期在登记/注销时以短 Mutex 操作维护 Weak 引用，flush 遍历活动记录。读取已预热共享绑定仅保留配方，不保留缓存对象。

 ## 环境解析回调

@@ -681,7 +681,7 @@

 回调在注册表锁外执行，可以重入或替换解析器；进行中的调用保留原回调直到返回。旧捕获在锁外释放，错误与 panic 不污染注册表锁。更换或清除解析器不驱逐服务缓存、不触发解析事件，`flush` 保留解析器，各容器策略独立。

-Rust 用借用查询枚举、结果枚举、Result 与可选 Arc 回调对应 PHP 联合类型、异常和可空 callable。回调要求 Send + Sync，以支持同容器并发请求。环境检查仅在短读锁内克隆回调，未做性能基准。
+Rust 用借用查询枚举、结果枚举、Result 与可选 Arc 回调对应 PHP 联合类型、异常和可空 callable。回调要求 Send + Sync，以支持同容器并发请求。环境检查仅在短读锁内克隆回调。

 应用在加载配置时安装默认匹配器。参见[确定当前环境](../getting-started/configuration.md#determining-the-current-environment)。

````
