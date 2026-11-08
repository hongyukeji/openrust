# 变更日志

OpenRust 的重要变更记录于此，格式遵循 [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)。OpenRust 尚未发布正式版本。

## [Unreleased]

- Eloquent 类转换可保留共享对象身份，支持对象缓存、可选禁用和保存前回写。

- Eloquent 新增显式自定义类转换，支持仅入站、读写补丁、数组输出及原始值比较。

- Eloquent 模型数组和 JSON 递归输出已加载的可见关系，支持关系键名、循环截断和中间表。

- Eloquent 属性数组和 JSON 支持追加计算访问器，遵守模型可见性并保留原始属性。示例记录预览展示 `is_published`。

- Eloquent 日期 cast 支持应用日期工厂、模型序列化覆写和对象缓存回写。自定义 `AttributeGetter`、`AttributeSetter` 及调用级默认访问器使用 `AttributeValue` 和 `AttributeMap`；JSON 业务在实际消费的字段上显式转换。

- 应用控制台新增 `key:generate`，按生效的 `app.cipher` 生成并设置应用密钥，支持只显示密钥及生产环境确认。

- Events 身份/响应切片（限定验收完成）：类事件由 owner 声明稳定 `Event::NAME`，与同名字符串共用 exact/wildcard 分组；响应经同一执行循环保留 null/false/对象身份并提供 until。移除按 payload 类型串送字符串事件和旧 flat 顺序；唯一模块、认证、事务、队列与翻译扩展同步接线。完整 Events 与 Model save 未完成。

- 模块机制迁至独立 `openrust-modules` 包，应用使用通用 Provider 预装配与 `configure_providers` 接线；当前迁移待验，不再由框架核心导出模块类型。

- MySQL 冷预处理取消后明确废弃单物理连接并有界拒绝旧事务，避免错帧/回滚挂起；独立生成应用继承驱动补丁，临时精确固定SQLx0.9.0。

### Added

- `BelongsToMany` 的 Pivot 等值条件应用于联结读取和中间表操作。新增默认值允许调用者显式覆盖，触碰所需的事务来源在写入前校验。

- Eloquent 支持 `BelongsToMany` 联结加载、中间表投影及自定义 `Pivot`。支持中间表新增、更新、删除、同步、切换和时间戳。示例记录预览展示已保存的补充分类及关系标签。

- Eloquent 支持 `BelongsTo` 批量反向加载、自定义所属键及默认模型。设置或解除关联经模型 setter 更新外键，并按操作同步关系缓存。

- Eloquent 支持 `HasOne` 批量单值匹配、缺失时构造默认模型，以及按候选条件和聚合列选取一条关联记录。

- Eloquent 支持通过 `HasMany` 批量预加载及嵌套加载一对多关系，并通过 `fresh_with` 和 `refresh` 回读关系。实现 `HydrateModel` 的模型接收查询选定的连接，并可按关系名称分派加载。

- Eloquent 属性快照支持 `only`、`except`、`fill` 与 `force_fill`，批量赋值复用模型白名单、实际 Schema 与同一 setter 管线。示例记录预览按白名单赋值并输出转换后的摘要与详情。

- 示例记录的创建和更新通过 Eloquent 保存，复用统一属性、原主键、类型化绑定与应用模型事件。预览沿用同一输入填充。

- 模型支持丢弃尚未保存的属性变化，恢复原始基线并清空变化记录。示例预览可显式丢弃本次请求的修改。

- 模型新增单键及整表转换后的原值读取。模型声明的 getter 使用原始基线，缺项默认值经过同一转换；唯一示例单列转换后原值，同时保留原始预览。

- `make:controller` 支持相对命名空间，生成控制器并逐层登记 Rust 模块。声明冲突在写入前拒绝，普通写入失败时回滚本次改动。

- 新增纯 NumericDate 格式化与 `U.u` 解析接口，复用现有日期内核并统一舍入负零。唯一示例提供可选数值日期输入及公开 consumer；JSON 数值域、UTC 和负数秒/微秒字段规则明确区分，不代表完整 Model 日期 cast 或 JWT Parser。

- 增强密码、登录与验证码流程通过注入的 `HashManager` 为每次秘密操作选择一次实际驱动。能力检查、真实校验和已有 dummy 复用该驱动；重置密码与验证码核验分别选择各自驱动。自定义驱动可通过原管理器的扩展工厂返回原实例。

- 统一 `HashManager` 的明文入口接受文本或原始字节，公开 `Hasher` 明文参数改为 `&[u8]`，已编码哈希与元信息仍为文本。新增默认驱动操作选择入口，唯一示例秘密文件哈希命令先解析已知嵌套管理器到同次具体驱动，再校验完整输入能力并生成；bcrypt 的现有长度和 NUL 策略保持。完整二进制模型值域与已编码非 UTF-8 哈希仍待对应。

- 模型属性支持调用级默认 setter：已注册的自定义 setter 优先，默认转换与多字段补丁复用唯一属性管线；默认分支可以借用现有服务，仍执行同名访问器版本和写入冲突检查。唯一示例的 metadata 修改预览同步接入，保留既有 JSON 类型投影。

- 模型原始基线、dirty、changes 与 previous 共用现有属性存储，提供显式内存同步和无存储副作用的单值 cast。唯一示例增加账户记录修改预览，返回 `persisted=false`，保持数据库记录不变；根应用、生成 User 和外部 consumer 同步接线。完整 `save`、事件、关系、批量赋值仍待实现。

- 翻译来源通过同一Loader/Translator选择文件、显式数据库及组合来源，提供有界快照、刷新与提交后提示；数据库来源位于独立可选扩展，根应用、唯一模块和生成项目共用公开Provider装配。文件仍默认，完整Laravel默认Loader/deferred/choice及发布前后端矩阵继续保留缺口。

- 根项目对应 Laravel 应用骨架，框架迁入同仓独立 `openrust-framework` 包；根应用保持默认 server 与 `openrust::` 导入，工具、测试、consumer 与第三方源码路径同步分包。

- Query Builder 增加 Any／All／None 六个多列入口、列与右值闭包、空 FROM 工厂及受控原始投影；逐子查询核对绑定数量，失效来源与未知原生参数不能因条件组包装而被忽略。完整原始 SQL 模式与版本感知仍开放。

- Query Builder 增加默认等值与显式操作符比较入口，绑定值、NULL、表达式及已有标量子查询复用同一条件内核；三方言 JSON 路径与唯一示例时间线、生成应用回归同步。普通 where 的其余重载及完整 Query 能力仍开放。

- 新增模型元数据、精确原始行水合及共享属性读取/异步赋值管线；账户标识与密码复用模型访问器。记住我修改返回拥有式回执，取消按字段所有权恢复，Cookie拒绝未持久化及无效终值；完整Eloquent持久生命周期仍开放。

- 框架默认 JSON、HTML 与应用错误封装共用应用 I18n，缺省语言采用配置默认值；验证摘要与安全 5xx 用户消息统一，稳定协议字段及内部诊断保留。

- 既有 SharedLocks 增加 SQLite 同机租约方言，保留数据库时钟、owner/fence/期限及事务内写保护；唯一示例导入生产链与生成 SQLite 回归同步，最低 SQLite 3.26 有真实验收。

- 新增应用 `env` 命令，从同一启动生命周期报告实际环境，拒绝多余命令参数；根宿主与生成应用使用同一命令注册。

- Query Builder 新增 `insert_get_id`，支持主库/事务内同次执行ID、SQLite有符号rowid、PG RETURNING及固定Processor转换；生成数据库应用附带公开行为回归，独立清单继承固定SeaORM安全访问器补丁。

- 新增统一 HashManager、bcrypt/Argon2i/id 驱动及可编辑 hashing 配置，认证、验证码与生成应用共享公共 Hasher；移除独立密码成本配置。标准驱动保持官方默认和迁移语义，增强登录统一拒绝截断输入别名，历史2x验证复用安全密码库。

- Query Builder 新增查询前回调与 SQL/绑定预览，单一受控状态保留修改、消费、克隆和共享子查询时机；事务与回调捕获分别跟踪借用，查询节点拒绝成环。

- Query Builder 新增 `exists_or` / `doesnt_exist_or` 惰性异步分支，以 `ExistsOrResult<T>` 保留短路true和任意回调值，查询及回调错误透传。

- Query Builder 新增借用查询的 `exists` / `doesnt_exist`，保留完整查询形状、事务和读副本行为；唯一示例导入时间线返回账户内未结束任务的存在性标记。

- Query Builder 与嵌套条件组新增四种列边界 BETWEEN/NOT BETWEEN 查询，支持限定列、可信表达式与同上下文标量子查询；AND/OR 优先级、空条件及读取/更新/删除条件链保持一致。附着后的事务重绑或跨上下文条件组明确拒绝，唯一示例新增账户导入时间线报告。

- 新增配置具名 Guard / UserProvider 与请求内 AuthManager、Session/记住我生命周期及原始类型账户 ID；同步应用模型、生成器和唯一示例，保留真实 Eloquent 与 Fortify 后继范围。异步 Guard 工厂递归或并发等待成环时有界拒绝，取消与失败后可重试。
- Session 保存核持久认证保护并解析实际 Guard 身份元数据；应用账户桥接校验来源，token 失败恢复采用账户内原子比较更新。

### Security

- 网络 `TestDatabase` 清理脱离调用方 runtime，并以正常退出屏障与持久归属回执补偿异常退出；新增显式 `recover`。固定物理管理会话持锁，业务池逐连接核服务身份与标记；活动池、权限不足及归属不明时保留资源，不按数据库前缀删除。网络恢复当前要求 Unix 私有权限及直连唯一可写主库。

- 并发登录挑战验证同一恢复码后，锁内发现因子快照已变化时稳定返回401；管理冲突仍保留409，凭据、审计与恢复码消费保持原子性。

- 显式 Fortify JWT API 的受保护 gRPC 调用复用所选 Guard / UserProvider 的请求作用域与完整凭据校验，拒绝跨域或未绑定 profile 的家族；普通 JWT / PAT 和公开 RPC 保持既有路径。
- Redis Session 独立授权读取使用同次服务端剩余 TTL 与记录截止的较早时刻，不续期，拒绝没有 TTL 的记录；Session 派生 JWT / PAT 在提交前复核实际父记录。

- Guard / UserProvider 的配置故障与请求装配故障按服务器错误返回 500，保持未知 Provider 查询的空值语义与凭据/Cookie 请求错误边界。

- SQLite 自有目录清理显式释放已验证并成功取得的 sidecar 锁，避免复制描述符暂存时仅 close 导致残留锁；身份不明及活动迁移仍保守拒绝清理。

- calamine 升级至 0.38，修复 RUSTSEC-2026-0436 的 CFB/OLE2 解析 panic 与无界内存分配；保持现有数据交换入口及资源限制。

### Changed

- Fortify 开发中的功能配置归回官方 `fortify.features`，区分包7项和安装6项清单；移除 `TwoFactorOptions.enabled`。动作按调用读取确认与密钥长度，`Fortify::two_factor_options()` 改为返回可失败的当前快照；路由及近期密码确认中间件仍按启动配置装配。

- Fortify 开发中的验证码扩展改为单次 `verify_with_context` 返回实际窗口、计数及周期；自定义 `TwoFactorAuthenticationProvider` 须同步实现该方法，原 `verify` 布尔查询门面保留。默认 Provider 使用单例和同一动态配置仓库，确认、Session 与直接挑战沿用该次事实，维护原缓存行为及同种子旧资产下界。短期消费记录不承诺覆盖未来任意扩窗或时钟回退；完整 Fortify 验收仍在进行。

- SQLite本地认证、PAT/RBAC、refresh、验证码和MFA复用统一数据库/Provider，安全读写采用同一连接Immediate事务、UTC精确期限和条件消费；数据库限流与事务审计同步适配。MFA预限流与锁内状态不一致时明确拒绝；外部OIDC关联不冒充本地挑战。SQLite生成认证测试使用自有文件库，完整Guard/OIDC与其他业务后端范围仍开放。

- 数据库 Session 默认转为原始 ID、Base64 属性载荷、`last_activity` 与请求内 `exists`；`connection/table` 复用统一配置及数据库管理器。原哈希身份、绝对截止、防复活和持续撤销能力通过显式 `database-guarded` 与增强迁移保留；旧摘要表不隐式转换。

- SQLite 配置支持 nullable 外键与原生 PRAGMA 标量；`foreign_key_constraints` 改为 `Option<bool>`，三个专用 PRAGMA 字段改为 `Option<serde_json::Value>`。SQLite 安装默认迁移、既有清单 up/down/status、单机文件锁与取消所有者已完成限定验收；旧版删列重建保留 main 普通表结构与数据。最低 3.26 路径关闭传递 RETURNING feature；完整 Migrator/Schema、业务 SQL 与故障原子窗口保持未完成。

- 新增 `db-sqlite` 内置连接与标准 Provider 配置、每物理连接 PRAGMA 和自有 SQLite 测试库；唯一模块支持独立 reporting 探测。SQLite 完整框架迁移及现有业务 SQL 适配尚未交付。MySQL 会话 UTC 初始化改为逐物理连接执行。

- 根应用与新应用默认通过集中模块清单注册启用，不再必需状态 JSON；文件发现与持久启停保留为显式选择。只读状态修改在事件前拒绝，空目录全选同样拒绝。宿主 API 前缀可编辑且与错误范围同源，模块响应策略可组合保留；路由参数超限转安全启动错误。首批正式指南采用固定 Laravel 导航。

- Mail/SMS日志驱动允许production或缺省环境启动，默认应用不再强制要求真实供应商；日志仍不记录正文/凭据。服务初始化错误提供有界、安全的配置键和原因诊断；其service字段改为封装身份，手动包装用Error::service_initialization::<T>，未知driver/SMTP feature与配置错误保持明确失败。

- `Application::commands()` 与 `ProviderRuntime::commands()` 的迭代项改为 `Arc<dyn Command>`，启动与运行期注册共享命令目录；同名覆盖保槽位，旧快照保实例，枚举/匹配/执行不持目录锁。显式类型身份需用 `command.as_ref().type_id()`；完整deferred Provider仍待后继。

- 标准根构建与 `new` 应用预整合 MySQL、Redis 缓存及数据库/Redis 队列；`--minimal` 显式选择无数据库/队列骨架。队列统一 `default`/`connections` 与 `QUEUE_CONNECTION`，旧顶层 driver 配置明确拒绝；连接及外部 creator/validator 按需解析。`queue work [connection] --queue <queue>` 区分连接和队列，迁移与 worker 仍显式运行；标准生成迁移复用唯一 FrameworkMigrator，普通模块任务可经无默认数据库的 Redis 队列执行。

- 通用数据库门面与 prelude 不再依赖 MySQL/PG feature，迁移注册独立于连接配置；具名实例不误启用默认库业务，普通模型生成去除驱动文字门槛。

- 默认文件模块激活器恢复构造期实例快照及写失败后的内存语义，实时读取保留为显式 file-live 扩展；构造返回 Result，坏文件可显式 reset_file 恢复。数字状态键与空表按 PHP 列表规则处理，协作锁/原子替换和未知值保护继续保留。
- 模块运行发现读取可配置路径中的 module.json，经命名编译工厂在包 register 接入同一 Provider 内核；编译文件初始化支持解析路径一次与显式共享执行会话，唯一示例/生成应用同步接线。PHP 弱类型优先级、仓储排序与显示分别对应；class aliases、完整 deferred 加载及非有限 JSON 表示缺口明确保留。
- `RegisterContext` 增加同内核嵌套 Provider 注册及配置仓储句柄；启动与已启动 App 共用去重、force、失败和取消状态，子 register 真实 await、boot 遵循原生命周期。唯一模块与生成 consumer 通过父 Provider 注册独立子项；空 Events 默认绑定采用独立身份，避免吞掉后续真实监听。

- 已编译模块增加同一容器中的 `ModuleRepository`、文件持久启停及 `module:enable/disable/list`；可编辑 `config/modules.rs` 独立配置状态文件，部署根统一为 `application_root`。启停经同一 Events 发送具名生命周期事件，当前加载资源保持，下次启动使用新状态；文件写入采用稳定锁与原子替换。唯一示例和生成模板同步接入；运行发现、交互选择与完整模块管理继续开放。

- 根应用信息响应移到 `app/http/controllers/`，由 `routes/api.rs` 经应用 Provider 登记，框架移除专属 `framework_routes` 入口；根与生成应用分开 Web/API 路由文件并保留既有路径及响应。bootstrap 只保留启动装配，宿主测试主体归 `tests/server/`，必要 Rust 私有测试挂载保持。
- 根 `app/` 支持普通应用 Controller/Model/Provider 开发，与可选模块共用同一内核；`bootstrap/providers.rs` 登记应用 Provider，默认框架 Provider 在统一 Builder 中装配，生成器同步应用结构与 Provider 自动登记。显式包清单不等同自动发现；deferred/eager 缓存、默认欢迎页及完整模块管理仍有后继范围。
- 默认应用配置转为已编译的 `config/*.rs` / `ConfigManifest`，dotenv 快照就绪后向同一仓储求值；模块沿相同机制。TOML 保留为显式可选 source，typed/factory 通过同仓储提供且不可序列化配置明确拒绝缓存。显式 `ConfigSources::merge_ordered_entries` 覆盖沿同次环境上下文、保留未覆盖的 profile 数据与元数据，并拒绝修改已编译缓存；旧 Figment 有损输入不伪造代码配置环境或恢复历史键序。console 专属环境文件选择和缓存屏障同步调用方；应用已有默认字段与可选分支在根/生成配置中显式展开；`ConfigContext::get/require`按清单顺序读取已完成区段及同次最终覆盖，`application_code_sources_with_overrides`在单次求值前接纳根键覆盖。端口与应用身份的缺省依赖保持单一来源，显式领域覆盖优先；晚merge/运行期set不热重算。模块沿原递归候选/CAS发布，生成Job同步接入queue配置并保留已有文件。前向、自依赖、typed冲突及越界上下文明确失败；本次不代表完整Laravel配置、模块管理或driver全量对应。
- 异常处理增加应用作用域 report/render、HTML/JSON 协商和最终响应回调；浏览器验证失败经同一 Session 保存错误包与过滤敏感字段后的旧输入，API envelope 通过公开策略显式配置，应用宿主决定全局路径策略；示例模块只登记自己的异常报告、渲染及响应扩展，不覆盖宿主谓词。`dont_flash` 支持点路径/数组；框架 Json/Validated 提取链在DTO失败前绑定规范输入，原生 Axum 逃生提取器不自动获得该保证。
- 默认 web/api 路由组分工：Cookie、Session 与请求伪造保护按路由组接入，匿名 Web 非安全请求也受保护；同源快速路径、测试豁免与 multipart 正文复用保留明确边界。未匹配路由或方法不进入该组的 Session 生命周期；匹配业务返回错误仍按原生命周期保存。
- 开发者运行产物使用中性保留名：`.staging`、`.data-exchange-retention` 与缓存私有命名等；旧名称仅保留拒绝保护，不自动改名或删除现有文件/对象。Local staging 仅接管有有效所有权标记的目录，无法确认的既有目录明确拒绝；旧清理payload不得套用新前缀误删。现有部署与bucket生命周期规则需按实际归属显式迁移。
- `Db::table` 增加结构化查询与事务接线、部分读写和自动/具名/simple分页，唯一文章列表通过公开查询入口默认分页15；默认表前缀为空、显式前缀与映射保留。CursorPaginator、完整Query/ORM/Factory能力和额外数据库后端仍开放，不以SeaORM或部分API宣称全部对应。


- TrustProxies 请求上下文统一读取可信代理范围和头集合；客户端链、协议、主机、端口与前缀供 Request、路由、重定向和签名消费者共享，新增公开客户端链及 base URL 读取。可信协议头先于物理 TLS 回退；`trusted_proxy_ranges`区分缺省/null和显式空数组，后者关闭平台代理自动信任。普通非平台请求未配置信任时忽略伪造头；`RequestInfo::port` 改为有符号 `i64` 以保留 PHP 请求端口语义，实际 listener 端口仍为 `u16`；唯一模块、生成模板与外部 consumer 同步接线。
- TrustedHosts 新增可选 `server.trusted_hosts` 和公开 `BootContext::trust_hosts` / `trust_hosts_with`；按精确环境、请求局部 PCRE 模式校验代理解析后的有效 Host；物理 authority 先做语法检查，并同步唯一模块、生成模板及外部 consumer。本批限定组合门禁及静态模式、代理 Host、HEAD 与精确环境的真实 TCP 验证已通过。
- 应用环境保留精确名称、自定义值、大小写与空字符串；未指定配置时默认 `production`。公开环境匹配与 `is_local` / `is_production` / `running_unit_tests` 按固定 Laravel 语义查询，既有生产安全保护使用独立派生策略。Loader 改用 `environment_override()` 区分显式覆盖与最终文件/缓存环境；环境值不再 `Copy`，同名值的相等性、哈希与序列化一致。唯一模块、公开消费者与生成测试夹具同步；骨架显式 `development` 保留应用选择；CLI 专属 dotenv 接线已纳入本批限定组合验收；完整配置与控制台家族仍开放。
- 上传文件 MIME 使用应用作用域 `MimeTypes`、可扩展 `MimeTypeGuesser` 与固定 Symfony v8.0.0 全量有序映射；默认 Unix `file` 内容检测，`UploadedFile` 的 MIME/扩展名/随机名改为按需异步并仅缓存随机主体。模块附件按真实内容校验，客户端声明不参与判定；Fileinfo 和 Windows 原生检测后端保留差异。
- Request 通过官方 Axum multipart 提取器接入有界文件 bag；`all/only/except` 改为返回可保留上传对象身份的 `DataValue`，纯 JSON 由 `input_all` 或显式投影取得。新增 `file/all_files/has_file` 与 `UploadedFile` 与原始正文的受管暂存、独立异步游标及 Storage 存储入口；`when_has/when_filled` 回调改接收可保留对象身份的 `DataValue`，前者在混合树判存在、后者在纯输入判非空；唯一模块、生成应用与公开 consumer 同步展示。客户端元数据不可信，有限服务端格式识别、S3 真实 ACL 与 `throw=false` 仍保留缺口。

- 显式可选 TOML source 支持递归发现与按真实文件名建立点路径命名空间，隐藏文件/目录符号链接排除，同名命名空间按实际遍历后项覆盖，再按完整名称自然序加载，不承诺跨平台胜者。根领域文件迁移为相对内容格式，这是一次破坏性配置迁移；模块配置仍沿既有独立 overlay；CLI 生成文件和唯一模块同步调整。同一 `ConfigSources` 仓储保留来源顺序和环境覆盖，晚到 Provider 默认值按浅层合并，白名单数组按已验证的 PHP 规则合并；仅有子文件时保留文件前 base 剩余父区段。完整 Config 和动态 Settings 继续开放；typed helpers 与配置缓存的限定子范围已另行交付。
- 配置仓储新增 `string`、`integer`、`float`、`boolean`、`array`、`collection` 六类严格读取，各提供直接、值默认与惰性默认入口；默认仅用于缺失，显式 null 与类型错误不回退。整数/浮点不宽松转换，错误不回显原值；配置集合复用现有 `Collection`，不把请求输入预算错误施加于已加载配置，其他集合受限操作不变。唯一模块与公开 consumer 展示固定业务键调用。
- 增加 Laravel 式 `config:cache` / `config:clear`，缓存同一应用 fresh Provider 启动后的最终配置。缓存命中跳过 dotenv、普通配置文件和模块配置重载；维护命令先正常引导再派发，坏缓存不静默回退。默认私有数据文件 `bootstrap/cache/config.json` 是 Rust 表示适配；原子写入、失败清理与资源关闭沿同一生命周期，命令只影响后续启动，不热改已解析客户端。
- Session 属性树的公共读取统一返回 `DataValue`，原始写入保留对象身份；DTO 使用显式 `*_typed` 写入，JSON/DTO 读取使用显式投影并处理不可投影对象。默认 native 编码在保存对象时拒绝且不写后端，json 编码仅接受对象声明的投影；唯一业务模块、生成模板和公开 consumer 同步迁移。本次破坏性 API 调整不承诺 PHP serialize 字节兼容，也不代表完整 Session 能力已验收。

- 测试应用增加受控 `TestAppBuilder::run_command`，使用同名 console 启动上下文执行命令并在返回前关闭应用；普通 `build` 继续进行正常启动校验。唯一模块的迁移用例经此入口先运行初次迁移，再按原断言检查回滚、重迁移与禁用后的旧表。
- Storage 增加公开可见性、受控对象发布与 CAS 仲裁基础，本地文件权限和租户附件示例经定向验证；可选 S3 对象 ACL/CopyObject 使用明确连接配置。真实 S3 ACL 正负例、迟到 CopyObject 响应以及 Laravel `throw=false` 返回语义仍未完成验收或对齐，不计为完整 Storage 1:1 交付。

- 监听环境键统一为中性的 `SERVER_PORT → server.port`，移除 `APP_PORT`、独立 `PORT` 和命名空间端口别名；根和生成应用使用 `APP_URL="http://localhost:${SERVER_PORT}"`。缓存按 Laravel 使用 `CACHE_STORE → cache.default`，默认选择已登记的 redis store，移除旧 `REDIS_DEFAULT_STORE`，不引入 `CACHE_DEFAULT_STORE` 别名；Redis 连接与队列共用 `REDIS_DB`，保留其他合法具名 store 和自定义 driver。

- 配置加载增加不可变 `ConfigSources` 与 `ConfigLoader::sources()`，在进入 Figment 投影前保留已有平层 TOML 的声明键序；默认引导一次捕获来源，类型配置和 Provider 复用该输入。仓储、Provider 与 TestApp 沿既有入口接收有序来源，显式 Figment 输入仍是无法恢复历史声明序的有损入口。递归发现与文件名命名空间映射见上项；完整 Config 对应继续开放。
- 命名路由增加永久与临时签名 URL，支持绝对/相对签名、原始查询验签、过期检查及当前/旧应用密钥轮换；`RequestInfo` 与路由 `signed` 中间件提供验证和忽略查询字段入口，无效签名在业务处理前拒绝。唯一模块和生成应用通过公开接口签发并保护草稿链接。当前参数限字符串映射；Vapor 代理原始查询与前缀入口、PHP 对象/数组/Enum 参数表示仍待对应。

- 配置仓储增加运行期 `set` 与有序原子 `set_many`，保留点路径、覆盖位置及稀疏/混合数组；App 与 Provider 共享仓储，`Application::section` 读取当前版本，已解析配置及服务保留启动快照。唯一模块展示固定业务键跨请求更新，生成应用与公开 consumer 同步接入；写入不持久化、不跨节点传播、不自动重建资源，来源进入 Figment 前已丢失的初始键序仍是后继缺口。

- Session 纯行为扩展测试并入既有 `http_session` 私有模块，保留全部用例与断言；原独立 `http_session_extensions` Cargo 测试入口移除，真实数据库、Redis 和环境子进程入口保持独立。入口减少不等于已证明固定提速。

- 路由按同一最终声明集合派生实际分派与动作 URL 视图，修正原串 domain/URI 碰撞、域左侧合并及声明首匹配；允许方法来自幸存集合，GET/HEAD 的405及合成OPTIONS遵固定上游顺序与头格式。唯一模块与生成 helper 展示草稿概览覆盖、静态先于参数路由；文章静态导出入口提前于资源 show，既有授权与业务断言保留。

- 增加真实路由动作身份与动作 URL：显式动作和 Rust handler 同次登记，匿名路由也可通过字符串、二元组或裸 invokable 身份查找；请求绑定 `Redirector::action/action_with` 复用同一生成器和原输入/错误闪存链。唯一模块与生成 helper 展示固定动作回跳、真实命中身份及一次性草稿状态；完整参数族、signed 与完整 Redirector 继续保留后继范围。

- 请求绑定重定向新增 `refresh/refresh_with` 与 `away/away_with`：刷新按当前路径生成目标并丢弃 query，外部跳转直接使用目标原值；均复用现有响应构造和输入/错误闪存链。唯一模块通过 POST→同路径 GET 展示草稿，外链使用可信常量；生成 helper 覆盖显式状态/头、刷新后闪存及固定外链，完整 Redirector 仍保留后继范围。

- 请求绑定重定向增加显式协议选择与 `secure`，并提供 Session `guest`/`intended` 及意图读取/设置入口；意图使用普通 Session 属性，先保存或先消费再构造响应。唯一模块与生成 helper 展示非敏感草稿确认回跳、双 Cookie 隔离与一次消费，保留既有输入及错误闪存流程；HTTPS Location 不代表监听器已启用 TLS，完整 Redirector 仍保留后继缺口。

- 请求绑定的 `Redirector` 新增 `route` 与 `route_with`，复用现有命名路由 URL 生成器，默认返回 302，也可指定状态与响应头；保留输入和命名错误包闪存链。唯一业务示例展示无 Session 的命名跳转和审核页错误闪存，生成应用测试同步接入命名路由；完整 Redirector 与自动校验失败重定向仍属后继范围。

- 视图增加 `Views::render_for_request` 与请求私有错误包；Session 启动后、handler 前恢复，根首页和普通请求使用同一入口。显式数据（含 null）覆盖共享错误，自定义消息包保留模板方法派发；无 Session 使用独立空包。请求共享对象与 Session 保存关联，整包替换后断开旧关联，缺失回退对象不写回 Session。模板支持多键、列表与关联字典值、标量多参数及可空格式参数。

- 首页模板读取运行时 `app.name`；自定义根目标在业务路径未匹配时优先于用户 fallback 和静态首页，默认 `file:/index.html` 保留静态首页与 fallback 组合。内部 forward 按目的 URI 分派，保留外部原始 URL。站点响应出口清空 HEAD 正文并保留状态、头与扩展，不推断 Content-Length；站点外层 middleware 短路响应不在此保证内。

- 将三个纯 Session 测试入口聚为一个，保留原源文件与 26 个用例；修复 gRPC 联合生命周期测试释放随机端口后重绑的竞争窗口。新增可接收两个已绑定 listener 的嵌入式联合运行入口，原 `serve all` 配置绑定方式不变。

- 新增公开 `MessageBag`、`ViewErrorBag` 与消息包扩展契约：保序/稀疏键、字段与通配查询、格式化、命名包及 JSON 选项；递归合并通过 `MessageValue` 保留派生数组。纯消息包无需 Session，自定义命名包保留对象身份并派发契约方法。请求局部视图共享通过 `Views::render_for_request` 提供；自动验证失败重定向仍属后继范围。

- Session与Request增加旧输入读写：`flash_input/get_old_input/get_old_input_or/has_old_input`、`Request::flash/flash_only/flash_except/flush`和`RequestInfo::old/old_or/old_all`。复用已有flash老化和单次写入预算回滚，保留缺失/null/默认、字面键优先dot及空筛选语义。手动闪存不默认剔除密码，应用须显式排除敏感字段；上传文件的持久化编码仍属后继范围。

- D10-B 公开 `Redirector::from_info/from_request`、`RedirectResponse` 与 `RedirectErrors`，普通重定向携带 `Location` 和 HTML 正文；请求绑定方法显式闪存规范化输入和命名错误包。Session 的 `ErrorBagCodecs` 支持 Provider 扩展并跨请求恢复自定义错误包，编码/解码在属性锁外运行，失败和预算超限不发布半成品。内置包无需登记；请求局部视图共享见本节视图入口，异常 `dontFlash` 与自动验证失败重定向仍属后继范围。

- 修复空库执行框架 `migrate` 时 MFA 表预查阻断启动：仅实际匹配的框架迁移命令免去运行态五表预查，其他命令、普通启动和 `serve` 仍严格校验；不改变迁移参数、回滚或失败清理语义。

- 受管理 Handler 支持显式启用的 Precognition：按参数顺序提取与验证，成功预检返回空 204，不调用业务函数。字段筛选可提前结束；认证和授权须放在路由中间件或验证参数之前，原生 Axum 逃生入口不自动获得此保证。

- `Validated` 的 DTO 改为实现框架 `Validate::rules()`，由同一 `Rules` / `Rule` 执行器处理普通请求与选中字段预检；不再直接以 `validator` 的派生 trait 作为框架验证契约。完整规则集及 FormRequest 仍按后继范围推进。

- Session 增加 previous URL/route helper；真实 GET 路由下游返回后记录当前请求，供后续请求读取，已确认预检跳过自动保存及 previous 更新，单纯客户端头不会改变保存行为。

- `Router::route` 改为接收公开 `http::routing` 的受管理方法声明；同路径按方法合并与后注册覆盖，保留各处理器的路由身份和中间件。原生 Axum 路由仍作为显式逃生口，不自动承诺受管理请求 Route 上下文。

- 显式 fallback 按 Laravel 使用 GET/HEAD；普通当前方法优先，fallback 参与备用方法与自动 OPTIONS 判断。域名与路径可使用同名参数，绑定时路径值优先；单个模式内部重复参数仍拒绝。

- 正式宿主与生成应用按数据库来源或显式非数据库 Session driver 装配唯一 Provider；数据库应用无需启用 Auth 才能使用 Session。默认 database 驱动且无数据库时不暗用内存，显式 Cookie driver 可在无数据库宿主运行。

- Session 统一到公开 `openrust::session`，认证与业务共享请求状态和 store；属性同步操作、持久化异步执行。默认数据库、120 分钟 lifetime，Cookie 名从最终 `app.name` 推导，Secure 未指定时依可信传输决定；取消请求不后台保存。自定义 driver 使用公开 `SessionStore`，不再依赖旧 tower Session 接口。


- 认证示例及生成应用的密码重置确认字段改为 Laravel 式 `password`，保留密码首尾空白，与登录使用相同规范化规则。

- API/framework JSON 默认使用语义 HTTP 状态（如创建 201、校验失败 422、资源缺失 404）；`always_ok` 仅在配置中显式选择时启用，统一 envelope 与机器码保持不变。

### Fixed

- Request 数值转换、Session 算术与 InputNumber 复用同一 PHP 显式指数规则，避免大尾数与反向大指数相消时误用 Rust 默认解析结果；Model 保留独立异常边界并委托共享数值内核。

- 修正简单列JSON包含在PostgreSQL中的模板占位符与jsonb转换；保留typed JSON绑定、MySQL分支和账户查询范围，并同步示例筛选与生成回归。完整JSON查询家族仍开放。

- SQLite 查询构造器的联表或显式限量更新、删除使用 rowid 子查询选择写入目标。保留排序、分页和绑定，直接写入遇到未消费的业务参数会在执行前拒绝。

- 加密服务改为首次解析时校验密钥。受保护 Cookie 请求复用 Router 的保护计划，失败时拒绝发送 Cookie。受管 Web 回程在保存前核对本次请求作用域和出站 Session Cookie 名。共享 Tower layer 应在 HTTP 入口任务上下文驱动首次派发。

- `APP_KEY` 和 `APP_PREVIOUS_KEYS` 从环境读取时保留前导零与普通空白，避免 URL 签名误用经通用配置解析改写的文本。旧签名继续按原始历史密钥验证。

- 配置来源错误仅显示可信诊断路径：未声明 schema 时保留已知父键，已声明字段才显示字段路径，动态名称与不可信数组片段不回显；来源值继续脱敏。

- 测试数据库仅在本次 CREATE 已成功后清理目标库；同名创建失败不再删除已有库。PostgreSQL 模板碰撞遵循同样的创建所有权边界；取消、连接中断或删除失败仍可能留下本次资源，不承诺必然回收。

- Cookie Session 认证校验真实受管理 Cookie 保护事实；plain、身份 Cookie 或合法数据 ID 被排除保护时拒绝认证，并在最终保存时防止更换名称/handler 降级保护。Cookie/Session 机制自身的失败响应统一禁止缓存，不泄出被替换响应的 Cookie 或正文。

- 本地存储回收 staging 时允许其他应用实例关闭并删除已列举的 run 目录；权限和其他 I/O 错误继续阻止初始化。

- 修复容器缓存完成通知早于构造帧结束的误报循环，以及生命周期关闭通知持锁时的同步 Waker 重入死锁。

- 配置目录显式错误不再静默退默认；已发现 TOML 精确读取，来源错误安全传播，Debug/启动日志不输出目录原值。

- 在 API 响应中保留类型化 `PageMeta`；明确 CORS 预检响应正文为空，同时普通跨域错误仍使用统一包装。

### Added

- 配置仓储新增 `get`、`get_or`、`has`、`get_many`、`get_many_with_defaults` 与 `all`；保留字面键优先、缺失与 null 的区别，批量读取固定同一快照。成功返回值可能含秘密，调用方须控制输出。

- Container 新增异步 `extend`，按顺序装饰原 factory 或实例并沿用既有发布/作用域机制；可信 Provider 可通过 `SessionStoreHandle::store()` 保留原 store 实现装饰。唯一模块与独立 consumer 提供公开用法，逃生口不替代请求 Session 校验。

- 请求公开真实匹配的 `Route`、路由名称、参数/default 与名称通配匹配，并提供可信应用可替换的 `RouteResolver`。上下文共享于同一请求，覆盖路由中间件与外层下游返回后的读取；不从响应状态、请求头或具名 URL 生成器推断命中。

- 请求新增 Laravel 风格 URL、路径分段/匹配与预取检查；`full_url`规范原始查询串，增删参数读取当前query bag，保留端口和可信传输边界。唯一模块和生成应用演示普通GET、JSON GET及POST来源差异；非法UTF-8与URL资源超限显式报错。

- Session 新增公开 driver `gc(lifetime_seconds)` 与请求级 `session.lottery`，默认 `[2,100]`，启动成功后、业务前对实际绑定驱动执行回收。数据库/内存清理已过绝对截止的记录，Cookie/Redis保留 no-op/TTL 职责；显式期限优先及取消不回滚已发生删除的边界明确记录。

- 新增公开 `CookieSessionStore` 与配置 `driver="cookie"`，复用请求绑定、Cookie 队列和密钥环实现双 Cookie；公开存储保证分类及认证完整性能力。客户端快照仅提供本请求与当前浏览器删除，不能承诺全局撤销；秒精度、异常弱类型及浏览器容量边界明确记录，唯一模块与生成应用提供使用参考。

- Session 新增可信 ID/驱动选择、来源与写入意图管理及请求内 driver 绑定；公开 `SessionRequestContext` 与默认请求 hook。`handler()` 改为返回 `Result`，同步外部实现与示例；显式转移、目标碰撞、撤销和失败 Cookie 清理分别定义，内置 Cookie driver 与客户端快照边界见本次新增能力。

- Session 新增 `remember`、`push`、`increment/decrement` 及显式步长、`remove`，复用同一属性和预算校验；`set_name` 接入真实出站 Cookie，`handler` 提供当前驱动引用。唯一模块和生成应用展示跨请求业务调用；PHP 数值差分与错误处理边界单独记录，完整 Session 扩展生命周期继续后继实现。

- 新增 Laravel 式 Cookie 读取、值工厂、请求隔离队列与响应组合；受保护模式复用现有密钥环，保留原始头的严格认证检查。唯一模块及生成应用展示 Cookie 往返，显式明文模式不提供加密保护。

- `Request` 新增 Laravel 式标量、字符串、集合、枚举、clamp 与条件读取；共用有界输入内核，`Fluent` 继承请求日历上下文。日期和区间使用 PHP 格式与语义，应用时区通过 `app.timezone` / `APP_TIMEZONE` 配置，可注入请求级时钟；唯一模块与生成应用提供公开 API 使用示例。

- 新增统一正文所有者 `Request`，提供 Laravel 式输入来源合并、dot/通配读取、存在性、筛选与输入修改；`Json` / `Query` 共享默认 trim/null 转换，公开 `InputNormalization` 支持路由或请求覆盖，原始正文保持不变。

- 新增只读 `RequestInfo` 提取器：检查路径、方法、严格 Host、头与 bearer，并按固定 Laravel/Symfony 行为执行内容协商；与 `Json<T>` 共存，限制元信息大小且 Debug 隐藏敏感内容。唯一模块示例展示 HTML/JSON 协商与正文读取。

- HTTP 路由支持 `RouteGroup::domain` 与 `DomainParams<T>`：域路由按声明顺序在普通路由前匹配，具名域 URL 由登记模板、单标签参数和 `app.url` scheme/port 生成；畸形 authority 失败关闭，转发 Host 不受信任，业务 404/405 不触发二次派发。

- HTTP 路由支持 `.name(...)`、按原样组合的组名 `name_prefix` 与 `UrlGenerator`；路由参数安全编码，额外参数生成有序 query，URL 来源固定为 `app.url`。

- Provider 可登记可复用的 HTTP middleware alias 与递归 group，路由及嵌套组可按名称选择应用；未知名称、重名与 group 循环显式报错，无应用注册表时 `Router::into_router()` 也会失败关闭。

- HTTP 路由新增 Laravel 式 `Router::fallback`；支持静态 RouteGroup 前缀与 middleware，GET/HEAD 未命中时优先服务 public 静态文件（含 `/api`、`/ws`），原始 URI/body 传递给业务 handler，同 scope 重复 fallback 稳定报错。参数化/通配符组前缀暂报 `http_fallback_scope_unsupported`，避免 Axum catch-all panic。

- R1-E 在唯一 `modules/example` 中加入有所有者约束的 Article CRUD，并以实际业务路径展示租户、附件、缓存、队列、认证、实时通信、数据交换、国际化与观测等当前公开能力；具体真实服务范围和仍未关闭的边界见示例 README 与验收记录。

- `openrust new --grpc` 生成 typed Echo 服务与 Provider，并提供应用二进制 `serve all` 的 HTTP + TLS gRPC 联合运行测试。`--auth` / `--fortify` 使用官方Web默认，显式 `--profile jwt-api` 生成JWT API与真实TOTP挑战、恢复码一次性及撤销测试；旧 `--auth-mfa` 已移除。两种模式与数据库及可选gRPC分别配置和验收。

- R2-M 新增编译期 `ModuleDescriptor`、运行时 `ModuleManifest`/文件激活快照、模块配置与资源路径加载、命名空间视图和翻译，以及按全局名称/批次执行的统一迁移历史。根应用与生成项目从运行根读取模块状态；示例业务完整移至 `modules/example/`。管理写命令与完整发布流程仍属后继。

- R2-D 新增与生产共用 Provider/Router 启动路径的 `TestApp<A>`、可重放的 Provider 移除与注册后绑定覆盖、有界请求和安全响应断言；完整 `reset` 先关闭旧应用再重建。普通及认证项目生成模板包含对应测试，综合示例验证业务服务替换和路由移除。

- R2-C 为HTTP流式响应、命令、队列每次尝试和调度每次触发接入独立容器scope与受管清理；公开 `Scope::run`、`Scope::spawn`、`ScopeFactory` 和自定义队列driver可显式等待的 `JobAttempt`。

- 新增 R2-A 类型化容器基元及安全错误 Debug；生产 Provider/请求生命周期尚未切换。

- 新增共享固定窗口限流，以及使用 PostgreSQL/MySQL 存储、绑定用途的一次性验证码。

- 新增 OpenRust `Json` / `Path` / `Query` 提取器、`Created::with_location`，并为常见 HTTP 拒绝响应提供 JSON envelope 回退。

- 新增不发布的 `apps/server` 集成宿主，并设为 workspace 中 `cargo run` 的默认目标。
- 建立 Cargo workspace、共享 lint 策略、双许可证、CI 基线和框架架构决策。
- 新增应用核心，包括分层配置、类型化服务注册表、插件生命周期、运行时命令、结构化 HTTP 错误和可配置 JSON envelope。
- 新增 HTTP 路由、资源操作、类型化响应、校验与注入提取器、健康路由、中间件、CORS 和压缩。
- 新增 facade 应用运行入口、最小示例，以及可将运行时命令转发给项目二进制的 CLI。
- 新增贡献、安全和社区指南。

### Changed

- 应用关闭同步封闭容器的新解析与受管任务登记；`TestApp` 析构时即使没有运行时也先封闭，异步回收仍需显式 `shutdown` 或存活的 Tokio runtime。已持有的服务 `Arc` 由调用者管理。

- `Application::run_command` 每次执行使用新scope，应用资源在显式关闭时回收；队列任务与调度触发不继承调用者的请求或租户身份。

- `ConfigLoader::figment()` 返回 `Result`，直接调用方须传播配置来源错误。

- 根 Cargo package openrust 同时提供框架 src/lib.rs 与默认 server 薄 src/main.rs；Laravel 应用职责目录回根，default-members=["."]、default-run="server"，根无参数 cargo run 启动唯一应用。

- 业务示例通过公开模块清单和统一 Provider 接入根唯一应用，专属迁移、资源、协议和测试位于 `modules/example/`；根不保留重复应用或 `examples/` 目录。CLI 生成项目也提供模块清单和文件状态入口。

- 根 CLI 运行命令按 Cargo metadata 默认应用转发，根 server 从根职责目录读取配置与静态资源。

- 错误响应中的 `timestamp_ms` Unix 毫秒数改为 `timestamp` RFC 3339 UTC 字符串（毫秒精度）。

- 将默认运行时配置命名空间切换为 `APP_`；应用可通过 `ApplicationDefinition::config_namespace()` 选择一个自定义前缀。
- 将所有 OpenRust JSON 成功和错误响应统一为 `ApiResponse<T>`，并新增 `[api] http_status`（`always_ok` 或 `semantic`）、请求 ID 正文/响应头一致性及限流重试响应头。

- 将 facade prelude 切换为 OpenRust 提取器，并按照 历史决策 008 统一校验、超时和未匹配路由响应。

- 按照 历史决策 006 在已实现 API 中统一采用以下名称：`Plugin::build/finish`、`AppBuilder::plugin/insert/get`、`ApplicationState::get/try_get`、`HttpPlugin`、`Router::nest`，以及资源操作 `list/get/create/update/delete`。
- 将运行时配置切换为 `config/default.toml`、`OPENRUST_ENV` 和 `OPENRUST_<SECTION>__<KEY>`；`DATABASE_URL` 和 `PORT` 映射到对应配置区段，带前缀的变量优先。
- 将配置命令改为 `config show`，并记录未来的 `openrust generate` CLI 规划。
