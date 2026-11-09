# 独立模块扩展

`openrust-modules` 对应固定 nWidart laravel-modules v13.2.0，默认集成于根应用。框架公开库名仍为 `openrust`，模块类型由 `openrust_modules` 导入；应用→模块包→框架单向依赖，同一 Container / Provider 负责全部启动与资源回收。

```rust
use openrust::foundation::ProviderBuilder;
use openrust_modules::{ModuleManifest, ModuleProviderBuilderExt};
let mut builder = ProviderBuilder::new();
builder.module_manifest(ModuleManifest::new());
```

根应用在 `ApplicationDefinition::configure_providers` 安装清单，代码配置类型为 `openrust_modules::ModulesConfig`。测试的 `ModuleTestAppBuilderExt::remove_module` 通过通用 Provider 蓝图重放，reset 不改持久状态。显式描述提前校验并绑定仓储，发现清单在 register 读取配置；模块包没有独立生命周期执行器。

本包尚未通过迁移后运行验收，完整模块生成器、aliases、deferred 与其余官方能力仍按唯一待办推进；不把拆包当作全量实现。

## 编译模块与持久启停

根应用和生成应用默认选择 `registered`，使用 `ModuleManifest::new().with_application_root(root)` 的显式描述与启用名称；同一仓储仍管理路径、依赖和 Provider。库 `ModulesConfig::default()` 保留上游 `file` 默认；下列发现/文件流程是应用显式选择的可选模式。只读激活器在状态变更前返回错误，不派发前事件或在空目录 `--all` 时误报成功。第三方可写激活器须覆盖 `ModuleActivator::ensure_writable`，再实现持久动作；文件驱动已实现。

`ApplicationDefinition::configure_providers` 与 `ModuleProviderBuilderExt::module_manifest` 在一次启动中只产生一份清单。`ModuleManifest::discover_from_config` 在包 Provider 的 register 中扫描 `module.json`，读取同一 `ConfigRepository` 已发布路径；`compiled_provider` 把运行名称映射到已编译类型工厂，真实调用即时进入唯一注册器。未编译 disabled 元数据可发现并持久启用，实际加载缺失绑定时失败。重复声明不预先去重，Provider 去重仍归唯一内核；仓储同名项以后来源覆盖，lookup 先大小写不敏感精确匹配，再 Studly 回退。状态写 canonical 精确键，不运行未编译 Rust。

`get_path()` 动态读取配置而不重扫既有快照。`possible_nested_providers` 仅声明条件可达类型，原平面 missing/order 预检保留；候选未被选择时不能满足真实登记点的依赖。必登记的 `nested_providers` 仍在父成功后核实际登记。

`ModuleFile` 是显式编译的初始化器，在模块 eager Provider 注册后、普通 boot 前执行，按解析路径 `include_once`；不建立文件私有生命周期。`compiled_module_file` 跟随元数据的文件包含目录覆盖，`compiled_file` 则按应用根绑定。`ModuleExecution` 默认每 App 独立，可由多个 App 共享；共享时并发等待，失败/取消明确封闭未完成文件，新会话才可重试，这是 Rust 异步安全差异。共享 Manifest 即使空也复用，空 catalog 可再扫描；不提供热卸载。PHP 类 aliases 明确未实现，非空表拒绝；完整 deferred `ProviderRepository` 亦不由 eager 注册替代。运行发现与文件初始化的使用方法见 [CLI 模块说明](../../../tools/cli/README.md#接入自己的业务模块)。

显式程序化 `ModuleDescriptor` 清单仍保留 requires/priority 增强，与发现输入不可混用，不将增强拓扑称为上游 Manifest 排序。

`Module::is_loaded()` 是本 App 的加载快照，`is_enabled()`、`is_disabled()` 和 `has_status(bool)` 读取所选激活器视图：默认 file 保构造期缓存及自身修改，不跟随其他进程；显式 file-live 扩展才读取新文件。`enable().await` / `disable().await` 派发前事件、持久化，再派发后事件；`set_active(bool)` 直接写而不派发这四事件。自身状态在写前改变，写失败保留缓存变更但不发成功事件，不能把 enabled=true 当持久化已经成功；当前写结果与磁盘字节须分别判断。已有 `ModuleManifest::enable/disable` 仍只改变当前启动输入，不是持久管理接口。

```rust
use openrust_modules::ModuleRepository;
# async fn change(app: &openrust::Application) -> openrust::Result<()> {
let modules = app.state().resolve::<ModuleRepository>().await?;
let module = modules.find_or_fail("example")?;
let loaded = module.is_loaded();
module.disable().await?;
assert!(!module.is_enabled()?);
assert_eq!(module.is_loaded(), loaded);
# Ok(())
# }
```

扩展命令为 `module:list`、`module:enable 名称…` 与 `module:disable 名称…`。启停支持 `--all/-a`、`--no-interaction/-n`；all 包含 disabled、覆盖显式名单，空 catalog 的 all 成功。全部名称先验证，再保留顺序和重复项执行；单项事件或写入异常停止后续项，已经提交的前项不回滚。`module:list --only enabled|disabled|priority` 按状态过滤；priority 只列 enabled，并支持 `--direction asc|desc`。TTY 搜索选择及通用批命令的 false/非零聚合仍待实现，不冒充完整上游交互。

命令仍完成同一应用的完整 bootstrap，坏 JSON、缺失启用模块/依赖、资源或服务启动失败也会阻挡管理命令；修复这些启动输入后再运行，不提供第二启动器或自动绕过。停用后当前 App 继续持有原 Provider/路由和资源，下一 App 才按新状态加载；状态查询与已加载事实需要分别观察。

生命周期通知沿同一 `Dispatcher` 服务派发 `ModuleLifecycleEvent`，阶段是 enabling/enabled/disabling/disabled，名称为 `modules.<声明名转小写>.<阶段>`。用 `EventsServiceProvider::listen_named::<ModuleLifecycleEvent, _>("modules.*.enabled", callback)` 登记命名/通配监听；回调返回 `EventPropagation::Stop` 只停止当前传播，不取消启停动作，返回 Err 则传播异常。前事件失败不写，写失败不发后事件，后事件失败时状态已经提交。同名 exact 先于 wildcard；wildcard 按模式首次登记顺序、同模式组内登记顺序执行。字符串身份不因载荷类型相同调用其他类名的 typed/queued 监听；不要另外注册空的同类型 Provider 覆盖业务监听器。

标准应用的默认 Provider 清单登记核心 `EventServiceProvider`；模块本身不补位事件服务。裸 `ProviderBuilder::new()` 要执行启停事件时须显式安装核心，再添加 `EventsServiceProvider` 业务监听贡献者。未安装核心时仍可加载和查询模块，`enable/disable` 返回 `module_events_unavailable`，不会写状态文件；显式排除核心也不会由模块恢复。

模块句柄复用弱 Resolver 访问同一根事件服务；命令和资源继续沿既有生命周期回收。调用方仍需协调并发停机与管理操作，不能依赖并发 `shutdown` 提供状态文件提交的原子屏障。文件锁/替换、hardlink 等价和 Windows 原生待验边界见 [配置说明](#模块激活与运行根)。


## 模块激活与运行根

根应用和生成应用的 [`config/modules.rs`](../../../config/modules.rs) 显式提供以下策略；`modules` 是同一配置仓储内的区段，不是另一份状态仓储：

| 配置键 | 默认值 | 代码配置读取的部署变量 |
| --- | --- | --- |
| `modules.activator` | `registered` | `MODULES_ACTIVATOR` |
| `modules.activators.file.statuses-file` | `modules_statuses.json` | `MODULES_STATUSES_FILE` |

`Configuration.application_root` 是应用运行根锚点，缺省为当前工作目录；模块资源仍从该根下的 `modules/<模块名>/` 解析。`ModuleManifest::from_config(&root, &config.modules)` 将状态文件的相对路径锚定到同一应用根，绝对路径保持原值。例如可将状态单独放在 `state/modules.json`，而不移动模块源码、语言或视图。不会自动搬迁旧状态文件；`APP_CONFIG_DIR` 也不会替应用选择资源根。生成的 `modules::manifest(root, &config.modules)` 使用同一入口。

库的 `ModulesConfig::default()` 使用 `file`，与上游包默认一致。`file` 在实际激活器构造时读取一次，后续 `snapshot/has_status` 使用该实例缓存，外部进程写文件不会刷新它。自身 set/delete 先更新缓存再写，写失败时缓存保留变更，后续成功写包含尚未持久化的变更。可用 `MODULES_ACTIVATOR=file-live` 选择实时读取扩展，复用同一状态路径。`FileModuleActivator::new(path)` 返回 `Result`，坏文件在构造处失败，手工恢复可显式调用 `FileModuleActivator::reset_file(path)`。

`file` 通过同一模块清单选择；自定义激活器须先用 `register_activator` 登记，未知名称启动失败。源码清单中的 `ModuleManifest::enable/disable` 只调整本次构建，持久写使用 [模块管理服务与命令](#编译模块与持久启停)。普通只读自定义激活器的写操作明确拒绝，不回退到默认文件。

状态文件只认模块声明名的精确键和 JSON 布尔 `true`；缺文件、缺键、null 或其他值不启用，损坏 JSON 不当作空文件覆写。`Module::is_disabled()` 是启用判断的否定；`has_status(false)` 则按精确布尔规则比较，缺失/null 视为 false，其他非布尔值不等于 false。未知键和值会保留，但不会成为可执行模块。模块加载是每次 App 的快照；激活器实例状态与磁盘持久化结果分别观察，写入 Err 不代表内存未变；它们不更改当前已加载的 Provider、路由或迁移历史。配置缓存可固定激活器及状态路径，**不缓存状态 JSON 内容**；改选路径后如已启用配置缓存，须显式重建或清除缓存。

默认缓存视图保持独立，文件写在协作式 sidecar 锁内重读，保留其他键，经同目录临时文件同步后原子替换；忙锁报 `module_state_busy`，由调用方决定重试。事件和用户回调不在文件锁内执行；替换是提交点，提交后的事件失败不回滚状态。缓存中 null/missing 的 delete 不写，文件已消失但缓存仍有记录时 delete 重建剩余内容；reset 即使文件已消失也清缓存。严格 IO 错误仍显式返回 Err，这是保留增强，区别于上游 reset 可能忽略底层 delete=false。空表和连续数字键写成 PHP 列表形状；外来非空列表或等价 numeric 对象拒绝，不能覆盖未知坏文件。只有同一实例对同一解析目标已成功提交且字节精确匹配的列表，才允许自身后续读写恢复。路径按符号链接目标协作；不同 hardlink 路径的等价锁定/替换语义及 Windows 原生文件系统行为仍待验收，不作已支持保证。与并发 `shutdown` 之间也不保证原子提交屏障；应用应协调管理动作与停机，而不是把一次生命周期检查当作事务锁。

`paths.modules` 默认 modules（获准目录约定），`scan.enabled` 默认 false、`scan.paths` 默认 vendor/*/*；主路径与状态文件独立。运行发现读取 module.json，按声明调用已编译 Provider/文件初始化绑定，不能执行未编译 Rust。`MODULES_PATH` 覆盖主路径；metadata 读取在包的 register 阶段，仓储 `get_path` 动态读当前配置，不热挂卸资源。
