# R2 ServiceProvider、Container 与配置合同

> 受众：下一批实现与独立审查 Agent
> 状态：Spec 修订；§12容器基元已由不同模型审查，其他Provider/配置/资源部分仍须随子簇收敛；不作为 R2 整体完成证据
> 更新：2026-10-28
> 授权：Laravel Rust 实现版本，全面实现 Laravel；本文件固定 R2 内核实施合同，不降低为 inspired-by 框架。

## 1. 范围与事实入口

R1-S 已以 34dfb5e 完成限定提交；现在领取 R2 基础，先收敛合同，再实现可独立验证的容器基元。已检查 plans/、相关 ADR 和 framework-design，没有同范围的可直接编码合同；总目标与矩阵继续以 [ADR-070](../decisions/adr-070-laravel-first-rebuild.md)、[总计划](laravel-rebuild.md)、[活跃待办](../todo.md) 为准。本合同不是第二份活跃队列；模块目标位置与资源边界须符合 [ADR-074](../decisions/adr-074-laravel-modules-directory.md)，实际开工前由主控收敛并独立审查。

### Spec

1. 统一为一个类型化 Container 与 ServiceProvider 执行引擎，先注册全部绑定，再启动全部 provider。
2. register 只声明绑定和配置默认值/schema，不解析服务、不装路由、不访问外部服务。
3. register 收集核心配置 defaults/schema，boot 按 provider 顺序执行模块配置 overlay；区分 Laravel 核心默认合并与 nWidart 模块覆盖，保留可观察时点。
4. bind、singleton、scoped、instance 分别表达新建、应用共享、生命周期共享和实例替换。
5. 显式异步 factory 代替 PHP 反射构造；缺失、循环、初始化失败均为 Result，绝不 panic。
6. root/boot 有启动 lifecycle scope，HTTP、job attempt、console 各有独立 scope；forgetScopedInstances 清除当前生命周期实例，复用 ExecutionContext。
7. 模块通过 provider 清单及公开路由/资源贡献接入；删掉应用清单中的模块后不影响框架基础服务。
8. 现有 Plugin 按职责拆分迁移，最终删除旧执行语义；不长期并存 Plugin/Provider 两套容器。
9. 重用 typed section、具名数据库/缓存与 driver，不改变 Redis 默认或静默降级规则。
10. 按可编译子簇推进；文档任务对照、真实服务及公开消费者验收决定完成，计划不代替实现。

### 当前可复用资产与具体缺口

| 当前事实 | 证据与 R2 处理 |
| --- | --- |
| Registry 以 TypeId 保存 Arc 服务；insert 后写替换，try_insert 拒绝冲突 | [app.rs](../../../src/core/app.rs)；保留类型键/共享句柄与替换能力，补 factory 和 scope，不造字符串 any 容器 |
| AppBuilder/App 均有 section<T>；来源为同一个 Figment | app.rs 的 extract_section、section、figment；复用提取/来源，不新增平行 Config 全局对象 |
| Plugin::build 可绑定服务/路由/命令；全部 build 后 freeze，再逐个异步 finish | [plugin.rs](../../../src/core/plugin.rs)、[lifecycle.rs](../../../src/core/lifecycle.rs) 的 finalize；有两批调用不等于 Laravel register/boot，build 的混合副作用必须拆开 |
| Application::Config 和 FrameworkConfig 当前在 Application::build 前读取 | lifecycle.rs 的 bootstrap_with；这阻止模块先声明 defaults/schema，须重排而非在旧 build 后追加默认 |
| 具名 DB instances 已实现，每个实例完整配置，不继承默认连接 | [database.rs](../../../src/config/database.rs)、DatabasePlugin；不是 R2 新功能 |
| 具名 cache stores、开放 driver options、公开 CacheDrivers 已实现 | [cache.rs](../../../src/config/cache.rs)、[缓存实现](../../../src/cache.rs)；保留动态 driver 字段边界，不机械全局 deny |
| ExecutionContext、Cancellation 与受管后台任务已有 | [execution.rs](../../../src/core/execution.rs)、lifecycle.rs；复用预算/取消/metadata，新增的是容器 scope 缓存与所有权 |
| 旧 core 事实曾写“不另增 register 生命周期” | [core/core.md](../core/core.md) 的旧实现说明不是本轮约束；实施时随 ADR-070 更新，不能阻止真实迁移 |

## 2. 上游行为基线

沿用 [R0 固定来源](laravel-rebuild-baseline.md)，不另滚动扩大主基线。文档 commit 为 `960eedf0911155ad16326ade49c0ea5d18eda0b8`，Laravel framework v13.39.0 commit 为 `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`；2026-10-28 重新核对下列官方入口与固定源码。

- [Providers](https://laravel.com/framework/docs/13.x/providers)：register 限容器绑定；所有 provider 注册后 boot，boot 可使用其他已注册服务。
- [Container](https://laravel.com/framework/docs/13.x/container)：绑定、共享实例、scoped 生命周期和接口解析；Rust factory 显式表达其构造依赖。
- [Lifecycle](https://laravel.com/framework/docs/13.x/lifecycle)：provider 清单完成 register/boot 后才进入请求分发。
- [Packages](https://laravel.com/framework/docs/13.x/packages)：provider 是服务、配置及模块资源接入点；本合同不把 Composer/autoload 当作 Cargo 功能。
- 固定 [Application.php](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Foundation/Application.php) 的 register/boot/bootProvider 和 [Container.php](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Container/Container.php) 的 bind/singleton/scoped/instance 为实现对照；不复制 PHP 全局实例方式。

以下 API 是选定的待实现 Rust 合同，不是当前可用接口。必要差异逐项验收，不以实现难度删去 Laravel 的后继行为。

## 3. 唯一启动状态机

`CollectProviders → RegisterAll → ValidateBaseConfig → SealBindings → BootAll → FinalizeRuntimeConfig → FinalizeContributions → Ready → Shutdown`。

1. 环境与来源准备沿用当前 bootstrap：确定 namespace/environment、加载 .env（已有进程变量不被覆盖）、收集文件/环境来源。此时不提取完整 FrameworkConfig 或 Application::Config，也不连接后端。
2. 收集固定 provider 清单：框架默认清单在前、应用清单在后。Rust 通过静态模块声明和 provider 值构造清单；编译期 package 声明不是运行时模块发现。
3. 按清单依次 register，收集工厂、实例、配置 defaults/schema。RegisterContext 没有 resolve/routes/command/extension_mut；工厂闭包可以捕获启动常量，不能提前构造连接/任务。
4. 全体 register 完成后，将全部默认值放在来源链最低层，完成配置合并、schema、类型与领域校验，再提供基础配置视图。模块 overlay 在各自 boot 中应用，并立即校验受影响 section；后续 boot/resolve 读取更新视图，早先已取得的 owned 配置不回溯改写。最终 FrameworkConfig、Application::Config 在 boot 结束后提取供运行入口使用。记录来源不记录秘密值；基础配置失败时不运行任何 boot。
5. 封住绑定变更；测试 overrides 是 register 后、seal 前的显式阶段。此后 resolve 只读定义，异步创建缓存由容器管理。
6. 按同一清单顺序 boot。boot 可异步 resolve 服务、贡献路由/命令/模块资源和注册关闭回调；不能增加 provider 或重新绑定；仅允许通过受控模块配置 overlay 接口更新配置。每次更新校验后发布新版本，失败沿启动回收路径退出。
7. 所有 boot 成功后冻结贡献、检查路由/命令/资源冲突并验证 require 根服务，才创建可服务 App、打开业务 listener/worker。Ready 前不可接受业务流量。
8. 任意阶段失败即停止后续阶段，逆序回收已登记资源；应用永不以半启动状态返回。正常停机使用同一关闭路径。

### Provider 身份、顺序与阶段约束

- 身份取 provider 具体 Rust TypeId，名称用 type_name 诊断；trait继承 Any，通过具体类型的object-safe id默认方法返回TypeId，不能对Box自身取TypeId；同类型重复登记默认幂等，保留第一项，匹配 Laravel 常规 register 的去重心智。需要不同实例身份时定义不同 provider 类型，不用字符串名称绕过身份检查。
- register 不要求服务依赖的 provider 在前：依赖在工厂实际解析时读取全量绑定。需要 boot 先后关系时声明 dependencies，要求依赖已在显式清单前方；缺依赖、循环或顺序错误在 register 前报错，不偷偷重排清单。
- 未声明依赖的 provider 保持清单顺序。异步 boot 首批串行，保证配置/贡献/失败清理确定性；不默认并发所有 provider。
- 禁止 Ready 后动态补 provider。动态加载/卸载、deferred provider 和完整模块管理另有后继子簇，不能把删清单重启等同于运行时热卸载。

## 4. Provider API 与模块示例

签名中的 ProviderId、RegisterContext、BootContext、Resolver、ContainerBuilder 均是待实现公开类型；BoxFuture 复用现有别名，错误继续使用 openrust::Result。

```rust,ignore
pub trait ServiceProvider: Send + Sync + 'static {
    fn dependencies(&self) -> Vec<ProviderId> { Vec::new() }
    fn register(&self, app: &mut RegisterContext) -> Result<()> { Ok(()) }
    fn boot<'a>(&'a self, app: &'a mut BootContext) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
```

Application::build 不再混合初始化。新入口为 `Application::providers() -> Vec<Box<dyn ServiceProvider>>`；框架默认清单仍由唯一 bootstrap 添加，应用只提交自己的 provider 值。保留 Application::Config/config_namespace 的意义，但把最终提取移到 BootAll 后的 FinalizeRuntimeConfig。测试可使用 AppBuilder.provider 收集相同清单，并通过同一 finalize；不能有测试专用第二启动器。

模块 ExampleServiceProvider 的操作示例：

```rust,ignore
impl ServiceProvider for ExampleServiceProvider {
    fn register(&self, app: &mut RegisterContext) -> Result<()> {
        app.config_section::<ExampleConfig>(
            "example", ExampleConfig::default(), example_schema(), ExampleConfig::validate,
        )?;
        app.singleton::<dyn Greeting, _>(|r| Box::pin(async move {
            let config = r.section::<ExampleConfig>("example")?;
            Ok(Arc::new(GreetingService::new(config)) as Arc<dyn Greeting>)
        }))?;
        Ok(())
    }

    fn boot<'a>(&'a self, app: &'a mut BootContext) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let _service = app.resolve::<dyn Greeting>().await?;
            app.routes(crate::modules::example::routes::register);
            Ok(())
        })
    }
}
```

业务类型、schema 和路由回调由模块自己定义，示例只说明已选接口间如何组合，不是已存在的 ExampleServiceProvider。`app.routes` 沿用公开路由贡献方法名，HTTP 扩展 trait 为 BootContext 实现；回调签名按当前 Routes contract，不在 RegisterContext 暴露。

### 模块资源与退出

- 模块配置/迁移/模板/协议/测试均留 modules/<模块>；根 app/mod.rs 或应用 provider 清单只声明与注册，不改 src 核心，不加样例路径/名字特判。
- BootContext 复用当前 typed extension 贡献机制，提供受阶段约束的 `extension_mut<T>()`，供各能力的公开扩展 trait 收集路由、事件、命令和资源描述；不能借它写容器实例或修改 ConfigRepository。
- 模块资源描述带 provider/module 身份及资源局部名称。重复完整资源键报来源冲突，顺序为 provider 清单+模块内登记顺序；不同 namespace 可并存，不以后写覆盖其他模块资源。
- Routes/命令适配与冲突验证在 R2 完成；视图/语言/迁移的领域扩展沿用其公开服务，先交两模块独立 typed 资源贡献的公开 consumer。完整命名空间视图/迁移合并能力有缺口时留对应领域/R2 资源适配待办，不虚构“放进 registry 就已生效”。
- 从应用清单移除模块后重启，不加载其默认值/资源/路由，不创建其连接和后台任务；框架 health 等通用服务保留。不自动回滚模块数据库或删除资源数据。

## 5. 类型化 Container 合同

### 公开操作与类型表示

`bind` 等价 transient，每次 resolve 创建新值；不另造同义 transient 注册方法。singleton 在一个 App 内共享，scoped 在一个显式生命周期内共享，instance 注册调用者提供的 Arc。四者使用同一 TypeId 键和 factory/cache 引擎。

```rust,ignore
// 以下为 RegisterContext/ContainerBuilder 共有的签名；T 可为 dyn Trait。
fn bind<T, F>(&mut self, factory: F) -> Result<()>
where T: ?Sized + Send + Sync + 'static,
      F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static;
fn singleton<T, F>(&mut self, factory: F) -> Result<()>
where T: ?Sized + Send + Sync + 'static,
      F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static;
fn scoped<T, F>(&mut self, factory: F) -> Result<()>
where T: ?Sized + Send + Sync + 'static,
      F: Fn(Resolver) -> BoxFuture<'static, Result<Arc<T>>> + Send + Sync + 'static;
fn instance<T: ?Sized + Send + Sync + 'static>(&mut self, value: Arc<T>) -> Result<()>;
// resolve 持有自己的解析链，不要求调用者持可变容器锁。
async fn resolve<T: ?Sized + Send + Sync + 'static>(&self) -> Result<Arc<T>>;
```

- 接口绑定用 `T = dyn MailTransport`，工厂返回 Arc<dyn MailTransport>；不得要求用户把 trait 换成字符串。内部以可 Any 下转型的 sized 槽保存 Arc<T>，不对 dyn Trait 本体做 Any 下转型，不使用 unsafe。
- 四种常规登记在 seal 前以后写定义替换先前同键定义，匹配绑定覆盖与已有 insert；保留 `try_*` 冲突拒绝变体只作为需要冲突检查的低层入口，不改变常规覆盖默认。
- 封口前不允许 resolve，因此替换不会留下旧缓存或依赖链。seal 后重绑返回 container_sealed；运行时 rebinding/extender/contextual binding 等 Laravel 能力进入 R2 后继，不伪称全部容器 API 已对齐。
- instance 为 singleton 等价生命周期，但资源归属不自动推断；调用者显式登记清理。AppState/Inject 改为此容器的句柄，不再保留独立服务 HashMap。
- 不做 PHP 自动反射构造。Rust 以显式工厂写出依赖；后继 derive 若提供，也只生成同一 bind 调用，不成为新容器。

### 构造、并发与失败

- Resolver 包含 Container 只读句柄、当前 lifecycle scope、当前解析链及资源归属。构造前标记 TypeId，遇到重复报告 service_cycle 和完整类型链，不堆栈溢出。
- singleton/scoped 对同键并发解析只允许一个在途构造，其他调用异步等待；不持 std Mutex/RwLock 跨 await。缓存按 (binding identity, App/scope identity) 隔离。
- 仅链内检查不足以发现两个并发根分别构造 A→B、B→A 的等待环。实现还须维护短锁保护的在途等待图，新增等待边前查环；取消/失败用 RAII 移除边、释放占位并通知等待者，不能挂死。
- 当前尝试的等待者得到同一次构造错误；失败不永久缓存成功槽。后续独立 resolve 可重试一次新的尝试，重试策略由业务调用决定，容器不内置无限重试。
- 可通过 `require<T>()` 声明启动必需服务根，在 FinalizeContributions 前解析。boot 内依赖与 require 根的缺失/环/构造错误均启动失败。
- opaque factory 的未执行分支不能凭空静态获知：未触达的 lazy/transient 分支在首个 resolve 报错。不能声称启动已经证明全部工厂路径。领域 provider 必须 require 其提供服务所必需的启动根，公开测试再覆盖其他分支；完整依赖元数据/derive 为后继增强，不提前运行所有 transient 工厂伪造校验。

| 错误码 | 条件/输出 |
| --- | --- |
| service_not_bound | 请求类型与安全解析链；无秘密值 |
| service_cycle | 同链或跨在途等待环，列类型链 |
| captive_scope_dependency | singleton 尝试直接或经 transient 捕获 scoped 服务；这是长驻进程安全增强，不能称 Laravel 同行为 |
| service_initialization_failed | provider/type/阶段，保留内部 source，公开消息脱敏 |
| container_sealed / scope_closed | 封口后重绑或关闭后新解析 |
| provider_dependency_missing / provider_order_invalid / provider_cycle | 清单校验失败，boot 不执行 |

## 6. 请求、任务与 Tokio scope

提供 `Container::scope() -> Scope`、`Scope::resolve<T>().await`、同步 `Scope::close()` 与 `Scope::shutdown(deadline).await`。`close()` 立即封闭新解析、取消受管子任务并请求父级受管清理；需要确认回调完成时显式 await `shutdown` 或应用最终 `App::shutdown`。Scope 自有缓存/取消/关闭登记，根 Container 同时持有 singleton 缓存及独立 root lifecycle 的 scoped 缓存；两者不混用，子 scope 不继承父缓存。`forget_scoped_instances` 异步封闭当前 lifecycle generation，拒绝该旧代新解析，等待/取消其受管构造并逆序执行已登记关闭；清理完成后发布新代，下一次 resolve 新建。旧代值不写入新代；已逃逸 Arc 不能强制销毁。A阶段曾只提供封闭机制；B/C 已补受管异步清理，不能以缓存释放冒充关闭完成。作用域在框架入口自动建立，业务通常不手动创建。

| 入口 | 创建与销毁边界 |
| --- | --- |
| HTTP | middleware 为一次请求建 scope；保持到 response body 结束/丢弃，包括 stream/SSE；取消后回收，不在 handler 返回时提前销毁 |
| Queue | 每次 job attempt 建新 scope，成功/失败/超时/取消均关闭；重试不会复用上一 attempt |
| Console | 一次命令一个 scope；长驻 worker 命令自身 scope 不代替每个 job scope |
| Scheduler | 每次触发执行一个 scope，不能把用户/租户上下文存入 scheduler singleton |

- singleton 的 factory Resolver 不携带 scoped 解析权限；即使从请求首次触发 singleton，也不能解析 scoped。经 transient 间接捕获 scoped 同样拒绝，避免长驻 worker 注入旧用户。
- scoped/transient 通过当前 Resolver 保留有效 scope；从根/boot 解析 scoped 使用启动 lifecycle，同一 lifecycle 两次同实例，forget_scoped_instances 后创建新实例。此行为对应固定 Laravel ContainerTest 的 scoped/reset 断言，不把 scoped 静默提升永久 singleton。当前 Inject<T> 从请求 scope 解析，再按生命周期获得相同底层类型实例，不从根表旁路。
- 复用 ExecutionContext 的 metadata/预算/取消；Tokio task-local 不自动跨 spawn。默认普通 tokio::spawn 不继承请求 scope；显式克隆根 Container 时仅能解析 root lifecycle，不获得请求身份。需要共同存活的任务使用受管 scope.spawn，关闭时取消并 join；独立任务必须创建新 scope，只显式传播允许的 metadata。
- `Scope::close()` 同步禁止新解析、取消受管任务并请求父级有界清理；实际 join、逆序回调与失败结果须通过 `Scope::shutdown(deadline).await` 或 `App::shutdown` 等待。Arc 已被业务显式克隆并逃逸时 Rust 无法撤回该对象；此限制必须公开，不承诺阻止用户主动跨作用域保存句柄。框架自身不能捕获/传播这种句柄。
- HTTP body drop 的同步 Drop 不执行 async 关闭；由 App 拥有的有界清理任务队列接管，App shutdown 必须 await 此队列，不产生失管 spawn。body cancel 与应用退出的清理各只执行一次。

## 7. 失败清理与可观测性

Resolver/BootContext 提供 `on_shutdown(name, FnOnce() -> BoxFuture<'static, Result<()>>)`。归属按构造缓存寿命：singleton→App，scoped→Scope，transient→当前 Scope（无 Scope 时为 App）。异步资源取得后立即登记关闭，才进行下一个可失败/await 步骤；连接池、listener、导出器与后台任务不得依赖 async Drop。

- 同一归属内按登记逆序清理，调用恰一次；startup fail、boot fail、正常退出和超时均走同一关闭状态机。
- 清理有共同 deadline，某回调失败仍执行后续回调；保留最初启动错误，附安全 cleanup 错误摘要。超时后取消并 join 框架拥有的任务，不能以第一项失败跳过其他清理。
- 不提供 shutdown 参数 App 引用给失败时尚未成型的应用；闭包捕获明确资源句柄。连接建立中被取消时由其 future/底层组件释放未移交资源，已移交资源由登记栈释放。
- 日志记录 provider/type、register/boot/resolve 阶段与耗时，不写配置值/身份令牌。复用现有 tracing，不新造日志内核。

## 8. 配置合同：复用与补强

### 已核实的加载链与缺口

只读配置审计与源码一致：ConfigLoader 当前从 FrameworkConfig 默认值开始，按领域文件名排序 merge，再合并 DB_*/REDIS_* 便捷值、PORT 等白名单别名，最后 namespace（如 APP_）覆盖；bootstrap 先选环境再加载 .env，进程变量优先。不增加字符串插值语言。

- config/mod.rs 的 figment 原先非fallible并静默略过read_dir/条目错误；R2-A2已改为Result并逐处传播，实际对应门禁已通过，不能当作完整配置schema/Provider已验收。
- FrameworkConfig 只覆盖框架根字段，DB/cache 等由插件 section 读取。全局根 deny_unknown_fields 会误拒合法插件/应用 section，不能这样修复。
- AppConfig/DatabaseConfig/CacheConfig 多数 serde(default) 未拒未知字段；`database.max_conections` 拼错可能回退默认。CacheStoreConfig.flatten options、动态 store/instance 名称本来开放，不能一并禁止。
- ConfigError 的 Figment 转换已有安全路径/类别；Invalid(String) 不会自动脱敏，领域 validator 不能写入原 DSN/密码。sensitive_section 当前丢失字段路径，应在不回显 value 的前提下补足。
- namespace flat env 映射已有特殊分支，cache store 名含下划线/更深 options 不是当前通用支持；不能根据一个成功键就宣称任意嵌套 env 已实现。

### 新元数据 API 与时点

`RegisterContext::config_section<T>(key, defaults: T, schema: SectionSchema, validate: fn(&T)->Result<()>)`，T: Serialize + DeserializeOwned + Send + Sync + 'static。它登记区段元数据与校验，不立即加载服务；对应 Laravel 包在 register 合并默认配置的职责。Resolver/App 继续使用同一 `section<T>(key) -> Result<T, ConfigError>`，不增加全局 GetAny。

- key 为明确区段路径；每个已注册 section 有唯一 owner 与 T/schema。同 key 不同 owner/schema 报 config_section_conflict；共享同一区段通过一次明确 owner 注册和公开扩展 schema，不能按 provider 顺序互相覆盖类型。
- SectionSchema 表达封闭字段、显式开放 map 子树/driver options、秘密字段与 safe-to-display 标量；不能只从默认 JSON 猜 schema，因为 None/空数组/空 map 不能描述完整字段。
- 所有模块 defaults 先合并，再加领域 TOML→便捷变量→namespace。核心默认值合并后执行基础 schema/typed validation。模块 overlay 保留对应 provider.boot 时点，发布版本化配置视图；每次 section 读取同一当前版本，已返回的 Rust owned 值不原地改写。boot 后再提取运行入口配置。
- 运行期重载不是本批功能；Ready 后配置变更拒绝。boot 内只有显式模块 overlay 可以更新当前配置；允许 earlier boot 与 later boot 观察不同值，与固定模块包一致。服务若在 overlay 前构造并保存 owned 配置，会保持当时值；这属于上游顺序可观察性，不能全局预合并抹除。

### 合并、未知字段与目录错误

| 情况 | 选定合同与回归 |
| --- | --- |
| 标量 | 后层覆盖，类型错误失败；不因 parse 失败回退默认 |
| 命名 map | 按名称及其字段递归合并；保留未覆盖的其他 store/instance。DB 每个 instance 的最终缺失值只取该实例类型默认，绝不继承默认连接配置 |
| 核心来源数组 | 领域文件/env 后层整组替换，先用 Figment 实测锁定；不冒充模块包数组语义 |
| 模块 boot overlay | 固定 nWidart v13.2.0 的 array_replace_recursive(existing,module)：模块值覆盖既有标量，map递归；数字索引逐项替换且保留未覆盖尾部，空数组不清空现有尾部。两个 provider 按实际 boot 顺序覆盖；禁用模块不参与 |
| 已注册 section | schema 关闭处拒未知字段，错误保留完整路径；可开放 driver options/动态名称节点，仍校验其已知公共字段 |
| 未安装模块/应用自有 section | 保留来源并允许 Application::Config/section<T> 使用，不对全树一刀切拒绝；只有其 provider 注册后才按相应 schema 严格校验，不宣称任意顶层拼写均能诊断 |
| 显式配置目录 | APP_CONFIG_DIR/自定义namespace CONFIG_DIR 或 ConfigLoader::new 显式路径：空值、缺失、非目录、不可读及目录条目读取错误均失败，不略过 |
| 未显式默认目录 | 默认相对 config 不存在可按当前源码使用方式保留内置默认；目录存在但不可读/损坏仍失败，错误明确来源与规则 |
| TOML 文件错误 | 已发现文件读取/解析/字段校验失败立即错误；不跳文件、不忽略未知已注册字段 |
| env 映射 | 保留既有便捷变量与 namespace 优先；不借 R2 默改含下划线命名 map 的含义。完整 escaped/深层映射需配置子簇新增精确语法与迁移测试 |

R2-A2已将ConfigLoader.figment改为fallible并逐处传播；仍只有一个loader入口。数组和 map 回归先建立，再接 provider defaults。

秘密值永不进入错误、Debug、config show、日志与测试快照。使用 schema 标记叠加现有 redact_secrets；未知或未分类字段默认不回显原值。错误带 key/path、来源和约束/类型；只有明确标安全的 bounded 数值/布尔值可附原值。公开 validator 返回安全消息，内部 source 保留结构化诊断而不透出原值。Redis 仍为标准生成应用默认，未知 store/driver 或失联明确失败，不回退 memory/file。

### Cache / Filesystem / Queue manager 的 Provider 可见时点

固定 Laravel v13.39.0 的 [CacheManager::store/extend/forgetDriver/purge](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Cache/CacheManager.php)、[FilesystemManager::disk/extend/forgetDisk/purge](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Filesystem/FilesystemManager.php) 与 [QueueManager::connection/extend](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Queue/QueueManager.php) 均按名称首次构造并缓存；后注册或替换 creator 对尚未解析的名称可见，已缓存实例保持原状。Cache/Filesystem 显式忘记或 purge 后可按当前 creator 重建；固定 QueueManager 无同名 purge API，不虚构等价。

- register 登记完整可解析的 manager factory 与配置约束，不用仅含空 `OnceLock` 的 facade 冒充已构造服务。boot 可 resolve manager、调用其公开 `extend` 或贡献 job/schedule；使用时点由 provider 顺序决定。后注册 driver 不全局冻结，也不自动追溯替换已解析实例。框架 `CacheDrivers::register` 的重复拒绝是声明式登记；运行时 `extend` 的同名替换须作为不同公开操作。
- Rust driver 初始化是 async，具名 `store`/`disk`/`connection` 首次解析因而是 async；同步旧调用方、模板及 consumers 必须随生产切换迁移。相同名称并发只发布一次成功实例，失败可重试，不同名称互不串行；已缓存对象允许明确 purge 后重建。
- 默认实例在 Provider boot 预解析并 fail closed，未使用具名实例保持懒构造。最终配置的未知 driver、非法公共字段和可静态校验的 driver options 必须在 Ready 前拒绝；晚 overlay 不得让已构造服务与最终配置互相矛盾或绕过验证。连接成功后立即登记受管回收；现有第三方 backend trait 无 async close 时只能报告所有权释放，不能声称远端连接协议已关闭。
- Queue 还须独立区分 job handler/schedule 贡献与 named connection/driver creator；前者在 worker 启动前封口，后者遵守同一时点规则。现有单 driver facade 不等于 Laravel named QueueManager，未实现前保留 R2 缺口。

## 9. Plugin 迁移与分批实施

不接受把旧 build 全部搬到 register、把 finish 改名 boot 就宣布完成。旧 build 中即时读取服务、打开连接、注册路由的部分必须逐个归位。最终 App/Inject/命令/队列均使用同一 Container，旧 Registry 与 Plugin 执行循环移除；0.x 不新增长期兼容壳。

| 子簇 | 依赖 | 可编译闭环与完成边界 |
| --- | --- | --- |
| R2-A 容器与配置基元 | R1-S 34dfb5e | 新 Container/factory/错误、root/子 scope/reset 与 Figment 合并/schema纯测试；不切生产 bootstrap，不称 Provider 已接入。只有旧引擎在运行，不建立第二生产内核 |
| R2-B0 内置资产拆责 | A contract | 按 HTTP/DB/Cache/通信/异步等文件族，把现 Plugin 中绑定声明、资源构造、贡献拆成可迁移函数；旧运行路径仍唯一，逐簇保持行为门禁 |
| R2-B1 单引擎切换 | A、B0 全部就绪 | 一次集成 ServiceProvider清单/基础配置/顺序boot overlay/最终配置，迁完全部feature下内置Plugin、当前示例公开接入、CLI模板、consumers与公开调用；移旧循环/服务表，无 legacy/new 模式双栈 |
| R2-C 入口 scope 与清理 | B1（root/手动scope基元已在A） | HTTP/queue/console入口、body生命周期、managed spawn、失败逆序关闭；真实取消/跨请求/attempt隔离，不能只测Container.scope工厂 |
| R2-D TestApp 最小路径 | B1，scope相关依赖C | 同一bootstrap，provider/instance测试替换，实际公开consumer与模块可移除测试；不实现另一应用容器 |
| R2-E 全量容器/Provider后继 | B1–D | 逐项补 contextual/aliases/tags/rebinding/deferred provider/解析回调及Laravel已纳入行为；每项公开例子/必要差异/测试，不因首批没做而移出矩阵 |
| R13 模块管理/生成 | R2依赖/清单稳定 | 模块生成、启停/管理/依赖诊断和发布资源体验；静态清单重启只证明基础接入，不能算完整管理 |

首批 Facade 采用已存在的可替换类型化 handle 与显式 DI，测试通过同一容器替换；不新建静态全局实例。Laravel 式静态便捷入口在 C/D 之后落地，必须从当前 App/scope 解析，无上下文明确失败，支持同进程两 App 与并发测试隔离；在 E 中保持未完成，不以显式 DI 声称 Facade 全量等价。

## 10. 首批文件所有权与验收

R1-S 结束后领取基础代码；以下是下一批分工建议，不在本合同编写期间派发或修改文件。所有人只在本仓 main，worker 不递归派发；共享文件由集成负责人串行。

| owner | 独占文件范围 | 可并行交付 |
| --- | --- | --- |
| Container worker | 新 src/core/container.rs 及其内嵌测试（需要拆目录先由集成者确认） | binding生命周期、trait object、覆盖、singleflight、环与取消；先不改 app.rs/mod.rs |
| Config worker | src/config/mod.rs、配置schema实现及配置内嵌测试 | fallible目录读取、defaults/来源/数组map/严格边界/脱敏；不改生命周期与Cargo |
| Provider worker | 新 src/core/provider.rs 与独立纯状态机测试 | 清单顺序/注册屏障/boot/资源清理上下文；依Container/Config已定公开签名，不另写bootstrap |
| 集成负责人 | src/core/app.rs、plugin.rs、lifecycle.rs、mod.rs、src/lib.rs、Cargo/lock、HTTP/queue入口、根bootstrap/清单、模板、共享文档与提交 | 串行整合；B0各能力文件另明确排他范围再并行，不让多人同时改manifest/路由公共接口 |
| 独立审查 | 全部只读 | 由不同模型/Provider核对生命周期、scope捕获、清理、配置秘密与Laravel任务等价；审查不代替运行 |

### 必须能发现真实缺陷的测试

- 所有 register 完成前任意 boot 都不能运行；A.boot 解析后登记的 B 绑定成功；register 不能提前拿到 resolve/route能力（compile-fail consumer或API表面检查）。
- 普通 bind 每次不同；singleton 同 App 并发只构造一次、两 App 不共享；scoped 同 scope 一次、不同 request/job attempt 不串；trait object可绑定替换。
- 缺 binding、同链环、跨并发根等待环、工厂错误、工厂被取消均有界返回；后续调用可恢复，不永久占位；singleton经transient间接解析scoped被拒绝（明确安全增强）；root scoped复用/reset后更新、不同App隔离。
- provider缺失/顺序/环、重复清单、配置section冲突、两模块资源同键冲突；启动失败前无业务listener，后登记根服务可供前provider boot解析。
- startupfail在第二个连接或boot之后发生：已创建资源按逆序各关一次，受管任务取消/join；清理一项失败仍清理其余且保留首错。
- HTTP两并发用户/租户、两job attempt、流body未结束/取消、command结束与scope child任务回收；用实际入口验证，不仅手工scope单测。
- defaults→TOML→便捷env→APP_、进程>.env；命名map保留兄弟项、实例不继承默认连接、数组替换/清空；显式坏目录/损坏条目/未知字段不回退；secret sentinel不在任何错误/Debug/show输出。
- 两普通模块各自provider/路由/typed资源，只改应用声明清单即可组合；移除一个后其路由404且框架health与另一模块成功，未触发被移除模块的初始化。
- 模块overlay必须测同键/嵌套map/数字数组留尾/空数组/两个模块顺序/禁用模块；前序boot读旧值、对应boot应用overlay、后序boot读新值。
- 可选DB/view provider无配置声明时不实例化；声明错误或不可达时明确启动失败，不静默跳过或回退。
- TestApp替换Mailer/业务接口，fake不外发；generated应用与modules/example复用同Provider流程，缺条件的外部验收明确标缺口。

### Laravel 文档任务对照门禁

| 官方章节/任务 | OpenRust 等价流程 | 必要 Rust 差异 | 实际验收入口 |
| --- | --- | --- | --- |
| Providers / register、boot | 在应用清单注册业务provider；register绑定，boot装配 | BoxFuture显式await，静态类型清单 | 生命周期屏障、后声明依赖、顺序/失败测试 |
| Container / binding interfaces、singleton、scoped | 绑定dyn接口并注入，按request/job复用 | 显式factory/Arc，无PHP反射；Arc逃逸限制公开 | trait object consumer、并发App/request/job tests |
| Container / instance与testing替换 | seal前应用/测试override，业务调用不改 | Rust不可变已封口定义；运行时rebinding仍E待补 | 生产/替换两App隔离与fake断言 |
| Lifecycle / provider后路由分发 | boot全成功再Ready、HTTP才处理 | 长驻Tokio要显式scope/body回收 | 真实TCP启动失败、流取消、退出资源断言 |
| Packages / defaults与资源 | 模块登记typed defaults/schema，boot贡献自有资源 | Figment/类型schema，不解释PHP配置 | 两模块公开consumer、配置冲突/资源冲突 |
| Scoped / Octane与queue生命周期 | 每request/attempt新的scoped cache | task-local不自动跨spawn，受管显式传播 | 请求并发、重试attempt、后台任务取消 |
| Container / 更完整能力与Facade | 按E逐项同容器补齐 | contextual/静态便捷入口须隔离App和scope | 保持待办，未测试不宣称完整Laravel实现 |

每源码子簇按 AGENTS 执行 fmt、workspace clippy/test/doc、deny及适用feature/MSRV；数据相关另PG18/MySQL8.4真实集成，模板生成后check/test/HTTP。最终记录实际命令、结果、跳过项与独立审查；本文件只做文档diff/链接/旧口径检查，不运行代码门禁、不预填通过记录。

## 11. 独立审查修订依据

- Laravel固定源码 ContainerTest.php:144–165：root make scoped 合法，forgetScopedInstances 后重建。见 [固定测试](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/tests/Container/ContainerTest.php)。root scope并非Rust不可实现的差异。
- nWidart固定 ModuleServiceProvider boot 的 merge_config_from：模块值覆盖existing且保留boot时点。见 [固定源码](https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/Support/ModuleServiceProvider.php) 与 [模块矩阵B017–B020/T17–T20](laravel-modules-matrix.md)。
- singleton拒绝捕获scoped是长驻Rust进程的授权安全增强，不作为Laravel原行为；如果暂不实现该增强则必须记录范围，不可漏掉实际root scoped行为。
- R2-A先提供未接生产bootstrap的容器基元与公开测试，旧生产启动引擎仍唯一；B1切换后删除旧Registry/Plugin执行路径，不保留双内核运行模式。

## 12. R2-A 容器基元本次实施边界

- `src/core/container.rs` 已实现 typed factory、dyn接口、覆盖、缓存、singleflight、同链/跨根环、取消恢复、root scoped/reset与独立scope。尚未接生产AppBuilder或Provider，不代表生命周期全闭环。
- builder的bind/singleton/scoped/instance当前返回 `&mut Self`，登记阶段不解析、不产生可恢复错误；`build(self)`消费定义。上文概念签名的Result由Provider RegisterContext包装时保留，不通过无失败分支制造Result。
- `scope()` 可能返回标识容量错误；本次 `Scope::close()` 与 `forget_scoped_instances()` 同步关闭解析并释放缓存。尚无on_shutdown/managed任务登记，所以不以unused async空壳冒充资源回收；B1前须增加异步有界清理协议并同步所有调用。
- Resolver只持Weak容器句柄，resolve时临时upgrade；全部外部容器释放后报告container_closed，避免singleton保存Resolver形成强引用环。保存Resolver后延迟解析只保留仍活跃的构造帧，已结束帧不误报循环。scope关闭必须同时中止当前scope的singleton owner/waiter解析，不能因为singleton缓存归App就让已关闭scope永久等待。
- 本次定义封闭，运行时重绑/aliases/contextual/tags/完整Facade仍在后继矩阵；当前容器不使用Registry的第二生产启动路径。
- 统一错误增加保留原始source的ServiceInitialization；Error的Debug只输出code/status，公开HTTP仍脱敏。根/子scope已逃逸Arc仍由调用者持有，容器无法强制撤回。

## 13. 配置子簇实施前的核心/模块合并区分

固定 Laravel ServiceProvider.php:154–178 提供两种不同操作：`mergeConfigFrom` 使用浅层 `array_merge(defaults, existing)`，existing同名顶层键整体覆盖；`replaceConfigRecursivelyFrom` 使用 `array_replace_recursive(defaults, existing)`，existing递归覆盖。固定 nWidart 模块boot使用相反方向的 `array_replace_recursive(existing,module)`，模块值胜。不能把三者统一成Figment递归低优先defaults并声称等价。

来源：[固定 Laravel ServiceProvider](https://github.com/laravel/framework/blob/d52c3b3129e4c8e213c97cdc68f94b0b7d27f181/src/Illuminate/Support/ServiceProvider.php)。R2配置子簇须明确typed schema/default注册和这两个包配置操作的不同职责，分别测试顶层标量、嵌套对象整体覆盖/递归、数字键追加或替换、空数组、两个provider顺序；模块overlay另按B017–B020覆盖反向与boot可见性。首批容器基元不实现配置操作，不转记为这些行为已通过。
