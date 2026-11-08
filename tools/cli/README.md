# OpenRust 命令行

`cli` package 提供命令 `openrust`，用于创建用户应用、生成代码并转发应用命令。无需全局安装或从 registry 获取框架包。
生成应用和仓库根应用均通过框架内置命令执行 `config:cache` 与 `config:clear`；全局 `openrust` 只转发，缓存位于应用根的 `bootstrap/cache/config.json`，生成的 `.gitignore` 排除该运行产物。
命令帮助中的说明、分组标题及帮助/版本选项均使用简体中文；命令、参数和配置字段保留原名。

## 创建和运行应用

从框架仓库根目录创建应用（默认 MySQL；示例显式选择 PostgreSQL）：

```bash
cargo run -p cli -- --help
cargo run -p cli -- new blog --db postgres --auth
cd blog
cargo run -p cli -- dev
```

`openrust new blog --db sqlite` 选择 SQLite，创建 `database/database.sqlite` 及 `.env`，并在全部源码生成后运行默认迁移。启用认证或加密时，安装器先在新建应用自有 `.env` 中为各用途生成独立的32字节系统随机密钥，Unix权限为0600；`.env.example` 保持空占位，密钥不打印。JWT/签名密钥为现有认证增强，非Laravel内置认证默认。已有应用的密钥不被覆盖。迁移失败会返回非零并保留生成文件；数据库及 sidecar 已在生成 `.gitignore` 排除。SQLite安装使用默认迁移；当前Web SQLite/MySQL、JWT SQLite及JWT+gRPC MySQL生成组合已完成相应验收，其他组合与后端支持仍按实际范围验证。完整 Migrator 和业务 SQL 仍有缺口，不以生成成功表示认证、数据库 Session、队列等生产能力已全面验收。生成迁移只使用 OpenRust re-export 的同一迁移依赖。

真实 `new` 在文件生成后使用当前工具链的 rustfmt 格式化新应用及其迁移包，显式选择这些包并排除本地框架依赖，再执行适用的安装步骤。需已安装 Cargo 与 rustfmt；格式化失败返回非零并保留生成文件，不自动安装组件或修改全局配置。

`openrust new <目标目录>` 创建独立应用 workspace，支持相对和绝对路径；本地 framework/tool 依赖在生成 manifest 中明确解析，源框架 workspace 不被修改。已有目标、非法目录名、冲突 package 名在写入前拒绝。应用 `src/main.rs` 是薄入口，装配在 `bootstrap/app.rs`，通用路由在 `routes/web.rs`，应用控制器、模型和 Provider 可直接在 `app/` 开发，业务模块可选放在 `modules/<业务>/`，资源在 `resources/`，迁移与种子在 `database/`。

根/生成 `config/*.rs` 显式展示已实现策略，按 `server → app → auth/security` 声明依赖。APP_NAME/APP_URL只在app配置求值，缺省URL取同次有效server.port；auth/security缺省身份读取最终app值，显式独立来源仍优先。标准生成项目默认启用queue及queue-redis，realtime仍按需选择；--minimal省略queue装配，生成Job时会保留已有queue.rs并安全登记queue代码配置，发现冲突先拒绝而不覆盖源码。根View的常量只控制默认声明，生成项目默认声明View，均不强制屏蔽显式来源。

队列统一由 `config/queue.rs` 的 `default` 与 `connections` 配置；`QUEUE_CONNECTION` 默认 `database`，可选择 `redis`，不再使用 `QUEUE_DRIVER`。先完成实际服务配置与迁移，再显式运行 `openrust queue work` 或 `openrust queue work redis --queue example`。编译内置 driver 与登记 Provider 不自动启动 worker，也不连接未选队列。`--minimal` 与 `--db`、`--auth`、`--fortify`、`--profile jwt-api` 互斥。

生成应用的 `tests/support/request_input.rs` 通过公开 Provider 挂载 multipart 业务测试路由，验证 `Request::file`、`has_file`、`only` 的对象身份、纯文本输入及异步服务端 MIME/扩展名；该测试路由不进入生产应用。

默认 `/api` 控制器使用 `resolve_key_with`，将启动配置中的应用名称传给当前请求作用域中的 `HomePresenter`。控制器通过同对象投影获得 `dyn HomeResponse`，再用公开 `ApiResponse` 返回带 `data` 的成功响应。类描述符由 `bootstrap/app.rs` 在 Provider 注册前加入应用容器。生成测试的 Cookie、Session 和重定向专属路由按需显式选用 `cookies`、`session` 中间件；Provider 注册服务不代表全部路由自动启动 Session。原宿主测试保留框架默认清单与应用清单合成后的唯一 Session Provider 和真实 Cookie 往返断言。

生成项目默认启用可选 `view` feature、注册 `ViewServiceProvider`，并以 `resources/views/index.html` 渲染 `/`。应用级 HTML 页面模板放在 `resources/views/`，CSS、图片等静态资源放在 `public/`；`root_target` 可切换为静态文件、应用内转发或重定向。需要纯 API 应用时，可在生成后的 `Cargo.toml` 移除 `view` feature 和 Provider 注册，再将 `root_target` 设为其他模式。

生成应用的 `bootstrap/app.rs` 通过公开 Handler 扩展仅对当前 `/api` 前缀选择统一 JSON 错误包装。应用修改 API 前缀时应同步修改该判定；HTML/Web 路由继续按自身内容协商处理异常。`APP_DEBUG` 由严格布尔配置读取，默认关闭服务端详情。

不带 `--db` 时默认生成 MySQL 应用，并预整合 Redis 缓存与两种队列驱动；`--db postgres` 选择 PostgreSQL，`--minimal` 显式生成无数据库/队列的应用，仍可生成模型并注册通用迁移命令。标准数据库应用生成自己的迁移 package 和 seed 入口，迁移以 `FrameworkMigrator` 的同一框架清单加应用迁移，覆盖所启用的 Session、Queue 等基础设施，不运行自动迁移。`--auth` 与 `--fortify` 选择相同的 Web/Fortify 认证脚手架，可同时使用；安装器自动加入所需加密能力。它们使用所选数据库、具名 Session Guard、Cookie/CSRF 与唯一 Fortify 三字段/Actions，生成 Web 登录、密码确认、TOTP 挑战及管理配套；不默认挂载 JWT 登录与刷新。

完全无 Session/Cookie 的 API 应用须显式选择 `openrust new blog --profile jwt-api --db sqlite`，不与 `--fortify` 共选。该 profile 用公开 API Provider 替换 Web Provider；`/api/auth/login` 接收 `account` 和 `password`，必要时返回 `two_factor`、`challenge` 与 `expires_at`，再向 `/two-factor-challenge` 提交代码。生成应用的邮箱验证策略由其公开认证回调实现，不成为框架的全账户邮箱要求。资源使用 access Bearer，refresh/注销仅接收 JSON 中的 refresh token。管理先以当前 Bearer 和密码换取短期 `management_token`，后续通过 `X-Fortify-Management` 调用同一 Fortify Actions；该授权不是资源 token。所有认证、挑战与管理均不借 Cookie、隐藏 Session 或 CSRF-cookie。

两种 profile 都提供用户自愿绑定、待确认密钥、TOTP/恢复码和密码复核。恢复码加密可重新读取，成功消费后替换并保持八份；不保留旧独立 MFA profile。路由与安全边界见[认证指南](../../docs/src/security/authentication.md)。`--http-client`、`--sms-twilio`、`--encryption` 可按需启用出站 HTTP、Twilio 短信和进程内加密。部署前配置数据库、邮件及对应 JWT/加密密钥并执行迁移；SQLite 安装器已有的自动迁移行为保持。当前Web与显式JWT的已交付组合及剩余边界见认证指南；真实Google客户端与UUID/字符串JWT持久链仍未闭合。

从应用目录运行后续命令；从 workspace 根执行时选择 Cargo metadata 中唯一的默认应用成员。CLI 会选中对应应用 package，并以应用目录作为子进程工作目录，因而读取该应用自己的 `config/`：

```bash
cargo run -p cli -- key generate
cargo run -p cli -- env
cargo run -p cli -- generate model post title:string published_at:date_time?
cargo run -p cli -- generate migration comments body:text post:references?
cargo run -p cli -- generate model post --uuid-id --soft-delete title:string
cargo run -p cli -- generate migration posts --uuid-id --soft-delete title:string
cargo run -p cli -- g migration labels slug:string
```

`openrust env` 转发给所选应用，输出其本次启动选定的环境，例如 `The application environment is [production].`。该命令没有本地参数，沿应用统一启动与关闭流程执行；它不重新读取父进程环境或把运行期配置修改当成新的启动环境。

`dev` 沿用已配置的应用运行环境并运行 `serve`；需要 development 时可在进程环境或应用 `.env` 中设置对应命名空间的 `ENV`，`serve` 同样保留当前环境。`dev` 默认使用 `APP_`；若应用的 `ApplicationDefinition::config_namespace()` 返回 `SHOP_`，请运行 `openrust dev --config-namespace SHOP_`。CLI 与应用处于不同进程，无法读取该 Rust 类型常量，因此自定义命名空间须显式传入。`dev` 的 Redis 启动预检目前只对显式 TOML 来源与可由环境确定的配置使用 `ConfigLoader`；CLI 进程不能静态求值应用的 Rust 配置函数，因此代码配置的最终依赖由实际应用启动校验。配置缓存命中时跳过实时 TOML；未命中时按应用根读取 `.env` 与显式文件来源。外部环境优先，CLI 不改写父进程环境；缺省 `config/` 不存在时仍读取环境来源。若有效配置指向 loopback Redis，会先探测服务：已有服务可用时直接复用，不可用时通过仓库 `compose.ci.yaml` 启动 Redis 并保留容器供后续本地开发复用。远端 Redis 由应用直接连接。其他命令转发给所选应用并保留退出码；当前不提供文件监听或就绪等待；仅 `new --db sqlite` 安装阶段自动执行默认迁移。

`config/api.rs` 通过 `API_PREFIX` 配置 API 路由前缀（默认 `api`，实际URL前缀为 `/api`）；应用 Provider 从同一 `ApiConfig` 规范化前缀并选择异常响应格式，前缀调整同时作用于路由与异常呈现。

## 接入自己的业务模块

模块能力由独立 `openrust-modules` 包提供，类型通过 `openrust_modules::` 导入。应用在 `ApplicationDefinition::configure_providers` 中引入 `ModuleProviderBuilderExt` 并调用 `builder.module_manifest(...)`，复用框架唯一 Provider/Container 管线；框架核心不再重导出模块类型。

新应用默认 `MODULES_ACTIVATOR=registered`，`modules/mod.rs` 返回锚定应用根的空清单，不扫描目录、不读写状态文件；显式选择其他激活器时使用 `ModuleManifest::discover_from_config(root, config)`；`ApplicationDefinition::configure_providers` 只装配一次清单，与应用共用唯一 Container 和 Provider 生命周期。根 app 仍支持常规应用开发，模块是可并存的业务组织。默认不复制框架示例。

需要文件发现模式时设置 `MODULES_ACTIVATOR=file`（或 `file-live`），在 `modules/orders/mod.rs` 编译自己的公开 Provider，再在 `modules/mod.rs` 的文件发现分支登记 metadata 名称到 Rust 工厂的绑定：

```rust
pub mod orders;
use openrust_modules::config::ModulesConfig;
use openrust_modules::ModuleManifest;
pub fn manifest(root: &std::path::Path, config: &ModulesConfig) -> ModuleManifest {
    ModuleManifest::discover_from_config(root, config)
        .compiled_provider("OrdersServiceProvider", |_| Ok(orders::OrdersServiceProvider))
}
```

`OrdersServiceProvider` 实现普通 `ServiceProvider`，路由和资源沿公开接口贡献，不手工转调其他 Provider 的生命周期。在 `modules/orders/module.json` 写运行声明：

```json
{"name":"orders","priority":0,"providers":["OrdersServiceProvider"],"files":[]}
```

生成器不创建状态 JSON，也不迁移已有文件。显式文件发现模式下，应用根 `modules_statuses.json` 中的 `{"orders":true}` 选择加载。已发现但没有编译绑定的 disabled 模块仍可被持久启用；缺失实现到实际加载时明确失败。Rust 不执行磁盘上的任意 `.rs` 或运行时编译 crate。

`config/modules.rs` 的 `paths.modules` 默认 `modules`（本项目获准目录约定），可用 `MODULES_PATH` 部署覆盖；`scan.enabled` 默认 false、`scan.paths` 默认 `["vendor/*/*"]`。额外位置可由 `add_location` 添加；依次扫描每个位置的直接子模块 `module.json`，保重复来源和声明顺序。状态路径另由 `activators.file.statuses-file` 控制，默认应用根 `modules_statuses.json`，部署变量 `MODULES_STATUSES_FILE`。相对路径基于 `Configuration.application_root`，不绑定编译机源码位置。

Metadata 在包 Provider 的 register 中读取前序已发布配置；禁用模块的坏 JSON 也会失败。启用项按固定 PHP 普通比较排序 priority（缺失/null 为 0），providers 递归展开并按 PHP 真值过滤，再进入同一注册器。同一 Provider 类型普通登记只执行一次；声明重复可多次构造。JSON `module_directory` 只覆盖文件包含目录；Provider 常规资源通过同一 ModuleRepository 的模块路径读取，仍取清单所在目录。工厂的 `ModuleLoadContext::directory()` 使用仓储同名最后来源，不把文件目录混入资源路径。

`files` 必须通过 `compiled_file`（应用根路径）或 `compiled_module_file`（metadata 模块目录路径）绑定实现 `ModuleFile` 的初始化器。所有 eager Provider register 完成后、正常 boot 前执行这些文件；同一解析路径一次。回调共用当前 Container/ConfigRepository，前序副作用没有事务回滚。PHP 类 aliases 尚无等价 Rust 编译接线，非空有效 alias 表明确失败；完整 deferred ProviderRepository、模块生成/发布和 TTY 交互仍开放。

`ModuleExecution` 默认每 App 独立，可显式共享文件和 Manifest 执行边界；并发文件初始化等待当前 owner 完成，失败/取消使该文件后续明确失败，重新执行需新会话。这是 Rust 异步安全适配，区别于 PHP 捕获异常后仍可跳过已包含文件；不把操作系统进程当永久一次。递归检测只覆盖同一异步调用链；tokio::spawn不继承task-local，初始化器不得跨新任务等待自身同一文件。共享 Manifest 的空启用快照也缓存，空仓储 catalog 允许后续扫描；不热挂卸已加载资源。`ModuleRepository::get_path()` 读取同仓储当前主路径，但不重新执行既有 Provider。

从应用根使用持久管理命令：

```bash
cargo run -- module:list
cargo run -- module:list --only priority --direction desc
cargo run -- module:disable orders --no-interaction
cargo run -- module:enable orders --no-interaction
cargo run -- module:enable --all --no-interaction
```

全部名称先检查，再逐项写入；重复保留，`--all` 覆盖显式名单，异常保留前项并中断。管理命令仍完成唯一应用 bootstrap；坏 metadata 或启用项加载失败不会另开绕过启动器。显式 file 缓存本实例状态，自身修改先改缓存后写盘；写失败不会回滚缓存，外部改盘在现App不可见。新实例才读取外部状态，Provider/路由不热卸载。需要实时状态时用 MODULES_ACTIVATOR=file-live 显式扩展，或 FileModuleActivator::new(path)?.with_live_reads()；构造失败可手工reset_file，不绕过完整bootstrap。

程序化 `ModuleManifest::new/from_file/from_config().module(ModuleDescriptor)` 仍支持显式清单和已授权 requires 拓扑增强；这是独立的显式输入方式，不能混入运行发现清单或冒称上游 Manifest 自动拓扑。

## 生成代码

生成项目默认在 `app/providers/app.rs` 实现 `AppServiceProvider`，由 `bootstrap/providers.rs` 实际登记。框架默认 Provider 由统一启动链按配置与编译 feature 安装；应用清单只写应用专属 Provider，模块继续走相同管线。当前编译期包 Provider 可显式登记，Cargo 包自动发现和 ProviderRepository 的 eager/deferred 缓存尚待后继对应。

应用 Provider 可在 `register` 中 `app.register_provider(ChildProvider).await?`，子项与根清单共用唯一注册/boot 管线和类型去重。启动时仍先完成全部 register 再 boot；不要手工转调子 Provider 的生命周期。真实公开生成 fixture 见 [Container consumer](../../tests/consumers/container-extension/README.md)。

`make:controller PostController` 在 `app/http/controllers/` 生成控制器类型并登记 Rust 模块，具体处理函数由开发者实现后在 `routes/web.rs` 或 `routes/api.rs` 注册；`make:provider CatalogServiceProvider` 在 `app/providers/` 生成 `register`/`boot` 骨架，并自动加入 `bootstrap/providers.rs`。`make:model Post` 在 `app/models/` 创建 SeaORM 模型，允许暂不声明字段，也不要求编入官方数据库驱动；它们都拒绝覆盖同名文件。原有 `generate model` 继续在 `app/models/_entities/` 生成字段驱动的 SeaORM entity；`generate migration` 在 `database/migrations/` 生成迁移并写入 registry。字段支持 `string`、`text`、`integer`、`big_integer`、`boolean`、`date_time`、`json` 和 `references`，在字段类型后加 `?` 表示可空。model 使用单数名称，表名采用简单英语复数；migration 接受表名。被引用表须在外键迁移前创建。

控制器支持相对命名空间。运行 `openrust make:controller Admin/UserController` 会生成 `app/http/controllers/admin/user.rs`，并在两级 `mod.rs` 登记模块。可以在路由代码中引用 `crate::app::http::controllers::admin::user::UserController`。命名空间分隔符也可用反斜杠，shell 中请用引号包住参数。绝对路径、越界路径、保留标识符和已有同名控制器会被拒绝。自动登记支持逐行外部 `mod` 声明、行注释与单行属性；已有无条件 `pub mod admin;` 命名空间可复用。同名声明使用其他可见性、raw identifier、属性或 inline 模块时拒绝；多行属性、块注释及其他自定义 Rust item 等未支持布局也会在写入前拒绝，不猜测解析结果。新增声明按完整模块 item 插入，保留原属性和文本。

控制器生成按可信、生成期间不被其他工具修改的工作区处理：预检拒绝已存在的符号链接，同命令排他锁防止两个控制器生成互相覆盖；这不提供恶意并发替换父路径的隔离。文件先写同目录暂存和原像备份，再安装；普通 I/O 失败会补偿恢复原文件并回收本次新文件/目录。恢复也失败时同时保留主错误、清理错误及恢复文件路径，需先检查并恢复残留，再重试。该机制不承诺进程崩溃或断电后的多文件原子性；遗留 `.openrust-controller.lock` 不能在另一个生成仍活跃时删除。提交后清理或终端输出失败会明确报告控制器已生成，不能按“未生成”盲目重试。

本命令目前接受控制器根目录下的相对命名空间；固定 Laravel 还支持应用完整限定名、前导命名空间分隔符及更多 stub/选项，这些对应项仍待交付，不能把当前有限输入限制当作上游规则。

两个数据库生成命令均支持 `--uuid-id` 与 `--soft-delete`，model 与对应迁移须使用相同选项。UUID 使用 UUIDv7，为应用及已有 migration package 启用 `db-uuid`；软删除增加可空 UTC `deleted_at`，只提供显式 `Entity::not_deleted()` scope，不改变普通 `Entity::find()`。

`generate mailer <name>` 生成实现 `openrust::mail::Email` 的邮件类型；`generate policy <resource>` 生成基于 `openrust::auth::Policy` 的资源授权包装，权限名为 `<resource>:read|create|update|delete`。`generate job <name>` 需要已有迁移 package 的应用结构，不通过 manifest 中的驱动文字推断能力；它会启用队列 feature；标准迁移清单由同一 `FrameworkMigrator` 贡献一次队列迁移，不再额外登记 `QueueMigration`。生成器同时生成显式失败的 handler 骨架，并把任务注册到应用队列 Provider；请先实现 handler，再启动 worker。上述生成器会登记模块且不覆盖已有文件；policy 需要通过 `--auth` 创建项目。

## 配置与验收

生成项目采用按领域命名的 Rust 代码配置，例如 `config/app.rs`、`config/cache.rs`、`config/database.rs`。`config/mod.rs` 显式登记各 `config(&ConfigContext)` 函数，`bootstrap/app.rs` 经 `ApplicationDefinition::config()` 接入统一仓储；函数在同次 dotenv 快照后求值，可组合类型、条件和环境值。框架仍允许显式附加 TOML source，但默认不会扫描或运行 TOML 文件。`.env.example` 按领域列出常用值：数据库使用 `DB_CONNECTION=postgres|mysql` 及 `DB_HOST`、`DB_PORT`、`DB_DATABASE`、`DB_USERNAME`、`DB_PASSWORD`、`DB_TABLE_PREFIX`；Redis 连接使用 `REDIS_HOST`、`REDIS_USERNAME`、`REDIS_PASSWORD`、`REDIS_PORT`、`REDIS_DB`；缓存选择使用 `CACHE_STORE`，映射到 `cache.default`，缺省为已登记的具名 `redis` store；队列使用 `QUEUE_CONNECTION`，标准默认 `database`；邮件、短信和国际化分别使用 `MAIL_*`、`SMS_*` 和 `I18N_DEFAULT_LOCALE`。缓存和 Redis 队列共用同一组 Redis 连接设置。监听端口由中性 `SERVER_PORT=3000` 映射到 `server.port`，`APP_URL` 可使用 `${SERVER_PORT}` 插值；应用字段仍使用 `APP_LOG_LEVEL`、`APP_NAME` 等命名空间变量。`APP_PORT`、独立 `PORT`、`REDIS_DEFAULT_STORE` 与 `CACHE_DEFAULT_STORE` 不再控制监听或缓存选择。`.env.example` 提供 `APP_KEY=`。使用 `openrust key:generate` 设置应用密钥，或加 `--show` 仅输出。应用命令使用本次启动选定的环境文件，生产环境已有密钥时须确认；签名 URL 使用原始 `APP_KEY` 字节，轮换旧值由 `APP_PREVIOUS_KEYS` 配置。复制 `.env.example` 为 `.env` 以配置本地或部署环境；不要提交含有密钥的 `.env`。生产环境可使用权限受控的 `.env`，进程环境变量优先；应用根目录不得作为静态公开目录。`openrust key generate` 输出一个 32 字节 Base64 密钥，不写入配置文件；请将输出保存到受保护的 secret store 或被忽略的 `.env`。加密只处理应用数据，不会加密数据库备份或 JWT。

未选择数据库、加密或 gRPC 的普通项目会生成 `tests/test_app.rs`：通过与生产同一 Provider 清单和 Router 的 `TestApp` 验证 `/api`、`/health`，使用显式内存 cache 与 view 目录，不要求 Redis 服务。数据库项目需真实数据库配置；加密项目需显式密钥；gRPC 项目生成自己的 `tests/grpc_flows.rs`，启动生成二进制的 `serve all`，用临时 TLS 证书验证真实 HTTP、Echo RPC、证书拒绝和信号停机。`TestApp` 公共 API 仍可用于这些应用，测试须自行提供相应配置。Web 认证应用的 `tests/auth_flows.rs` 使用真实 Router、账户、Cookie/Session/CSRF 验证 Fortify；JWT profile 的同名测试覆盖真实 API 管理、独立 TOTP 与逐响应无 Cookie，`tests/jwt_api_business.rs` 保留注册、邮件验证、密码重置、受管 PAT、refresh 重放和注销回归。运行数据库测试前，设置 `OPENRUST_TEST_DATABASE_URL` 为可创建和删除测试库的 PostgreSQL 或 MySQL 管理连接；不得把缺少变量时的跳过算作真实数据库验收；JWT profile 的网络数据库测试缺少该变量会明确失败。新生成项目应执行 `cargo fmt --all --check`、`cargo check` 与 `cargo test`；涉及数据库的测试还须在相应真实服务上实际运行。

普通生成应用的 `tests/test_app.rs` 还包含可选 `server.trusted_hosts` 测试：精确 `development` 环境下 `app.url` 主机通过，其他 Host 返回 400。生成配置保留 `server.trusted_hosts` 注释示例；应用 Provider 可经 `BootContext::trust_hosts` 或每请求 `trust_hosts_with` 覆盖。详见 [HTTP 可信主机](../../vendor/openrust/framework/src/http/README.md#可信主机)。

生成的 Provider 在 `register` 中返回 `BoxFuture`，绑定服务时调用异步 `RegisterContext::instance` 等方法并 `await?`。测试替换使用 `TestAppBuilder::configure_after_register`，其回调接收与生产相同的 `Container` 共享句柄，返回 `BoxFuture<'static, Result<()>>`；框架在全体 Provider 登记后、基础配置校验和 boot 前等待回调完成。运行期业务 Provider 可经 `openrust::foundation::Application::register_provider` 登记，显式重新登记使用 `force_register_provider`。

应用默认安装 `TranslationServiceProvider`。`config/i18n.rs` 中的 `i18n.default_locale` 与 `i18n.fallback_locale` 设定默认语言和回退；请求的 `Accept-Language` 选择受支持的语言。应用可通过 `MessageCatalog` 扩展 JSON 消息资源。

## API、worker 与 scheduler

参见 [`modules/example/`](../../modules/example/README.md)：同一应用可分别作为 API、worker、scheduler 进程启动，共用数据库和队列迁移。`serve` 不会隐式启动后台进程。两个数据库后端使用相同示例，只需选择 `db-postgres` 或 `db-mysql` feature。

生成应用的 `tests/support/request_input.rs` 经生产 Provider 清单追加受管理 `openrust::routing::Router::route(path, openrust::routing::get(...).post(...))`，验证真实路由名称、`route_is`、最终 URI/方法和缺参数默认值。直接使用 Axum `Router` / `MethodRouter` 属于原生逃生口，不具有上述受管理声明的匹配上下文保证。显式 `openrust::routing::Router::fallback` 按 Laravel 仅接受 GET/HEAD；POST 等方法没有对应业务路由时返回方法不允许；OPTIONS 存在可用方法时合成空响应与 Allow。

生成应用的 URL helper 同时调用 `Request::client_ip/client_ips/base_url`：没有注入连接信息的进程内请求分别得到 `None`、空列表和空前缀。真实连接经 `server.trusted_proxy_ranges` 指定可信代理，`server.trusted_proxy_headers` 使用公开 `TrustedProxyHeaders` 的位集合选择可采用的转发头；URL、协议、主机、端口和前缀由框架的同一请求上下文提供，应用不自行读取转发头作为可信数据。helper 的无连接断言不代表真实代理 TCP 验收。

### 生成项目的消息包参考

普通生成应用的 `tests/support/message_bags.rs` 通过测试业务Provider和TestApp演示公开MessageBag/ViewErrorBag、递归合并及JSON选项；`tests/test_app.rs` 调用此入口。它只注册测试路由，不增加默认生产接口。数据库认证模板使用独立的 `tests/auth_flows.rs`，不自动运行此helper；业务可参考唯一 `modules/example` 中的生产模块调用。

普通生成应用的 `tests/support/request_redirect.rs` 也只在测试 Provider 挂载业务路由：`Redirector::from_info` 生成普通响应，`from_request` 闪存排除敏感字段后的输入与命名错误包；公开 `ErrorBagCodecs` 扩展恢复自定义错误包，再用两个真实 Cookie 请求验证读取与老化。该测试不添加默认生产路由；数据库认证模板仍按自身测试入口验收。

生成应用用 `routes/mod.rs` 导出两类路由，`bootstrap/app.rs` 组合它们并配置 API 前缀和中间件组；API 文件声明相对路径，Web 文件保留应用 fallback。原有认证、MFA、PAT 与签名 URL 的外部路径不变；文件名本身不会自动启用 Cookie/Session，中间件组仍须显式声明。

独立生成应用会继承框架根清单的驱动 `[patch]`，本地路径转换为框架源码的绝对路径。Cargo 不继承依赖包自己的 patch；手写 workspace 须自行登记当前 MySQL 冷预处理取消修复，见[驱动说明](../../vendor/launchbadge/sqlx-mysql/OPENRUST.md)。


## 显式翻译来源（Source）

Source 簇已编码，运行验收尚未完成。框架默认 `TranslationServiceProvider` 仍不代表 Laravel 默认 FileLoader、locale/deferred 生命周期已经等价；官方 `get`/`choice`/Loader 完整接口继续保留对应缺口。下列应用安装器是显式增强，复用唯一 Container `Translator`，通过已有 extender 选择 `TranslationLookup::Catalog`，不建立第二套翻译服务。

根应用和新生成应用在 `config/translation_sources.rs` 编辑 `SourceSettings`。默认 `files` 读 `lang`；不登记数据库扩展，不要求 Redis 或后台任务。原语言配置仍用临时 `i18n` 键。未指定语言沿原 `i18n.default_locale`，请求协商复用同一选择规则。显式数据库初始捕获失败会阻止启动，不静默当成文件来源成功。

启用 `translation-database` 和相应 `db-mysql` 后，首次安装先保持 `mode: Files` 并设 `install_database: true`，运行标准 `migrate`；成功后改 `Database` 或 `Composite`。命名连接使用 `translation_database.connection` 与同一 `Databases`，迁移显式传 `migrate --database <连接名>`，Loader 连接不会改写命令目标。不要在新库尚未建表时直接选择数据库来源。

组合顺序固定为 `[files, database]`：同一 locale/resource/key 后者覆盖前者；空串是有效值，禁用/删除表示该来源缺失。先在请求的精确语言查全部来源，再按既有语言回退顺序查询。例如 `fr` 文件为 `Bonjour`、`en` 数据库为 `Hello` 时，显式 `fr` 返回 `Bonjour`。`en` 文件 `label=file`、数据库 `label=db` 返回 `db`；数据库值改空串返回空串；禁用/删除后返回 `file`。模块文件别名仅补缺，不能覆盖已捕获 Source 的精确全局键或 `namespace::key`；数据库删除并发布下一快照后恢复模块文件。JSON 字面键优先规则保持原有公开行为。

唯一模块示例使用命名空间 `example-source`，配置 `namespaces` 映射到 `modules/example/lang/source-overrides`；编译数据库扩展并登记来源后可运行 `example:translation show [locale]`、`set locale text`、`disable locale`、`delete locale`、`refresh`。该命令是运维入口，没有匿名 HTTP 写入口。写操作回执区分已提交修订、通知交付和提交后刷新错误，刷新失败不伪装数据库已回滚。

同步翻译仅访问已发布快照；HTTP `response_context` 在取得请求槽后、共享绝对期限及取消信号内核对刷新，再按原始请求头协商一次，后台任务显式刷新。批量捕获、校验和原子发布失败保留上次有效快照并记录脱敏固定错误码；通知是提示，有界修订核对负责最终收敛，不承诺全节点即时一致。外部 SQL 必须按契约推进修订或显式刷新。Windows 原子文件打开边界与完整 Laravel Loader 对应仍是公开缺口。

应用安装器在唯一 `default_providers()` 清单移出原默认 Translation 后追加到默认末尾，在 package register 前首次登记来源工厂与构造链装饰；不是 application Provider 列表中晚 force 替换。合法 package register 提前解析与 boot 后解析复用同一个 Arc/Source 目录。JWT 生成继续仅有一个 default_providers 函数，保留显式 Fortify 替换与 Cookie/Session 排除。
