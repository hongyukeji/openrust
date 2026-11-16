# 01-02 容器与延迟 Provider 整单元验收

固定 Laravel 13.40.0 的容器目录有 11 个测试文件、208 个测试方法。另核对 Provider 仓库的 6 个测试方法及 18 个容器属性。下表按契约族归并，OpenRust 的测试不要求与 PHP 方法逐个同名。 [固定容器测试](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Container) · [固定 Provider 仓库测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Foundation/FoundationProviderRepositoryTest.php)

| 固定 Laravel 测试 | 方法数 | OpenRust 验收 |
| --- | ---: | --- |
| ContainerTest | 89 | 容器基础、条件绑定、作用域、重绑定和绑定目录。覆盖类型和名称、别名循环、绑定与解析状态、构造默认值、名称与类型化参数、环境匹配、条件属性和清理。[条件](../01-02a/README.md) · [作用域](../01-02b/README.md) · [重绑定](../01-02f/README.md) · [参数覆盖](../01-02i/README.md) · [构造](../01-02j/README.md) · [目录和清理](../01-02n/README.md) · [下标](../01-02q/README.md) |
| ContainerCallTest | 18 | 闭包、静态和实例方法、公开命令入口、命名参数、类默认值、变参以及失败后上下文恢复。Rust 编译登记取代 PHP 可调用反射。[调用](../01-02l/README.md) · [延迟调用](../01-02m/README.md) |
| ContainerExtendTest | 11 | 已绑定、未绑定、已有实例、别名及上下文扩展，以及延迟应用、多个扩展、解绑和重绑定。[扩展绑定](../01-02e/README.md) |
| ContainerResolveNonInstantiableTest | 3 | 缺失抽象值的默认值、变参回退及参数覆盖恢复。[静态构造](../01-02j/README.md) · [变参](../01-02k/README.md) |
| ContainerTaggingTest | 3 | 标签按登记顺序延迟解析，保留重复项并可反复遍历。[上下文和标签](../01-02c/README.md) |
| ContextualBindingTest | 27 | 最近消费者规则、实例隔离、别名更新、缺失参数、配置默认值、标签和变参，以及方法上下文与注册工厂回退。[上下文和标签](../01-02c/README.md) · [变参](../01-02k/README.md) · [调用](../01-02l/README.md) |
| ContextualAttributeBindingTest | 23 | 基础属性、类型及标量解析、方法与构造、请求及路由、Context、服务管理器和认证属性。[属性及回调](../01-02s/README.md) · [基础属性](../01-02x/README.md) · [请求属性](../01-02y/README.md) · [管理器属性](../01-02aa/README.md) · [认证属性](../01-02ab/README.md) |
| AfterResolvingAttributeCallbackTest | 4 | 参数、类、构造以及方法回调和手动派发。[属性及回调](../01-02s/README.md) · [公开回调](../01-02z/README.md) |
| ResolvingCallbackTest | 25 | 全局和服务回调、别名、接口和实现投影、上下文参数、顺序与重绑定事件隔离。[回调](../01-02d/README.md) · [重绑定](../01-02f/README.md) · [接口事件](../01-02w/README.md) |
| RewindableGeneratorTest | 2 | `TaggedServices` 从登记清单提供长度，每次迭代重新解析。Rust 迭代器与长度接口表达 PHP Generator 的计数和回绕。[标签](../01-02c/README.md) |
| UtilTest | 3 | 利用编译参数描述、类型键、显式默认值或闭包回调区分参数和列表。PHP 反射及弱类型值包装由 Rust 类型和迭代契约表达。[构造](../01-02j/README.md) · [变参](../01-02k/README.md) |
| FoundationProviderRepositoryTest | 6 | 编译进二进制的 Provider 目录保存 eager、服务键和事件触发元数据。编译与应用装配替代 PHP 的磁盘 manifest 读写、重建和目录权限检查。用实际启动验证惰性工厂、按键加载、事件加载、顺序、失败和隔离。[延迟键](../01-02g/README.md) · [异步工厂](../01-02h/README.md) |

18 个属性分为基础 `Config`、`Give`、`Tag`，类型绑定和生命周期 `Bind`、`BindWhen`、`Scoped`、`Singleton`，请求 `RequestAttribute`、`RouteParameter`，组件 `Cache`、`Database`、`DB`、`Log`、`Storage`、`Context`，以及认证 `Auth`、`Authenticated`、`CurrentUser`。条件及生命周期使用编译描述，`Context` 有公开、隐藏和动态默认值回归。 [绑定](../01-02t/README.md) · [生命周期](../01-02u/README.md) · [Context](../01-05/README.md)

## 事件触发 Provider

复用 v0.x 区分服务触发与事件触发的契约及弱引用监听器，接入当前 ProviderRepository 的并发就绪状态。事件注册保持延迟键，失败可用后续事件重试。完成 boot 后触发会等待该 Provider 的 boot。启动时按实际成功注册次序执行 boot。普通事件注册只进入常规列表，boot 前由服务键装载时同时具有 booting 回调。将注册的 Rust 类型名作为事件监听结果，保留 `until` 在首个非空结果结束的行为。 [固定 Laravel ProviderRepository](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/ProviderRepository.php) · [固定 Goravel 装配](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/application.go)

双应用的真实 TCP HTTP 验证事件与服务键并发装载、每个应用只注册一次、失败后的再次触发、HTTP 500 隐藏错误详情。单独核对 bootstrap 注册期间的事件顺序和服务键保留，以及原有服务触发路径 boot 顺序。新增事件监听器使用弱引用，应用和 Provider 由原有目录拥有。

## 整单元验收

隔离基线 `a1620b87d`。Rust 1.94.0 的 231 项框架容器／事件及 26 项根应用定向回归通过。额外真实进程测试启动 16 个子进程，覆盖容器 Artisan 命令、未知命令和启动参数退出码。框架库及事件测试、根应用延迟生命周期测试通过严格 Clippy，实际 `main artisan inspire` 通过。以并行提交后的 `e5103489c` 基线复验 17 项延迟生命周期回归、16 个 Artisan 子进程和实际 `main artisan inspire`。

三语 Provider 示例分别编译和运行，检验事件派发、服务读取及共享实例。VitePress 构建通过，根路径及 `/docs/` 下通过 12 项浏览器回归和 6 项事件章节锚点检查。 [英文](../../../../docs/en/architecture-concepts/providers.md#loading-providers-on-events) · [简体中文](../../../../docs/zh_CN/architecture-concepts/providers.md#loading-providers-on-events) · [繁體中文](../../../../docs/zh_TW/architecture-concepts/providers.md#loading-providers-on-events)
