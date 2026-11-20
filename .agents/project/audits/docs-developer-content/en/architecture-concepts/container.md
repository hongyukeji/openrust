# 开发者文档边界整理：docs/en/architecture-concepts/container.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/architecture-concepts/container.md (迁移前)
+++ docs/en/architecture-concepts/container.md (开发者正文)
@@ -103,7 +103,7 @@

 `forgetScopedInstances()` clears the current lifecycle's scoped cache while preserving other active lifecycles and ordinary singletons. Outside an execution scope it clears the root scoped instances. `forgetInstance(key)` still uses the original key, and `forgetInstances()` clears current scoped instances as well as the application's ordinary instance caches. A scoped `instance` override stays local to that lifecycle. Rebinding remains an application-level registration operation.

-Container eviction does not clear a facade's separately cached root. Clear that facade cache when you need to re-resolve it within the same execution. Application-created Tokio tasks do not inherit ambient scopes; pass services explicitly or start the intended lifecycle around the task. Streaming bodies can retain already resolved services through owned `Arc` values. Streaming and termination callback wiring remain part of the HTTP delivery units.
+Container eviction does not clear a facade's separately cached root. Clear that facade cache when you need to re-resolve it within the same execution. Application-created Tokio tasks do not inherit ambient scopes; pass services explicitly or start the intended lifecycle around the task. Streaming bodies can retain already resolved services through owned `Arc` values.

 ## Contextual binding and tagged services

@@ -148,7 +148,7 @@

 ## Container resolution events

-`beforeResolving`, `resolving` and `afterResolving` accept `None` for a global callback or `Some(ServiceKey)` for a selected identity. Aliases are normalized when registering the callback. The callback receives a `ResolutionEvent` and the current container, and returns `Result<(), BindingResolutionError>`. This follows the old implementation's event and ordered-selector design, adapted to the current synchronous factories.
+`beforeResolving`, `resolving` and `afterResolving` accept `None` for a global callback or `Some(ServiceKey)` for a selected identity. Aliases are normalized when registering the callback. The callback receives a `ResolutionEvent` and the current container, and returns `Result<(), BindingResolutionError>`.

 The sequence is global before → selected before → construction → global resolving → selected resolving → global after → selected after. Before callbacks can register a missing service and execute on every lookup, including cache hits. Existing instances and shared cache hits skip post-construction callbacks. Each stage snapshots the global callbacks first, then reads selected callbacks after the globals have run: a global callback may register a selected callback for that same stage. A callback registered for a later stage can run during the current construction.

@@ -468,7 +468,7 @@

 Tagged or computed collection factories use the existing typed collection contract: `needs::<Vec<Arc<T>>>().giveTagged(tag)` or `giveTaggedAsync(tag)`. The variadic constructor can consume that collection. A direct contextual binding for `T` takes precedence over the collection adapter. Collection factories execute as their own resolution, so items do not inherit the consuming class's contextual bindings; direct `giveServices` arrays preserve that class context. Collection callbacks use the `Vec<Arc<T>>` identity, reflecting the Rust list contract.

-For primitives, bind `$name` to `Vec<Arc<T>>` or `Vec<ParameterValue>`. For example, `needsNamed::<Vec<Arc<String>>>("$names").giveConfig("app.names", None)` reads the current configuration array. The framework enables Serde's `rc` feature for these typed lists. Shared item references support values without `Clone`; heterogeneous carriers preserve null and are checked individually. These ownership representations add argument wrappers and reference counting; no zero-cost or numerical performance claim is made.
+For primitives, bind `$name` to `Vec<Arc<T>>` or `Vec<ParameterValue>`. For example, `needsNamed::<Vec<Arc<String>>>("$names").giveConfig("app.names", None)` reads the current configuration array. The framework enables Serde's `rc` feature for these typed lists. Shared item references support values without `Clone`; heterogeneous carriers preserve null and are checked individually. These ownership representations add argument wrappers and reference counting.

 Primitive contextual closures execute inline in the constructor context, as in Laravel's primitive resolver. They keep the constructor's parameter snapshot and do not create an additional service event/cache/history for `$name`. Normal dependency lookups inside the closure still follow ordinary container behavior. Async primitive closures can await initialization in that same context.

@@ -554,7 +554,7 @@

 An actual callable object or static method establishes a class context only if that class is absent from the logical build stack. `Class@method` retains the parent context, as in Laravel BoundMethod.callClass. Dependencies still enter their own normal construction contexts. Contexts are restored on every async poll and cancellation; separate applications and concurrent calls remain isolated. Application entry methods also establish the application Facade context. Running calls keep their selected override, original parameter ownership and receiver alive until completion or cancellation.

-The compiled type `C` replaces PHP class-name reflection; Rust trait aliases and concrete classes do not gain automatic method projection. Callback adapters return `Result<R, BindingResolutionError>` for injection failures; `R` may itself be a domain result. Stored callbacks and the callable dispatch future are boxed to support asynchronous borrowing on Rust 1.94. No performance benchmark or zero-allocation claim is made. Parameter attributes and SelfBuilding remain outside this accepted section.
+The compiled type `C` replaces PHP class-name reflection; Rust trait aliases and concrete classes do not gain automatic method projection. Callback adapters return `Result<R, BindingResolutionError>` for injection failures; `R` may itself be a domain result. Stored callbacks and the callable dispatch future are boxed to support asynchronous borrowing on Rust 1.94.

 ## Lazy Closures

@@ -597,7 +597,7 @@
 }
 ```

-`Callback::reusable` gives Rust 1.94 closure inference the repeatable `Fn` contract. `Callback::new` still supports `FnOnce` for ordinary calls. A wrapped callback requires `Send + Sync`; use synchronized shared state for mutation. Wrapper closures borrow their Container/Application and cannot outlive it. Each invocation returns its own Send future and may return externally borrowed data. Concrete returned closures avoid a boxed wrapper; shared callback ownership uses an Arc, and asynchronous dispatch retains the boxed-future adaptation described above. No performance measurement is claimed.
+`Callback::reusable` gives Rust 1.94 closure inference the repeatable `Fn` contract. `Callback::new` still supports `FnOnce` for ordinary calls. A wrapped callback requires `Send + Sync`; use synchronized shared state for mutation. Wrapper closures borrow their Container/Application and cannot outlive it. Each invocation returns its own Send future and may return externally borrowed data. Concrete returned closures avoid a boxed wrapper; shared callback ownership uses an Arc, and asynchronous dispatch retains the boxed-future adaptation described above.

 ## Binding Directory and Flush

@@ -643,7 +643,7 @@

 Existing values held by application code and original factory snapshots remain usable. Already admitted constructions may finish with their old values, but cannot repopulate a removed/replaced binding or its history. This base-container operation does not reboot Application providers or clear independent Facade caches. Application teardown has its own lifecycle responsibility.

-Rust uses typed definitions and owned ServiceKeys instead of a dynamically typed PHP array. Snapshot lookup is linear in the number of definitions; it is an explicit inspection operation, not part of ordinary request resolution. To support flush across concurrent requests, lifecycles register/unregister a Weak reference under a short mutex operation; flush traverses those active records. Reading a warmed shared binding retains its recipe rather than its cached object. No performance benchmark or zero-cost claim is made.
+Rust uses typed definitions and owned ServiceKeys instead of a dynamically typed PHP array. Snapshot lookup is linear in the number of definitions; it is an explicit inspection operation, not part of ordinary request resolution. To support flush across concurrent requests, lifecycles register/unregister a Weak reference under a short mutex operation; flush traverses those active records. Reading a warmed shared binding retains its recipe rather than its cached object.

 ## Environment Resolver

@@ -681,7 +681,7 @@

 Callbacks execute outside the registry lock and may reenter or replace the resolver. An active invocation retains its original callback until it returns. Retired captures are released outside the lock; errors and panics do not poison the registry. Changing or clearing the resolver does not invalidate services or emit resolution events, and `flush` preserves the resolver. Separate containers keep separate policies.

-Rust uses borrowed query enums, a typed result enum, Result and an optional Arc callback instead of PHP unions, exceptions and a nullable callable. Callbacks require Send + Sync because one container can serve concurrent requests. Environment checks clone the callback under a short read lock; no performance benchmark is claimed.
+Rust uses borrowed query enums, a typed result enum, Result and an optional Arc callback instead of PHP unions, exceptions and a nullable callable. Callbacks require Send + Sync because one container can serve concurrent requests. Environment checks clone the callback under a short read lock.

 For an application, the configuration bootstrap installs the default matcher. See [determining the current environment](../getting-started/configuration.md#determining-the-current-environment).

````
