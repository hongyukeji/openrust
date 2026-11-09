# Laravel 命名空间与开发流程对应

本计划属于唯一待办 AR-N01。目标是公开职责、生产调用与开发流程对应，不能以搬迁完成代替 Container、Routing、Translation 等完整能力验收；完整范围继续沿原矩阵，不建立第二队列。

## 固定来源与当前输入

Laravel framework `d52c3b3129e4c8e213c97cdc68f94b0b7d27f181`、docs `960eedf0911155ad16326ade49c0ea5d18eda0b8`。固定来源索引、Git blob/SHA、当前源码SHA与调用清单位于 `tmp/namespace-alignment-20261106/`；它们是设计证据，不是运行验收。相关矩阵见[总基线](laravel-rebuild-baseline.md)及[完整对应规则](laravel-rebuild.md#逐项对应与差异分类)。

## 公开责任与实体所有权

| 官方责任 | 目标与迁移约束 | 当前状态 |
|---|---|---|
| Foundation Application / Configuration ApplicationBuilder | 运行中的应用及启动builder归foundation；当前Application trait是编译期宿主定义，不冒充官方Application contract。应区分编译配置声明与运行应用，保留唯一Container/Provider机制 | 运行Application、编译期ApplicationDefinition、ApplicationState及统一装配/资源/异常实现已迁到foundation，调用方/生成入口已同步；core保留错误、执行/限流/就绪与协议OTel辅助，限定迁移运行证据已收齐，v12构造器修正及剩余适用门禁已通过；完整官方契约仍开放 |
| Container | 实体、绑定、作用域和解析归container；保持弱Resolver及取消/缓存/重绑边界，不建立第二容器 | 唯一实现及44个Rust文件入口已迁到container，限定迁移编译/运行证据已收齐；旧core::container入口移除，完整Container契约仍开放 |
| Contracts | 官方interface应由真实可替换trait及同一容器实例承载；不能整域pub use旧实现后称已对应。Routing/Contracts的dispatcher不能误放到Contracts/Routing | 分域trait/服务绑定及缺失方法待实现；不创建空门面 |
| Support ServiceProvider | 官方Provider基类职责归support，具体装配及运行注册机制归foundation；Rust trait/BoxFuture仅是对象模型与async必要适配 | 唯一ServiceProvider trait、DefaultProviders与BoxFuture已物理迁至support并同步源码/生成器入口；装配runtime已归foundation；Foundation/Contracts依赖与完整语义未闭合，限定迁移已编译运行 |
| Console | 命令回调、输出及注册仓储归console；应用运行与Provider注册仍复用唯一生命周期，不新增空Console Application/Kernel | 唯一Command与注册仓储及18个Rust文件调用方已迁移，旧core入口移除；签名、交互IO、isolatable、解析/退出码等完整官方语义继续保留缺口；限定迁移已编译运行，完整命令语义仍开放 |
| Database | database应成为唯一canonical实现，而非db别名；迁移Query/Eloquent及全部宏、driver/消费者、生成路径 | 64物理文件已迁至database、255份Rust候选及生成/当前指南同步，旧db模块与alias移除；特性/连接名/SQL/已有Db逃生口保留，限定迁移已编译运行且日常真实MySQL证据已收齐；完整Eloquent/Schema/Manager/五driver与惰性连接等仍开放 |
| Routing / Http | 路由登记、URL生成、Redirector与dispatcher归routing；Request/Response/RedirectResponse等归http。核当前混合Provider的router与server职责后再拆 | 路由实体、URL生成、Redirector、domain与登记仓储已归routing，Request/Response与server Provider归http，应用HTTP装配归foundation；组合静审已修一处测试导入，v12生命周期整理已通过影响复验，Resource/controller/middleware完整语义仍开放 |
| Translation | translation持Translator与加载实现，Contracts/Translation持真实Loader/Translator契约；ICU/快照/数据库来源通过同一加载与翻译机制保留 | 唯一Translator与TranslationServiceProvider及来源WIP已迁translation，数据库扩展同步同一公开服务；限定迁移已编译运行；已对齐应用默认与回退语言的配置入口；默认文件 Loader 与无资源语言已限定验收；完整 Loader/choice 接口、运行期语言变更与 deferred 仍有缺口，数据库来源后继另行交验 |
| Filesystem | filesystem持manager、adapter/provider，Contracts/Filesystem区分Factory/Filesystem/Cloud；底层StorageBackend不能冒充完整业务contract | 唯一FilesystemManager/FilesystemAdapter/FilesystemServiceProvider及调用方已迁filesystem，安全路径/上传取消/driver扩展保留；限定迁移已编译运行；完整Contracts、默认、throw/report与按需生命周期仍开放 |
| Encryption | encryption持Encrypter/provider与对应契约、错误；签名链接职责不随security文件名一起迁移 | 唯一Encrypter及认证/应用调用方已迁encryption，现AES-GCM-SIV/or1增强与安全语义保留；限定迁移已编译运行；官方cipher/key/payload默认和完整字符串/值及替换契约未完成 |
| Facade / prelude | Facade需要容器寻址与替换/缓存/测试机制；prelude是Rust导入便利。真正同实体重导出可保留，纯旧命名兼容面在全部调用方迁移后删除 | 不把现prelude当Facade，也不为表面无reexport删除有效扩展 |

## 公开名称验收规则

依 [命名规范](../conventions.md#命名) 检查固定 Laravel 核心、Laravel Modules 与各已授权官方包中已实现的公开表面。每域追到根装配、唯一示例、生成模板及真实生成结果。库的声明只能证明存在，须沿实际生产登记及消费找到应用可观察的入口。

每项记录固定 SHA 与源码或测试、上游全名或默认值、OpenRust 现名及实际调用点、目标、差异分类和验证状态。差异分为一致、品牌映射、必要适配、待整改、独有扩展、上游能力未实现。必要适配须记约束与等价证据。独有扩展保留行为验证，上游能力未实现回归原功能簇跟踪。

提交评审独立检查 Rust 路径和运行标识。前者编译实际外部调用点，后者用上游独立推导的完整字面值验证真实登记或消费。被测常量、实现输出和随同修改的文档不能作为测试预期的来源。词语扫描或批量替换只提供候选与迁移线索。验证状态区分待取证、静态已对照、待运行、运行通过或失败，运行结论绑定实际测试及受验输入。

### 分域审查对照

按唯一待办和依赖分批补齐下表。核心按职责分域，Laravel Modules 独立覆盖命令、配置、模块清单及生命周期，已实现官方包按包检查。根装配、唯一示例和生成模板是每域的消费检查点。

| 域与固定上游 | 上游全名或值 | 当前 OpenRust 与入口 | 目标与差异分类 | 验证状态 |
|---|---|---|---|---|
| 核心 Translation，framework `d52c3b3` 的 `Translation/FileLoader.php` | `Illuminate\Translation\FileLoader` | `openrust::translation::FileLoader`，默认翻译 Provider 与根、生成应用来源配置 | Rust 模块映射及固定 Application 的语言目录选择 | [目录选择运行通过](../memory/testing.md#应用语言目录选择限定验收)，公开名称提交 `d61369b5` |
| 核心 Cookie，framework `d52c3b3` 的 `Cookie/Middleware/EncryptCookies.php` | `Illuminate\Cookie\Middleware\EncryptCookies` | `openrust::cookie::middleware::EncryptCookies` 由 Cookie Provider 装配，根 Web 组与 `cookies` 路由别名消费同一应用快照 | Rust 模块及应用作用域适配，PHP 子类序列化与进程静态重置仍待对应 | [公开入口与例外运行通过](../memory/testing.md#cookie-公开中间件与例外限定验收) |
| 核心 Encryption，framework `d52c3b3` 的 `Contracts/Encryption/Encrypter.php` | `Illuminate\Contracts\Encryption\Encrypter` | `openrust::encryption::ApplicationEncrypter`，Cookie、Auth 与生成应用使用的应用加密 trait | 合约名称与路径待整改，完整值语义归加密后继 | 静态已对照，运行待验 |
| 核心 Events，framework `d52c3b3` 的 `Events/EventServiceProvider.php` 与 `Foundation/Application.php` | `events` singleton，具体类 `Illuminate\Events\Dispatcher` 与契约 `Illuminate\Contracts\Events\Dispatcher` 指向同一实例 | `events` 与 `openrust::events::Dispatcher` 在默认应用解析到同一实例 | 具名键与类型已对应，完整契约待对齐 | [默认派发器运行通过](../memory/testing.md#默认事件派发器限定验收) |
| 核心 Events，framework `d52c3b3` 的 `Events/EventServiceProvider.php` | `Illuminate\Events\EventServiceProvider` | `openrust::events::EventServiceProvider` 在核心清单惰性登记，应用 `EventsServiceProvider` 向同一实例贡献监听 | 核心路径已对应，应用 Provider 官方路径及发现仍待对齐 | [职责和生命周期运行通过](../memory/testing.md#默认事件派发器限定验收) |
| 核心 Hashing，framework `d52c3b3` 的 `Hashing/HashServiceProvider.php` | `hash` 管理器、`hash.driver` 默认 driver | `hash` 与 `HashManager` 共用实例，`hash.driver` 与 `dyn Hasher` 共用首次解析的默认 driver | 具名键一致，Provider 延迟身份继续核对 | [具名键运行通过](../memory/testing.md#哈希与日志具名入口限定验收) |
| 核心 Log，framework `d52c3b3` 的 `Log/LogServiceProvider.php` | `log` 管理器 singleton | `log` 与 `LogManager` 共享实例、默认频道及容器装饰 | 具名键一致，完整频道契约继续核对 | [具名键运行通过](../memory/testing.md#哈希与日志具名入口限定验收) |
| 核心 Console，framework `d52c3b3` 的 `Console/Scheduling/Schedule.php` | `Illuminate\Console\Scheduling\Schedule` | `schedule::ScheduleDefinition` 描述任务，`schedule::Scheduler` 执行 | 分别核定义、登记及运行后确定路径和类型，待取证 | 上游与本地入口已定位 |
| 核心 Console，同 SHA 的 `Console/Scheduling/Schedule{Run,Work,List}Command.php` | `schedule:run` 执行到期调度，`schedule:work` 持续调度，`schedule:list` 列清单 | `schedule run --once` 扫描一次，`schedule run` 持续运行，`schedule list` 列清单 | 单次与持续执行分开核对，再对应三个命令名及参数，待整改 | 静态已对照，真实命令验收沿原 Console 待办 |
| 核心 Translation，framework `d52c3b3` 的 `Translation/TranslationServiceProvider.php` | `translation.loader` 与 `translator` | 默认 `TranslationServiceProvider` 将同名键关联到 Loader 与 Translator 类型绑定 | 保留原名，一致 | 静态匹配，既有[默认 FileLoader 验收](../memory/testing.md#默认文件-loader-与无资源语言限定验收) |
| 核心 Auth / Foundation，framework `d52c3b3` 的 `Auth/Events/` 与 `Foundation/Events/Terminating.php` | `Illuminate\Auth\Events\Login` 等已实现事件，`Illuminate\Foundation\Events\Terminating` | Session Guard 及 HTTP、控制台终止链保留对应运行全名 | 保留 `Illuminate` 及其余原名，一致 | 静态匹配，既有[内核终止事件验收](../memory/testing.md#内核终止事件限定验收) |
| Laravel Modules，`59714840708a2aece19e810764473f53227c35fe` 的 `src/Module.php`、`src/Constants/ModuleEvent.php` | `modules.<小写名>.<阶段>`，`enabling`、`enabled`、`disabling`、`disabled` | `vendor/openrust/modules/src/module/repository.rs` 经激活链派发相同模式 | 保留原名，一致。上游包的 `Nwidart` 是第三方标识 | 静态匹配，既有[模块启停验收](../memory/testing.md#模块持久启停与同容器管理限定验收) |
| Laravel Modules，同 SHA 的 `src/Commands/Actions/{List,Enable,Disable}Command.php` | `module:list`、`module:enable`、`module:disable` | 模块包 `commands.rs` 注册相同命令名 | 保留原名，一致 | 静态匹配，既有[模块启停验收](../memory/testing.md#模块持久启停与同容器管理限定验收) |
| Laravel Modules，同 SHA 的 `config/config.php` | `modules.activator=file` | `ModulesConfig::default()` 为 `file`，根和生成代码显式默认 `registered` | 应用装配差异，保留[现行应用约定](../core/cli.md)及库默认 | 静态已对照，模块运行语义按原矩阵验收 |
| 官方 Fortify，`c456642584c102f6a6aafee9932299542da637b5` 的 `src/Events/TwoFactorAuthenticationEnabled.php` | `Laravel\Fortify\Events\TwoFactorAuthenticationEnabled` | 同一事件类型由 `openrust::fortify::events::TwoFactorAuthenticationEnabled` 导出，运行名 `OpenRust\Fortify\Events\TwoFactorAuthenticationEnabled`，示例及 Web/JWT 生成应用消费 | 当前路径与运行名，Rust 模块映射及品牌映射 | [运行通过](../memory/testing.md#双因素事件命名限定验收)，八项逐一覆盖 |
| 官方 Passport，`f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的 `src/Http/Controllers/AccessTokenController.php` | `Laravel\Passport\Http\Controllers\AccessTokenController::issueToken` | 扩展包根导出 `openrust_passport::AccessTokenController::issue_token`，控制器已实现 | 目标 `openrust_passport::http::controllers::AccessTokenController::issue_token`，公开层级待整改，方法拼写按 Rust 映射 | 静态已对照，待核真实消费者 |
| 官方 Passport，同 SHA 的 `routes/web.php`、`src/PassportServiceProvider.php` | `POST /oauth/token`、`passport.token`、`passport.path` 缺省 `oauth`，授权同意路由名 `passport.authorizations.authorize`、`passport.authorizations.approve`、`passport.authorizations.deny` | Provider 登记服务，根、示例及生成应用未装配上述路由，`PassportConfig` 尚无 `path` | 保留原路径、路由名和配置，路由、同意 Session 与配置接线归 L090 未实现项 | 静态已确认接线缺口，构造测试 Request 不计路由验收 |
| 官方 Passport，同 SHA 的 OAuth 控制器、迁移与依赖 `AuthCodeGrant.php` | `grant_type`、`client_id`、`code_verifier`，`oauth_clients`、`oauth_auth_codes`、`oauth_access_tokens`、`oauth_refresh_tokens` | 扩展现有 OAuth 参数和显式迁移保留原名 | 标准协议和持久化名称一致，保留原名 | 静态匹配，协议运行验收沿 L090 |

## 实施与 WIP 保护

1. root持公开contract、manifest、宏及集成；只读分域审查不自行冻结公共接口。当前Container先完成唯一实现的实际归属，其余责任按上表继续收敛，AR-N01保持开放。
2. Foundation运行对象、宿主编译声明、Container契约不能仅因同名合并。缺少的path/environment/locale/deferred/Container alias/tag/call/解析回调保留为同一待办下的语义缺口，不划不适用；已有extend等真实能力应复用。
3. Container当前仍携带配置/资源上下文桥；后续随Foundation/Support归属收敛通用上下文与应用资源职责，不把namespace移动说成依赖解耦已经全部完成。
4. 同步root/bootstrap、所有第一方扩展、宏、CLI/模板、唯一example、consumer、Rustdoc与指南。固定上游及旧验收快照不移动、不重写；当前源码引用单独迁移。
5. SC-N01/J01/Passport/I18N-S01等WIP按原字节和SHA保全。Container变换前文件副本与逐文件回执已保存；只改同一类型的路径，分离结构迁移与未验行为，后续冻结候选不能夹带无关WIP。
6. 不保留core::container、db/database双canonical或i18n/storage/security的长期兼容壳；去除前必须同步全部实际调用方与文档，保留真实Facade及必要语言便利面。

## 集中验收与退出

- 每个责任使用固定官方源码和对应测试，列出Rust生产入口、默认/配置/错误及必要语言适配，关联原矩阵；未知或不等价继续为缺口。
- Provider/Container：实际注册、单例/作用域身份、依赖解析、重绑/extend、关闭与取消及公开外部契约接入；验证新入口，不以旧路径测试报告抵扣。
- Routing/Http：根真实TCP的API/Web、参数绑定、路由与中间件、响应/重定向；in-process Tower与真实TCP分别记录。
- Database/Translation/Filesystem/Encryption：唯一模块实际CRUD/来源/文件/认证传输与错误负例；加密需固定官方默认与轮换、失败语义，保留授权增强；日常仅MySQL8.4，其余后端按发布前政策。
- 真实CLI/生成项目check+test、受影响consumer、默认cargo run及关闭/端口复用，全部只解析冻结副本；完整main fmt/严格Clippy/workspace test/doc及适用门禁由独立测试对话集中运行。
- 源码、配套、官方映射与独审收齐后冻结输入交验；通过后限定提交推送与清理可再生产物。最终受验输入为纯迁移v14（运行代码同v12），后继Source/SQLite/Passport等WIP不在输入内。限定迁移已取得默认及routine测试、拒警Rustdoc、consumer与真实生成等证据；CLI快照来自实际生成结果。默认feature构造器生命周期整理、对应复验及最小feature/MSRV/依赖门禁均通过，源码提交`b3f6117be1fc`；详见[限定验收](../memory/testing.md#命名空间与开发流程限定验收)。本子交付不关闭上表完整官方语义。
