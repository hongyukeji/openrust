# API-A01 用户认证入门套件

## 范围与状态

执行基线为[正式计划](../../plans/user-auth-starter-kit.md)及其S01至S10契约。2026-11-25用户授权持续实现A01-01至A01-05，遇到实现问题按固定基线与既有契约裁决；实测通过后分批本地提交，不推送。范围和完成门禁没有缩减。

| 批次 | 状态 | 剩余工作 |
| --- | --- | --- |
| A01-01 安装与基础认证 | 开发中 | 显式安装预设、基础账户流程与响应适配、安装保护、真实Artisan／HTTP。 |
| A01-02 邮件与生命周期 | 开发中 | 已验邮件落地、本地SMTP、加密可靠队列、无会话恢复及会话／PAT撤销；待无密码与跨驱动最终组合。 |
| A01-03 账户中心 | 未闭合 | 已验资料／密码、浏览器设备、个人令牌、头像及注销／资源恢复；待自定义模型／驱动和最后组合。 |
| A01-04 网页与可选认证 | 未闭合 | 基础认证、设备、令牌、头像、注销和2FA管理网页／API已验；待Passkeys／第三方登录管理、无密码确认及最终三语组合。 |
| A01-05 组合验收 | 未闭合 | 两种预设、三库／缓存、常用系统与最终支持矩阵。 |

## 首个实现单元：响应与安全资源

沿用固定Fortify 1.43.0的[LoginResponse](https://github.com/laravel/fortify/blob/v1.43.0/src/Http/Responses/LoginResponse.php)与[RegisterResponse](https://github.com/laravel/fortify/blob/v1.43.0/src/Http/Responses/RegisterResponse.php)：HTML依然委托原生响应，业务JSON仅由显式套件Provider替换。未将普通Fortify默认输出全局改写，也不因为存在响应绑定而声称安装预设可用。

旧版复用检查：`openrust-v0.x/vendor/openrust/framework/src/auth/fortify/api_http.rs`以及`tests/server/auth/fortify.rs`。旧JWT登录、挑战和刷新体系不符合本计划的Sanctum PAT／Session分工，未直接搬入；借鉴秘密不入URL与认证响应禁止缓存的边界，复用当前ApiResponse、Fortify响应契约和Auth上下文。

默认UserResource只读取id、name、email、email_verified_at，保留字符串主键；不读取整个模型、密码、remember token及因素秘密。自定义资源通过既有响应绑定替换。

### 本地验收（2026-11-25）

所有Cargo命令由`tests/build_guard.py --workflow user-starter`运行于`target/tmp/user-starter/source`冻结快照、Rust 1.94.0；不纳入其他对话未提交的Artisan输入修改。

- `cargo +1.94.0 test -p openrust-fortify --test starter_responses --locked`：8项通过、0忽略。共20次HTTP请求，其中3次经过回环TCP；覆盖JSON成功／未认证、原生HTML重定向、204零正文、密码重置响应与字段错误、三语消息及应用隔离。默认根User模型、时间属性转换及字符串身份均已实测。此处验证响应契约，不冒充完整注册／重置或数据库持久化流程通过。
- `cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings`与工作区格式检查通过；最终日志为`workspace-clippy-final.log`及`fmt-final.log`。
- 三语文档构建通过，既有全站浏览器检查与新增三语文章12组检查通过，覆盖`/`及`/docs/`、手机／桌面、公开类型与锚点、无横向溢出。新增页面日志`article-browser.log`，全站`browser-second.log`，构建`docs.log`。首次默认浏览器版本不可用的失败保留；随后显式使用本机Chromium1243通过，未下载额外浏览器。
- 构建中32GB停线自动安全清理释放82,322,956,288字节，清理后可用114,121,220,096字节；保留源码、日志、依赖、Cargo锁原inode、数据库及数据卷。首次中断的workspace检查不计通过，恢复后重验通过。继续复用本工作流的缓存，不每批清空。

原始证据在`target/tmp/user-starter/`。源码SHA核对清单为`response-source-sha.json`。这一单元不包含安装命令、完整UI、邮件投递、令牌签发、数据库／缓存驱动矩阵或Windows实机；上述范围按批次继续实施，没有因本单元通过而关闭。

## 安装与基础页面单元（进行中）

已进入实现：`init --starter-kit=full|api`、显式资源发布、`config/user.rs`、应用Provider、基础Blade页面及安全`/user`资源；`all`沿用原有扩展组合，不暗中加入用户套件。两种预设均显式开启原生stateful API，API预设保留最小邮件页面。配置与安装仍有待整体验收的改动，尚未提交、未计为A01-01闭合。

早期组合已运行：安装9项、配置3项、SQLite真实HTTP基础流程1项、Blade路由助手2项。随后新增了AST安装保护、第一方SPA接线和API信息根路由保持，须在最终冻结源码上复验；不能把早期通过直接沿用为这些新增代码已通过。真实Artisan隔离安装正在`target/tmp/user-starter/runtime-full`执行，日志`runtime-full-install-first.log`；不得操作开发数据库。

必要核心补齐：Blade全局`route()`／`URL::route()`复用既有URL生成器；Provider列表装配由AST定位唯一vec，支持已安装Fortify的let包装并保留cfg；source_edit公开只读预检／stateful API编辑用于安全安装，拒绝无法识别的自定义闭包。继续核对已有项目、API前缀、会话／Bearer、失败恢复、三库与三语页面，不放宽S01至S10要求。

### 生成项目的首轮实际结果

隔离`runtime-full`经真实`./artisan init --starter-kit=full --extensions=api,fortify --no-interaction`完成重建与SQLite迁移（`runtime-full-install-third.log`）。启动生成的main后，12次真实HTTP已通过：公开`GET /api`名称／版本，匿名`/api/user`401，HTML表单／CSRF，注册201、安全字段白名单，Cookie认证`/api/user`，本人页面与待验证提示，退出204／后续401，错误密码422及中英文登录消息。记录在`runtime-full-http.json`／`runtime-full-http.log`，服务已退出，独立数据与日志保留。

前两轮失败保留：第一次选择了初始化器不支持的array邮件驱动，预检拒绝且未改配置；修正为log后，第二次暴露默认API信息骨架与空API模板不一致，未完成资源装配。现已明确识别当前公开API信息骨架并保留ApiController入口，第三轮通过。真实邮件投递尚未据此通过，仍属于A01-02。

后续新增的环境预检与自定义函数安全编辑仍须最终复验；API-only完整安装、重复安装、`--force`、最终严格门禁及页面浏览器验收待完成。A01-01仍未闭合，不推送。

### 双预设与邮件落地增量（仍待最终提交门禁）

- API预设真实Artisan首次与重复安装通过，保留`API_PREFIX=api/admin`。14次真实HTTP覆盖公开信息名称／版本、注册201、Cookie用户读取、退出204、错误密码422、匿名401和语言切换；日志`runtime-api-install-first.log`／`runtime-api-install-second.log`／`runtime-api-http.log`。初轮脚本复用登录前CSRF令牌导致419，改为使用随Cookie更新的`X-XSRF-TOKEN`后通过，未关闭CSRF校验。
- 新增API预设邮件登录页、验证提示／收据入口和密码重置默认跳转；保留自定义前端重定向。HTML未认证复用异常渲染器及`Redirect::guest`保存intendedURL；API及JSON保留401。未增加另一套认证引擎。单独页面通过不等于完整邮件签名、投递、过期或凭据撤销已验收。
- 当前页面单元3项分别通过SQLite3.54.1、MySQL8.4.13和PostgreSQL18.7真实数据库，均经回环TCP；日志`api-landing-fourth.log`、`ui-mysql-final.log`、`ui-postgres-final-second.log`。PostgreSQL首轮夹具未开`prefix_indexes`，多套前缀表索引冲突，修正为项目实际默认配置后通过。没有修改数据库方言实现规避错误。
- 三语教程已补双预设、配置、重复安装与force、CSRF、受保护用户路径及未完成边界。文档构建与12组文章浏览器检查通过：`docs-install.log`／`article-browser-install.log`。固定Laravel `route()`助手签名依据为Framework基线的`src/Illuminate/Foundation/helpers.php`，复用公开URL生成器，非自建命名路由注册表。
- 最终冻结门禁仍在运行；新增CSRF／429测试与验证开关关闭时的默认落点须随最后源码复验。A01-01至A01-05均不因本段增量自动闭合。

### 已落定的装配裁决

1. 使用`init --starter-kit=full|api`明确选择；内部扩展名`user-web`／`user-api`只用于现有安装器依赖与资源分发，不另建包管理或全局默认套件。
2. 套件Provider在原生Fortify注册前装配配置，在所有注册完成后的boot中替换套件响应并注册视图；独立Fortify默认契约不变。
3. 两种预设均采用第一方Cookie默认值；Token登录与管理必须经S01／S03独立实施，不用已有Cookie通过冒充纯Token流程。
4. 资源发布沿用现有发布器；语言路径走应用`lang_path`，不写死根lang；编译／资源安装需要完整项目源码，未声明脱离源码的单文件二进制安装支持。
5. HTML未认证跳转使用既有异常扩展和`Redirect::guest`，保留签名原URL；不改签名路径、不把签名替换为其他授权。API前缀内及显式JSON请求不应用HTML登录跳转。
6. 完整账户中心、设备枚举、PAT近期证明、撤销并发和外部清理属于后续实际实现门禁；当前文档明确标为开发中，不能仅因同名路由或历史模块验收而闭合。

后续生命周期实现继续核对固定Fortify 1.43.0 PasswordController（更新Action、重置令牌删除与事件顺序），以及Jetstream浏览器会话的database驱动要求。套件扩展的跨凭据撤销严格按S02执行，不将数据库会话枚举等同于通用撤销机制。

### 安装单元最终验收与交付边界

最终冻结父提交为`229301926`，源文件／教程SHA在`target/tmp/user-starter/installation-source-sha.json`。`install-gate-resumed-second.log`完整通过：安装10、配置5、响应8、SQLite网页／隐私／API落地／CSRF与429共4、Blade路由2、源码安全编辑3、根init预检2，共34项，0失败0忽略；root init另14项未选择不计通过。相同冻结源码的workspace／all-targets严格Clippy与fmt通过。三语字典键一致；38份交付源码／教程与冻结快照逐字核对。

追加最终MySQL8.4.13、PostgreSQL18.7各4项全部通过（`ui-mysql-delivery.log`／`ui-postgres-delivery.log`），无忽略；两种原生数据库测试容器均已由夹具删除，无数据卷。浏览器／文档证据复用本单元未再变更的三语教程检查，不重复计算为新增功能。

最终CSRF测试首次失败是夹具声明`Sec-Fetch-Site: same-origin`触发原有来源校验分支；移除声明后按真实令牌路径验证419，并证明5次错误登录后第6次429。未关闭来源或CSRF保护。关闭验证时API预设默认回到根页面，不跳往未注册的验证提示；显式自定义前端跳转保持不变。

一次最终严格检查触及32GB停线未计通过，守卫收尾报告Operation not permitted。确认无活跃构建后复用同一Project清理器取得所有Cargo锁，直接清理83,390,291,968字节，清理后可用115,385,188,352字节（`install-cleanup.log`，锁inode保留）。恢复后重新完成上述严格检查，日志／源码／下载／数据保留。继续复用缓存，不删除正在占用的资源。

本次只交付双安装预设、基础页面、API自读取与必要框架接线，不宣称A01-01整阶段闭合。邮件投递／签名完整链路与全凭据撤销、纯Token协议、头像／设备／注销、额外因素UI与发布矩阵仍按A01-02至05继续开发；实际供应商和外部设备未验证。

## 邮件与密码会话单元（A01-02 进行中）

### 实现与裁决

- 实测发现登录后尚未访问受保护页面的会话没有旧密码指纹，重置后首次访问会被错误接受。套件显式设置`auth.session_fingerprint_on_login=true`，SessionGuard在登录及有效remember恢复时立即保存既有HMAC指纹，并在每次解析持久会话身份时核对；缺失／过时指纹清理当前设备并拒绝认证。实际业务路由不必靠另加`auth.session`才能得到这项套件保证。独立认证默认false；不把此增强声称为上游新增配置。
- 同一开关接入原生与Sanctum的AuthenticateSession，旧会话不能因缺少指纹绕行。没有删除其他设备的Session行来冒充通用失效，也未扫描file／Redis键。密码更新仍保留当前会话，重置沿用原生remember token轮换。
- 实测发现关闭验证时，默认User仍实现验证接口，注册会尝试生成不存在的验证路由并返回500。套件仅在关闭时通过现有`NotificationSending`事件取消`VerifyEmail`的mail渠道；不移除Registered监听器，不取消密码找回。注册、资料更新与后续找回均已实测。
- 邮件链接复用原生签名、所有者／当前邮箱校验、密码代理claim／finish及幂等处理。API最小登录保存原始签名URL；验证后保留原生`?verified=1`跳转并显示真实数据库收据，伪造查询参数不能验证账户。HTTP夹具模拟浏览器Cookie与重定向，不冒充已完成应用页面的Chromium控件验收。

### 最终验收（2026-11-25）

本单元父提交`7918a9e61`，交付13份源码／教程与冻结快照逐字一致，SHA在`target/tmp/user-starter/lifecycle-source-sha.json`。

- `lifecycle-final-sqlite.log`：套件页面与邮件11项、配置5项、响应8项通过；11个页面／邮件用例还分别在file及database会话重跑通过。这里是相同用例的驱动矩阵，不虚增为33个独立功能。
- `lifecycle-mysql-final.log`／`lifecycle-postgres-final.log`：真实MySQL8.4.13与PostgreSQL18.7，database会话，各11项通过。`lifecycle-redis-final.log`：独占`redis:8`镜像及真实Redis会话11项通过。SQL／Redis临时容器均由既有夹具按批次标签回收，未挂载持久卷。
- 覆盖注册、登录、隐私闪存、CSRF与429、签名篡改／过期、重复验证不重复发信、换邮箱后旧链接403、关闭验证保留找回、密码修改保留本人并拒绝其他未访问过的会话、重置后普通／remember凭据拒绝。相同重置令牌并发兑换在三库路径中只允许一个200，另一个422，后续再次使用422；没有重写令牌存储。
- `lifecycle-gate-first.log`：Sanctum5项通过（明确过滤缺环境的MySQL父用例，不计通过），包含新增首次SPA访问前修改密码及缺失指纹拒绝；原生Session SQLite父用例1项通过，其他未选择用例不计通过。workspace／all-targets严格Clippy、额外Sanctum feature的all-targets严格Clippy及fmt均通过。
- `docs-lifecycle.log`文档构建通过；`article-browser-lifecycle-final.log`三语、两个挂载路径、桌面／手机共12组通过，验证邮件／会话章节锚点、正式接口与无横向溢出。

### 保留的缺口

这不是A01-02闭合。当前邮件是可捕获的本地array传输；真实SMTP捕获、队列失败重试与生产供应商实际收件仍须分别验收。跨进程／节点凭据生效、PAT撤销／并发签发、无本地密码账户的全设备撤销尚未闭合；不能把本单元的密码浏览器会话结果扩大到这些范围。下一单元优先复用现有`Model::refresh_for_update`、`save_in_transaction`及密码代理claim机制核定共同写入边界；不另建认证或令牌引擎。


## 令牌前置：PostgreSQL 归属与时间绑定（已验）

沿用固定 Sanctum 4.4.0 的模型关系、属性转换与生命周期，不另建令牌存储。复核发现归属查询错误地向 PostgreSQL 发出 MySQL `BINARY` 表达式；现仅在 mysql／mariadb 连接追加相应精确匹配。自定义模型的 Timestamp 属性转换产出 ISO 文本，PostgreSQL TIMESTAMP 参数绑定现接受数据库日期文本及 RFC3339 文本，按无时区列保留当地日期时间；Null 使用明确时间类型，未转换整数／非法日期明确拒绝。未改 TIMESTAMPTZ。

最终冻结快照为 `target/tmp/user-starter/source`，源码摘要 `pat-source-sha.json`。Rust 1.94.0，全部经 build_guard：

- SQLite `sanctum_authentication::bearer_and_opt_in_spa_over_http_with_sqlite`：1 项通过，内部覆盖标准整数及自定义 UUID／ULID；两个具名连接通过同一个临时文件共享数据库。初轮独立内存库、随后未创建文件导致的夹具失败均保留日志，修正夹具后通过。
- 独立 MySQL 8.4.13 同测试的 MySQL 目标：1 项通过，实际设置 URL 和必需标志；覆盖整数／UUID／ULID，日志 `pat-mysql-regression.log`。
- 独立 PostgreSQL 18.7 明确运行两个 `--ignored` 基础设施测试：2 项通过、0 忽略。HTTP 目标内部覆盖三种主键、归属、模型事件／转换、权限、撤销、清理与 SPA；时间绑定目标覆盖数据库文本、Z／+08:00 文本、Null、整数及非法文本。日志 `pat-postgres-delivery.log`。
- 工作区 all-targets 严格 Clippy、框架 sanctum all-targets 严格 Clippy 与格式检查通过，`pat-final-gates-second.log`。三语 Sanctum 文档构建通过，三语／两种挂载／桌面移动共12组浏览器检查通过，`pat-docs-browser-final.log`；首次浏览器期望大写 TIMESTAMP 与正文小写不符，修正断言后通过。

这批只闭合必要数据库兼容性修复；不代表 PAT 撤销并发或 A01-02 闭合。SQL Server、MariaDB 原生服务及生产供应商没有据此验收。临时数据库容器已按夹具退出清理，数据卷未操作；源码、日志与正常增量缓存保留，继续执行凭据生命周期。


## 持久化凭据版本与事务签发（本批验收通过）

本批补齐S02／S03／S07的底层生命周期与生成Action，不代表A01-02／A01-03整簇闭合。

- 套件独立发布用户 `credential_version` 与令牌 `credential_stamp` 迁移模板，按原生发布机制生成时间戳文件名，跟随实际模型表及连接；不改基础迁移。启用套件后旧会话须重新认证、旧PAT须重新签发。会话摘要绑定身份、Guard、密码及持久版本，无密码账户也能撤销；PAT摘要绑定归属身份与版本，普通密码重哈希和原生其他浏览器退出不误撤销PAT。摘要及令牌秘密保持隐藏。
- 签发、密码变更与全设备撤销共享用户行写锁；SQLite先无变化UPDATE取得写锁。锁内刷新并核对原身份／凭据，模型事件取消或事务失败回滚，不破坏调用者模型。跨连接令牌存储在写入前明确拒绝。Provider读取主库；remember／重哈希条件写防止撤销后的旧流程恢复凭据。
- 保留标准 `User::create_token`，复用自定义随机令牌生成、模型事件及属性转换；30天上限、服务端能力白名单、显式更短期限及Sanctum全局更短期限分别生效。属性快照支持自定义瞬态模型状态。期限检查和清理采用所属应用时钟。
- 生成Action只按可识别AST安全升级，保留业务函数、宏语义、换行、其他trait覆写；识别普通表达式格式差异。真实重复安装发现原生 `install:api` 只按空实现字符串判断、可能追加重复trait，已改AST识别并补回归。Fortify关闭时保留原生令牌方法，Sanctum关闭时迁移仍可编译，不误执行令牌迁移。
- 仅注册响应适配器不会改变Guard、默认令牌模型或启用凭据策略；显式应用套件Provider才启用。

### 实际证据

所有原始日志／冻结源码在 `target/tmp/user-starter/`；清单与SHA为 `credential-files.json`、`credential-final-source-sha.json`。

| 范围 | 实际结果 | 记录 |
| --- | --- | --- |
| 原生认证与AST | 配置扩展2、生成方法4、SQLite会话6、Sanctum5通过；MySQL不具备环境的目标已筛除，PostgreSQL外部目标不混计 | `credential-final-gates-second.log` |
| 套件公开入口 | 安装11、响应9、SQLite生命周期1、TCP界面13通过 | 同上；最新2030时钟的SQLite目标再次明确运行1项通过：`credential-sqlite-final.log` |
| 初始化与生成 | init16、startup28通过，含保留签发覆写的重复API安装 | `credential-startup-final.log` |
| MySQL 8.4.13 | 独立夹具明确运行生命周期1、数据库Session HTTP13、原生整数／UUID／ULID令牌1，全部通过 | `credential-matrix-delivery-second.log` |
| PostgreSQL 18.7 | 独立夹具明确运行生命周期1、数据库Session HTTP13、原生整数／UUID／ULID令牌1，全部通过；需外部环境的目标用 `--ignored` 显式执行 | 同上 |
| 撤销与时钟 | 三库覆盖无密码账户、不可用只读端点、并发签发／撤销、旧remember条件写、事件取消、归属替换、自定义模型瞬态属性、30天／更短期限／全局期限及真实清理命令 | 同上及SQLite最终日志 |
| 真实生成项目 | 实际Artisan升级及重复安装；两份迁移首次执行、重复执行、回滚两步、重执行；15个TCP请求验证标准User签发、隐藏字段、进程重启、bcrypt4→6后旧PAT有效、修改密码后旧PAT401且当前Cookie保留、新PAT可用 | `credential-runtime-upgrade-fourth.log`、`credential-runtime-delivery-third.log`及HTTP摘要JSON |
| 可选特性 | 生成项目 `cargo check --no-default-features --bin main` 及仅 `--features sanctum` 均通过 | `credential-runtime-feature-off.log` |
| 静态与文档 | 工作区all-targets严格Clippy、Fortify sanctum all-targets严格Clippy、格式检查通过；三语正文构建与12组浏览器检查通过 | `credential-final-recheck.log`、`credential-docs-final.log` |

失败与修正保留：早期2040测试时间超出MySQL原生TIMESTAMP范围，改为三库均支持的2030固定时间，不更改原生列类型；原始失败日志 `credential-matrix-delivery.log`。生成项目两次验收路由夹具错误（不存在post_with、回调内重复借用路由器）修正后才计真实HTTP通过，不把夹具错误算作产品行为。启动测试中一条已过时英文提示断言改为当前中文，单独提交 `c7ee3d96e`；未修改Provider运行逻辑。最终重查曾误用不完整测试名导致0项运行，不计通过，随后明确运行sqlite完整目标1项通过。

磁盘：`credential-runtime-upgrade-second.log`触及32GB，守卫安全停止并持Cargo锁清理全部项目编译产物，实际释放81,995,673,600字节，之后可用114,223,865,856字节。中断不计通过。源码、日志、数据库及数据卷保留；最终三库夹具临时容器已退出清理，正常增量缓存继续复用。收口时剩余空间见 `credential-delivery-space.json`。

仍待后续：无Cookie令牌登录／额外因素挑战／近期证明、全设备HTTP入口、完整账户中心与头像、可替换注销及持久外部清理、SMTP／通知队列与跨进程／节点的整体验收、可选因素UI和最终安装矩阵。三语教程明确服务能力与尚未交付接口；不将本批通过扩大为A01-01至05闭合。真实供应商、硬件与生产发布继续单列。

## 无 Cookie 令牌登录与持久挑战（本批验收通过）

本单元沿用 S01、S02、S07，复用固定 Fortify Provider、Timebox、因素校验、原生恢复码事件及 Sanctum 事务签发，不另建访问令牌。默认 Cookie 登录保持不变；只有显式套件 Provider 注册 `/auth/token` 与 `/auth/token/challenge`，路径跟随 `api.prefix`。近期重新认证、令牌管理和完整账户中心仍继续开发。

### 实现与边界

- 密码登录遵循自定义认证回调、用户名规范化、命名登录限流与错误事件；密码重哈希前固定凭据版本，之后从主库重新确认密码／版本。设备名与能力请求校验，服务端白名单及未验证用户能力交集在用户锁内再核对；调用者不能放大权限。
- 已启用且已绑定双因素时返回200和 `auth_challenge_required`，不返回访问令牌；完成 TOTP／恢复码后201。只返回一次明文 PAT，持久化摘要，沿用30天／更短全局期限。包含证明或访问令牌的成功响应 `no-store`，无 Cookie、无 Session 依赖；秘密不进入验收摘要。
- 由现有迁移发布器增加 `user_auth_challenges`。不透明随机键仅保存摘要，载荷绑定用户、多态归属、用途、设备、已批准能力和凭据状态；AppKey HMAC还绑定有效邮箱／因素策略与因素状态。使用用户所属连接、主库读取、行锁和锁内时间检查；SQLite先取写锁，JSON精确比较避免MySQL排序规则放宽绑定。单次消费后再发权限，事务签发前再次核对期限及最新状态。中断可要求重新登录，不遗留可重放授权。
- 原生公开 `replace_recovery_code` 保留固定上游字符串替换行为；认证消费采用新增内部事务入口，锁内刷新、核对密码／版本、精确验证恢复码、模型事件取消回滚，再保存及提交后派发原事件。两个独立挑战并发使用同一恢复码只能成功一个，不能靠各自进程快照重复使用。
- 显式套件增加 `auth:prune-challenges --hours=24`，`--hours=0`立即清理已过期记录；负数／溢出拒绝。复用应用时钟与 Artisan，调度由应用选择。编译内置三语默认值只补缺键，保留已有应用语言文件及运行时覆盖；升级不用覆盖开发者字典。
- 安装器按AST识别并升级旧标准Provider／模型能力，保留业务改动；先写模型能力再启用新泛型路由，避免中断后留下无法编译的Provider。第三份迁移按真实模型连接生成，不改现有账本。

### 当前实际证据

冻结源码为 `target/tmp/user-starter/source`，当前父提交 `f55b5f4c0`，最终交付SHA与范围在本单元结束时记录。

- `client-tenth-gates.log`：安装11、持久挑战SQLite1、真实TCP HTTP16通过；MySQL／PostgreSQL标注忽略的2项未计通过。Fortify sanctum/all-targets 严格Clippy通过。新增旧版语言目录用例验证缺键默认值、保留自定义登录文案和真实Artisan命令分派／重复清理／非法参数。
- `client-matrix-first.log`：独立MySQL8.4.13与PostgreSQL18.7各持久挑战1、数据库Session真实HTTP16通过，基础设施测试用 `--ignored` 明确执行；MySQL还实际运行原生认证14和因素模型4。挑战主库读取另配置不可用只读端点，成功证明没有误走副本。
- `client-docs-first.log`：三语文档构建通过，两个挂载路径、桌面／手机12组浏览器通过，核对新章节锚点、公开路径及无横向溢出。

早期新增测试错用了 `Application::handle_command` 的输出参数和 `count()` 无参形式，修正为真实 `Artisan::call` 与 `count("*")` 后才通过；失败日志保留。`client-sixth-gates.log`曾在代码生成时触及32GB，未计通过；守卫持锁清理81,829,707,776字节，之后可用113,779,240,960字节，后续恢复真实重编译。源文件、日志、数据和依赖下载保留。

### 最终补验

- `client-runtime-first.log`：真实生成项目执行两次 `./artisan init --starter-kit=full --extensions=api,fortify --no-interaction`，旧标准Provider安全升级并保留已有应用配置；挑战迁移首次执行、重复执行、回滚一步、重新执行通过。实际 `./artisan auth:prune-challenges --hours=0` 成功。
- `client-runtime-http-fourth.log`与摘要JSON：21个真实TCP请求。使用原生密码确认、2FA绑定／确认和恢复码读取端点建立因素；没有绕过生产Controller直接给用户写因素。两个独立应用进程共享SQLite数据库和file缓存，签发挑战的进程终止后由另一个进程兑换；新进程重启、并发双节点仅一个201、重放401、不同AppKey实例401且不会烧掉合法实例的证明，均通过。过程的长期PAT、挑战、因素秘密不写摘要日志。首次夹具未刷新注册后CSRF返回419，修正为重新读页面；密码确认按原生201断言。错误响应不含秘密，失败返回沿用原生异常头；成功证明／签发返回no-store，不宣称全部异常自动增加该头。
- `client-final-gates-first.log`：workspace/all-targets严格Clippy、Fortify sanctum/all-targets严格Clippy、格式检查通过；最终安装11、HTTP16、原生认证14、因素模型4通过。此轮不带外部连接的MySQL条件分支不计真实MySQL证据，真实证据以上述独立数据库矩阵为准。Fortify无Sanctum的all-targets检查通过。
- 生成项目仅Fortify而关闭Sanctum的 `cargo check --no-default-features --features fortify --bin main` 通过，记录在 `client-runtime-feature-off.log`。最终源码范围／SHA为 `client-files.json`、`client-final-source-sha.json`；上游复用及协议差异见本节和唯一计划S01至S10。

三库临时容器已由验收入口回收；三个HTTP节点退出；保留源码、日志、数据与正常增量缓存。本批收尾空间见 `client-delivery-space.json`。仍继续实现近期重新认证、当前令牌退出和本人令牌管理、无会话邮件闭环、完整账户中心／头像／注销、可选因素UI、SMTP／队列与最终安装矩阵，不将本单元当作任一整阶段闭合。真实手机、供应商短信／邮件、Windows和生产部署不由本机TCP测试冒充通过。

## 近期确认与本人令牌管理（A01-03 本单元验收通过）

本单元复用已发布挑战表、Fortify确认回调、Sanctum模型和Eloquent事务，未添加第二套认证凭据。完整账户中心、退出所有设备、注销和无状态邮件闭环仍继续开发。

### 公开入口与安全裁决

| 入口 | 实际契约 |
| --- | --- |
| `POST /api/auth/confirm` | 验证密码／应用确认回调，失败422并受5次／分钟限制；PAT返回201及单次proof／到期时间，Cookie写原生Session确认并返回201空载荷。 |
| `GET /api/user/tokens` | 本人安全元数据白名单，不含令牌摘要、版本摘要或内部归属；PAT须有tokens:manage。 |
| `POST /api/user/tokens` | 近期确认后201，仅本次返回明文；服务端／未验证策略与父令牌能力共同限制，未指定abilities时默认交集。 |
| `DELETE /api/user/tokens/{token}` | 近期确认、本人归属及能力检查，成功204空正文；其他用户ID404。 |
| `POST /api/auth/logout` | 只撤销本次实际PAT，成功204，不接受其他token参数替代当前凭据，不要求近期证明；其他设备和Cookie不受影响。 |

API路径跟随既有`api.prefix`；端点由显式套件Provider注册，不改变普通Fortify路由和默认Cookie登录。

- Cookie沿用原生`auth.password_confirmed_at`，可以使用`/user/confirm-password`或原生Passkey确认后的标记；不同Session不共享。缺失／过期423，CSRF仍419。按固定[Framework RequirePassword](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/Middleware/RequirePassword.php)的Unix秒边界，在timeout当秒仍有效、下一秒过期。PAT证明在到期秒即失效并且单次消费。
- 核对固定[Fortify ConfirmablePasswordController](https://github.com/laravel/fortify/blob/v1.43.0/src/Http/Controllers/ConfirmablePasswordController.php)发现原生实现使用应用Date时钟；本地Session确认原先用系统时钟，导致应用测试时钟与密码中间件不一致。现仅确认写入／检查共用应用时钟，不改Cookie会话载荷寿命的系统时钟。2030测试覆盖原生中间件与套件接口的相同边界。
- PAT证明绑定身份、实际当前PAT、用途、AppKey及有效凭据／因素／邮箱状态；持有其他设备Token、其他用户、错误用途或旧证明不能换取权限。只存摘要，失败可消耗证明，重试先重新确认。无密码账户不会自动通过；无状态Passkey适配仍未交付。
- 签发及撤销使用同一用户写锁；重新锁定当前PAT后再次执行原生期限／自定义认证和归属回调，并核对最新能力。签发在模型事件之前和提交之前均检查证明，事件期间到期回滚，不只在Controller入口检查时间。目标令牌锁定后、删除事件结束后再次检查期限。
- 补齐`Builder::lock_for_update`和`get_in_transaction`的正式公开读取入口，保留Scope、转换、retrieved事件及预加载。声明连接及执行Scope后的真实主查询连接均须匹配事务，不静默改走池连接；内部既有跨连接关系加载语义不改。SQLite调用方先取得用户写锁。
- `PersonalAccessTokens::find_owned_in_transaction`／`revoke_token_in_transaction`沿用配置模型，精确检查原始主键及多态归属；支持整数、UUID和ULID。Scope不能放宽归属；删除事件取消返回错误并由外层回滚。`Sanctum::validate_token_for_owner`复用既有期限和回调，不平行实现凭据有效性。
- 错误／成功消息沿用本地化ApiResponse；证明、密码和令牌不进入old input。新证明／令牌成功响应no-store；204零正文。额外展示字段使用白名单，没有序列化整个令牌模型。

### 实际证据与边界

父提交`cf774d781`。冻结源码`target/tmp/user-starter/source`，交付清单／SHA：`account-files.json`／`account-final-source-sha.json`。

- `account-tests-eighth.log`：SQLite完整TCP20、生命周期1通过；原生Sanctum整数／UUID／ULID父用例1通过；Request输入30、Session／Response门面8、密码校验3、Web验证4通过。两个未提供数据库环境的生命周期目标保持ignored，不计外部通过。新增错误测试夹具先缺Facade trait import、启用2FA后误将待挑战响应当登录成功、遗漏确认选项等问题已修正，失败日志保留。
- `account-matrix-final.log`：MySQL8.4.13、PostgreSQL18.7各生命周期1、数据库Session HTTP20、原生三主键模型1通过。最后新增的Scope连接拒绝、原始归属复查、Cookie临界秒和默认父能力交集再由`account-final-matrix.log`明确验证：两库各生命周期1、账户HTTP4、原生三主键模型1通过；需外部环境的目标显式`--ignored`运行，没有把缺环境分支当真实验收。
- `account-final-gates-second.log`：公开账户HTTP4、SQLite生命周期1、原生三主键1通过，workspace/all-targets与Fortify sanctum/all-targets严格Clippy、Fortify无Sanctum的all-targets检查通过。最终默认能力交集及缺键语言回归通过 `account-delivery-gates.log`；吸收无重叠的Console提交a0e4dcbdb后，`account-combination-gates.log` 的workspace与Fortify sanctum两组严格Clippy已通过，清理中断的单个命令／语言用例由 `account-combination-resumed.log` 恢复后1项通过，不把中断计通过。
- `account-runtime-final.log`：真实生成项目重新构建，30次TCP请求验证跨进程近期证明、七字段安全元数据、父能力控制的子令牌创建／撤销、当前PAT退出以及既有2FA重启／并发／不同AppKey拒绝；不绕过正式HTTP入口。`account-runtime-language.log`进一步通过31次TCP请求，补上旧应用字典缺键时的423中文回退；仅Fortify／关闭Sanctum的真实生成项目检查也通过。三节点退出；原生CLI升级／重入和迁移证据复用上一单元未变路径。
- 三语入门套件／Eloquent／Sanctum正文同步；`account-docs-final.log`文档构建与36组页面检查通过，覆盖三语、两种挂载路径、桌面／手机及新锚点／接口／无横向溢出。最终默认能力说明更新后 `account-docs-delivery.log` 再次构建及36组页面检查通过；随后无重叠Console正文提交不改变本批9份正文及浏览器路径，复用上述页面证据。

当前证明协议不接受测试假Token冒充真实可撤销PAT；敏感生命周期验收使用真实持久化凭据。Cookie原生Passkey确认标记可复用，但纯PAT无密码确认和可选因素的完整UI／接口仍按后续A01-04实现，不能据此宣称全套可选认证闭合。真实供应商、硬件、Windows与生产发布未由本机HTTP证据替代。

本单元实际父提交为 `a0e4dcbdb`；仅将已提交的41份Console／语言资源变化纳入组合快照复核，不提交它们。无重叠的在途 `tests/artisan_tutorial_docs.py` 保留；下一邮件单元的独立 `starter/api.rs` 草稿不纳入本单元。

磁盘守卫在组合用例代码生成时触及32GB，安全终止后取得所有Cargo锁，直接释放81,531,502,592字节，之后可用113,549,819,904字节。两项已完成Clippy证据保留，未运行完的单个用例恢复冷构建并通过；日志／源码／依赖下载／数据库与卷保留。两库临时容器与三HTTP节点均退出，正常增量缓存继续复用；收尾实际空间见 `account-delivery-space.json`。

## A01-02／03：无会话注册、邮件恢复与签名时钟

本单元沿用固定Fortify控制器、FormRequest、Action、密码代理和通知，没有另建身份或邮件队列。新增API均为显式用户套件扩展，不称为上游自动注册的默认路由。参考固定[VerifyEmail](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/Notifications/VerifyEmail.php)与[DatabaseTokenRepository](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/Passwords/DatabaseTokenRepository.php)的应用时钟；既有旧版API适配的正文凭据／不可缓存边界继续复用本记录前述调研，不搬回旧JWT体系。

- `POST /api/auth/register`：复用可替换CreatesNewUsers及Registered事件，201四字段资源，不自动建立浏览器会话、不签发PAT，不绕过额外因素。注册关闭时不装配该API。
- `POST /api/auth/forgot-password`：ControllerDispatcher沿用原生SendPasswordResetLinkRequest的正式验证／预检生命周期，成功200，业务验证／代理失败422。
- `POST /api/auth/reset-password`：复用原生重置控制器、配置Action及密码代理的单次消费，成功200。既有套件凭据版本继续拒绝旧PAT和会话；没有另外发明刷新令牌。
- `POST /api/auth/email/verification-notification`：认证后允许未验证用户重发，202已发送、204已验证；保留配置限流器和功能开关。
- 原生`verification.verify`签名路由位置及参数保持不变。套件默认将该路由的认证链设为已配置会话Guard优先、Sanctum回退，支持客户端对原URL提交Bearer；开发者自定义认证中间件不被覆盖。浏览器原有邮件登录与回跳保持。签名不允许更换Host或重写查询串；集群由公开Origin／负载均衡保留Host。此网页路由可以建立匿名Session Cookie，但不会因Bearer验证登录浏览器。
- 本批API认证分组统一禁止缓存，包含内层400／401／422／429等失败。非GET／HEAD拒绝查询串中的用户名、邮箱、密码、重置Token、OTP、挑战和近期证明等；凭据走正文或正式认证标头，语言参数仍可用。Cookie请求的CSRF没有关闭。
- VerifyEmail默认签发、URL默认验签和数据库／缓存密码重置策略使用现有应用Date时钟；URL生成器通过弱引用绑定所属应用，另一应用的当前作用域不能串用时钟。显式签名到期参数、URL／邮件自定义回调及系统时间默认值不变。

### 本批实际验收

父单元`e72f85254`；冻结源码`target/tmp/user-starter/source`。交付清单`mail-api-files.json`，最终组合HEAD见`mail-api-combination-head.txt`；复核中仅吸收其他已提交Console依赖，不改其代码或把其未提交工作夹入本批。

- `mail-api-tests-third.log`：SQLite 3.54.1新增5项真实TCP通过，含多级API前缀、注册输出白名单、Bearer签名验证、幂等重发、过期与并发重置、旧PAT失效、查询秘密拒绝及错误no-store。
- `mail-api-matrix-gates.log`：独立MySQL 8.4.13和PostgreSQL 18.7各新增5项通过，实际database会话；两库临时容器退出。SQLite完整套件25项、原生密码代理3项、URL6项回归通过；workspace及Fortify+Sanctum all-targets严格Clippy通过。
- 本地SMTP实际接收注册验证与密码找回邮件。一次临时451拒收返回500且未计投递成功；显式重发202后收件箱增长一次。与完整SMTP／TLS既有专项分开，不把本地无TLS捕获端点冒充真实供应商投递或生产收信。
- `mail-api-runtime-fourth.log`／`mail-api-runtime-http.json`：已有生成full应用实际重建后三进程20次TCP通过；配置关闭注册／验证404，本地SMTP捕获签名邮件、失败重发、跨节点验证、并发重置只有一个成功、新密码登录及旧PAT401。初轮验收快照混用Console版本的编译失败已修正；脚本误对公开`/api/user`要求认证分组no-store及跨端口丢失公开Host的失败均保留，随后按正式分组和公开Origin重跑通过。
- `mail-api-docs.log`：三语文档站构建通过，正文`client-mail`在两种挂载前缀／桌面手机共12组浏览器检查通过。

本单元不将账户中心、设备列表、注销／头像持久化清理、无密码PAT的Passkey近期确认、可选因素UI及最终安装矩阵计为闭合。认证默认邮件当前同步发送；自动队列重试／故障恢复尚待单独实现及真实Worker验收，真实外部SMTP和供应商收信留发布验收。自定义认证标识与模型主键不同的邮件生成／更新组合仍需在S10专项核对，不用默认User通过冒充高级自定义全部通过。

最终组合`985c71511`：`mail-api-final-gates-second.log`中新增URL所属应用时钟隔离1项通过；随后新增匿名Cookie断言初次引用了夹具不存在的`/api/admin/user`而返回404，改为夹具已有受保护`/api/token-me`后，`mail-api-final-gates-third.log`的5项通过，并确认匿名Cookie不能登录浏览器。该日志workspace严格Clippy54.30秒、Fortify+Sanctum严格Clippy36.46秒及全workspace格式检查均退出0。最终内容以`mail-api-final-source-sha.json`为准，临时SMTP／HTTP进程均已退出；文档构建输出直接删除，正常增量缓存保留，空间记录`mail-api-delivery-space.json`。

## A01-02／03：全设备退出（本子单元验收通过）

沿用S02既有持久化凭据版本及共享用户写锁，不增加第二套会话存储或撤销缓存。[Jetstream浏览器会话](https://jetstream.laravel.com/features/browser-sessions.html)区分database设备列表与其他会话退出；固定[SessionGuard](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/SessionGuard.php)区分当前设备退出。套件此处按已确认S02额外提供全凭据撤销，不将新增HTTP路由称为Laravel默认路由。旧v0.x Fortify目录及API适配未找到可直接复用的全设备版本撤销实现，继续复用当前已验的凭据服务和近期证明。

- `POST /api/auth/logout-all`，路由名`user.devices.logout-all`，跟随API前缀。PAT须有`account:update`及绑定当前PAT／用户的`devices`用途单次近期证明；Cookie须有原生Session近期确认与CSRF。成功204零正文，no-store；无密码不能自动确认。
- 用户共同写锁内重新校验当前PAT、归属、配置及证明，变更持久化凭据版本并清空remember token；保存事件结束后、提交前再查期限。取消保存、事务异常或期限在异步保存中失效，均回滚版本。失败可以消费近期证明，须重新确认后重试。
- Cookie当前设备使用原生`logout_current_device`清除当前Guard状态，再invalidate／轮换CSRF，避免版本提交后从旧用户快照写回remember token。无显式Fortify guard时选择静态`auth.defaults.guard`，不误用请求已选的Sanctum驱动。
- 此后新认证读取拒绝全部旧PAT、database／file／Redis会话及旧remember Cookie，不扫描Redis键或假设删一张sessions表等于通用撤销。已进入业务执行的请求不承诺撤回；用户仍可用原密码重新登录。旧PAT行保留供显式管理／清理，但不再能认证；此入口不声称设备枚举已完成。
- 同时将账户内部能力错误的缺失项按实际要求输出，避免account:update路径错误报告tokens:manage。

实际证据：`devices-matrix-first.log`，SQLite array、独立MySQL8.4.13／PostgreSQL18.7的database会话、SQLite file和独立Redis8.10.3会话五组各4项通过，覆盖Cookie确认、旧remember拒绝、多PAT撤销、错误设备证明、只读能力403、模型取消回滚、保存中到期回滚及并发单次证明一胜一拒。临时Redis使用tmpfs数据路径且无持久化，不清理现有实例／数据卷；三种临时容器退出。

`devices-runtime-first.log`／`devices-runtime-http.json`：生成full应用真实重建、两个应用进程及file会话，共21次TCP通过，跨节点PAT／Cookie全设备退出、错误设备证明、新登录、旧PAT与浏览器失效。`devices-docs.log`：三语站构建与正文两挂载／桌面手机12组浏览器检查通过。该项只闭合全设备退出，不将设备列表、头像、注销或剩余账户中心计为完成。

最终`devices-final-gates.log`：SQLite套件29项、凭据生命周期1项通过；未提供环境的另外2个生命周期目标ignored，独立数据库证据以矩阵日志为准。workspace严格Clippy25.50秒、Fortify+Sanctum严格Clippy13.08秒及全workspace格式检查退出0。吸收其间纯计划提交后的组合复核`devices-combination-gates.log`退出0，源码未夹带其他任务改动。交付清单／SHA为`devices-files.json`／`devices-final-source-sha.json`，空间及本批文档产物直接清理见`devices-delivery-space.json`；正常增量缓存、数据库、日志保留。

## A01-02／05：凭据策略的Guard／模型作用域（本子单元验收通过）

实际发现：套件原来将`auth.session_credential_version_attribute`与强制登录指纹全局启用，独立后台表没有该列时登录／remember写入会失败；Sanctum归属策略也会要求其他多态模型提供套件摘要。修正作用范围，沿用现有配置、Auth Provider、Session指纹及Sanctum校验，不新增全局Manager。

- 套件在注册用户路由时，以选中会话Guard、相同Eloquent模型配置的Provider及其真实连接／表装配`auth.session_credential_version_scope`。持久化版本查询／条件更新按连接和表匹配；Session指纹及必须在登录时写入的要求按Guard匹配。未配置scope的手动框架应用保留原有全局语义；无Sanctum的网页构建仅收窄对应会话Guard，不引入PAT依赖。
- 配置严格解析`guards`／`tables`，缺失字段、未知字段或空连接／表／Guard名称不静默回退。纯配置中可明确选择范围，不能信任客户端提供Guard／连接。动态Provider、异名模型别名、额外连接等手动组合仍须S10逐项核对；本项不宣称任意自定义配置自动等价。
- `credentials::configure_for<M>`只追加所选模型的凭据摘要检查。新增公开`Sanctum::add_token_owner_validator`复用既有归属回调槽，以逻辑合取追加并保留原有拒绝／错误；原`validate_token_owner_using`保持显式替换语义。原`credentials::configure`继续提供显式全局选择，但现在也保留先前检查。
- 新的账户路由用户模型中间件在Sanctum认证后限制为该套件显式模型；其他合法多态PAT仍能访问自身原生路由，访问套件确认／令牌管理／邮件操作返回401，不触发内部模型转换错误，也不误修改同ID套件账号。

证据：`scope-tests-fourth.log`包含新增实际TCP作用域1项、Fortify无Sanctum的网页11项、SQLite凭据生命周期1项、完整套件30项通过。两个缺数据库环境的生命周期目标ignored，不计外部验收。后续`scope-native-regression.log`明确强制管理员bcrypt重哈希成本由5变4并保留remember恢复与其他模型令牌；原生配置认证2、Session／Response门面8、Sanctum 6、Session认证7个目标通过，其中缺外部数据库参数的提前返回不计真实MySQL通过。

`scope-matrix-gates.log`：独立MySQL8.4.13／PostgreSQL18.7均实际执行Scope1＋设备撤销4；管理员表无版本列、用户／管理员数值ID同为1、原生重哈希及remember、其他模型PAT、跨模型401、回调保留及套件撤销通过。workspace严格Clippy28.96秒、Fortify+Sanctum16.56秒、无Sanctum37.79秒及格式门禁通过。

`scope-final-gates.log`：新增4种格式错误作用域的真实HTTP失败／恢复检查，与原Scope回归共2项通过；最终feature严格Clippy11.73秒及格式退出0。`scope-runtime.log`：生成full应用重建、两进程file会话21次TCP通过。`scope-docs.log`：三语站构建及两前缀／桌面手机12组浏览器通过。先前测试夹具的trait import、类型推断及HashOptions构造错误已修正，失败日志保留。

闭合范围是已选身份的策略隔离，不是完整A01-03或S10。下一步核对已有源码中验证邮件使用认证标识而非模型主键的组合，随后继续资料、头像、设备列表、注销与可选因素；队列及最终组合门禁不变。

## A01-02／05：验证邮件的自定义模型身份（本子单元验收通过）

固定Laravel13.40.0的VerifyEmail使用可验证用户的模型主键。实际发现本地通知和条件更新曾使用认证标识，认证标识为邮箱而模型主键为整数／字符串UUID／ULID时会选错身份。复用既有MustVerifyEmail、邮件通知、EmailVerificationRequest及Fortify控制器，不新增验证路由或通知管理器。

新增默认能力`MustVerifyEmail::get_email_verification_identifier`，保持原有非Eloquent实现兼容；默认User以实际模型主键覆写。通知路由ID与授权使用该标识；验证状态更新使用模型真实连接、主键、当前邮箱及条件状态，并使用所属应用Date时钟。请求还核对实际认证用户与可验证对象的具体类型，拒绝相同认证ID的其他模型。EmailVerificationRequest只在首次成功更新后派发Verified；重复访问保持幂等，不假造模型save事件。自定义邮箱认证模型须按三语教程覆写主键能力，Rust trait对象不隐式具有Eloquent模型能力。既有自定义URL回调优先级保留。

证据：`verification-key-consumers-second.log` SQLite原生Core和Fortify两项各遍历整数／字符串UUID／ULID，通过共60次真实TCP；`verification-key-final-matrix.log`独立MySQL8.4.13、PostgreSQL18.7各同样两项、60次TCP。覆盖签名ID、不同用户／模型拒绝、并发与重复Verified只一次、换邮箱旧链接失效、新链接、应用时钟和过期；不使用Controller单元调用冒充HTTP。`verification-key-resumed-gates.log`根User命名连接持久化1项及邮件回归10项通过，workspace和Fortify+Sanctum严格Clippy通过；共享fixture重构后最终矩阵日志再次通过两组Clippy及全workspace格式。初轮fixture缺cache导致原生限流500，已补齐夹具配置后通过，失败日志保留。

`verification-key-docs-final.log`三语文档构建及两挂载／桌面手机12组浏览器通过。沿用既有本地SMTP标准User证据，本轮不宣称新增供应商投递、原生PostgreSQL uuid列或生成应用自定义模型全矩阵通过。收口范围仅此身份差异，完整账户中心、头像、设备枚举、注销、队列邮件和可选认证仍未闭合。

过程中磁盘守卫安全持锁清理编译profile释放81,363,800,064字节，清理后可用113,389,518,848字节；保留源码、日志、下载和数据，仅恢复未完成命令。交付清单／SHA为`verification-key-files.json`／`verification-key-final-source-sha.json`，文档dist直接删除、正常增量缓存保留，实际空间见`verification-key-delivery-space.json`。

## A01-01／05：应用级原生用户资源（本子单元验收通过）

复核固定[JsonResource13.40.0](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Http/Resources/Json/JsonResource.php)的数据解析与外层响应职责，并保留固定Fortify的可替换响应契约。旧v0.x未找到可直接移植的同职责原生用户资源，不将旧JWT响应体系带回。

- 两种预设发布`app/http/resources/user_resource.rs`，实现原生JsonResource；应用Provider以`UserResource::use_resource<M,R>`选择。注册、JSON登录／双因素完成、当前用户及令牌签发中的用户共用真实请求资源。注册信息存于所属应用容器，不使用全局回调；类型不匹配拒绝认证，不回退序列化其他模型。
- 复用原生prepare_response与to_array，结果必须JSON对象；嵌入字段不套第二层Resource封装，不移植wrap、with、状态及HTTP响应钩子到外层认证响应。完整响应仍由Fortify响应绑定定制。默认四字段白名单保留；网页内部资料／邮箱字段与可改名的API资源分开。令牌签发前完成资源解析，失败不遗留调用方未收到的新令牌。
- 安装复用发布器、源码锁和原生模块AST；新增source_edit::declare_module验证独立文件模块与条件。重入保留应用资源，force明确替换；原有Provider标准模板可升级，不能安全识别的内联／条件布局在套件写入前停止，不覆盖业务。此子单元不等于S10任意自定义项目矩阵完成。

证据：`resource-tests-third.log`安装12、响应10（含两个应用9次TCP自定义／默认隔离、真实请求、异步准备、错误／非对象拒绝、状态及no-store）通过；完整回归`resource-final-gates-first.log`含Sanctum套件32、无Sanctum网页11／响应9／安装12通过，两组严格Clippy及fmt通过。新增资源失败不签发后的最终`resource-final-matrix.log`SQLite、独立MySQL8.4.13和PostgreSQL18.7各1项真实TCP通过，覆盖注册、Session登录、页面、PAT及本人管理签发、资源错误后令牌数量不变。临时容器退出。首次fixture把Request.path当作带前导斜杠，以及编译期Send／错误类型等夹具问题均已修正，失败日志保留。

`resource-runtime-second.log`生成full应用用真实`main artisan init:resources --extensions=user-web,api,fortify --no-interaction`发布并注册资源、再构建通过；初轮main遗漏artisan子命令的usage退出不计通过。`resource-runtime-http.log`生成应用两进程file会话21次TCP回归通过，默认资源与全设备退出／凭据隔离保持；自定义字段以三库原生套件HTTP证据为准。`resource-docs-first.log`三语构建及正文两挂载／桌面手机12组浏览器通过。最终`resource-delivery-gates.log`切回冻结源码并触碰首方入口确认实际重检，Fortify+Sanctum及workspace严格Clippy、格式全部退出0。

交付清单／SHA：`resource-files.json`／`resource-final-source-sha.json`。本批文档dist直接删除，保留正常增量缓存、数据和验收记录，实际空间`resource-delivery-space.json`。仅闭合资源装配；账户敏感更新、设备枚举、头像、注销／可靠清理、队列邮件、可选因素及最终安装矩阵继续开发。

## A01-02／03：事务化资料与密码API（本子单元验收通过）

延续固定Fortify的UpdatesUserProfileInformation／UpdatesUserPasswords、原生响应、密码代理和邮件通知；新增路由是用户套件扩展，独立Fortify入口保持。两种预设增加PUT `/api/user/profile-information`、`/api/user/password`，跟随API前缀，仅所选模型及account:update。资料（包括仅姓名）统一要求email用途近期证明，密码要求password用途；Cookie使用Session确认与CSRF，PAT使用绑定用户／实际令牌／用途的单次证明。

应用Action新增显式validate_for_transaction／update_in_transaction：先验证（默认Unique从真实模型选择表／连接），再在共享用户写锁的传入事务中保存，拒绝退回可能另取池连接的旧Action。默认密码Action通过公开credentials::change_password_in_transaction复用写锁、哈希和持久化版本撤销。新增Transaction::belongs_to的公开可见性，核对实际查询来源而非仅比较连接名。返回模型须同类型／主键／连接并重新从事务水合；密码必须已持久化变更、递增版本且清空remember，资料不许借机更改密码／版本；邮箱变化必须未验证。保存后提交前用锁定快照复查证明期限，允许已授权邮箱／密码改变安全摘要，不错误拒绝合法变更。

Cookie在提交前核对配置的Fortify会话Guard与本人类型／身份，提交后以SessionGuard::synchronize_user更新已解析当前设备的身份／指纹及轮换，不重新写remember token或派发Login；同时同步本次Sanctum身份。其他旧会话／PAT失效，当前Session保留并清除旧近期确认。邮件与密码代理清理按原生顺序在用户提交之后执行，其失败不会虚假回滚已提交数据，三语教程明确此边界。默认更新200本地化空载荷；能力、验证、证明及CSRF失败保留真实状态与no-store。

安装器用add_generated_trait_methods升级仍为标准实现的Action，保留原函数和注释／换行及新增方法属性；自定义旧方法在套件写入前报错，完整自定义事务实现保留，force沿用已有明确覆盖契约。未加入历史模板备份。此项未把原生网页更新改成新事务路径；网页近期确认与账户页面继续下一批接线，不能据API通过标记A01-02／03整簇闭合。

证据：`profile-matrix-second.log`SQLite、独立MySQL8.4.13、PostgreSQL18.7各9项真实TCP／事务通过（含原有资料回归1）。覆盖邮箱验证清空／旧链接拒绝、新邮箱登录、字段错误后证明仍可使用、错误用途／其他用户／重复证明、只读能力、模型取消及异步保存到期回滚、自定义Action、旧Action禁止绕行、并发同证明一胜一拒；两个应用同连接名的实际来源不同，密码事务拒绝且显式rollback／closed事务检查通过。`profile-session-gates.log`另有file和Redis8.10.3各Cookie生命周期1项，完整套件40、安装13、原生Action2通过，feature严格Clippy通过；临时Redis使用tmpfs且已退出，数据库容器亦已退出。

`profile-final-gates-first.log`补充SMTP451发生在邮箱提交后：HTTP500且no-store、旧邮箱登录失败／新邮箱可登录、显式重发202才增长收件箱；完整套件40／安装13、无Sanctum网页11／原生Action2通过，workspace与feature严格Clippy及fmt通过。`profile-docs-first.log`三语站构建与两挂载／桌面手机12组浏览器通过。初轮fixture的Builder借用、roll_back名称、JSON验证204与CSRF刷新错误均已修正，失败日志保留。

`profile-runtime-first.log`已有生成full应用实际重建，真实Artisan init:resources无force给旧标准Action追加方法，再重建及两进程file会话24次TCP通过：跨节点资料更新／证明重放、Cookie当前设备保留、PAT密码变更撤销旧凭据、新密码登录。无需覆盖业务或修改现有用户数据库Schema。最终吸收其间已提交Console依赖并重检，`profile-delivery-gates.log`workspace33.36秒及feature16.08秒严格Clippy、格式退出0。

交付清单／SHA为profile-files.json／profile-final-source-sha.json；原始输出均在target/tmp/user-starter，文档dist直接删除、正常增量缓存与数据保留，实际空间profile-delivery-space.json。下一单元优先实证并强化密码重置链接在邮箱再分配、密码变化并发旧快照时的身份绑定；原生按邮箱定位／提交后删除不等于这些边界已闭合。已核对固定Laravel DatabaseTokenRepository及Django6.0 PasswordResetTokenGenerator的身份／凭据绑定设计作为参考，不将尚未实现的强化计通过。随后继续网页、设备枚举、头像／注销可靠清理、队列邮件及可选因素。


## A01-02／03：密码重置令牌身份与领取后竞态（本子单元验收通过）

固定[Laravel DatabaseTokenRepository 13.40.0](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/Passwords/DatabaseTokenRepository.php)按邮箱定位令牌，本套件额外参考[Django 6.0 PasswordResetTokenGenerator](https://github.com/django/django/blob/6.0/django/contrib/auth/tokens.py)的身份／凭据绑定思路。下列是用户套件的安全增强，不宣称为上游原生强制行为。复用已有数据库／缓存仓储、原生64位十六进制客户端令牌、哈希器、节流和claim／finish；没有另建令牌表或URL协议。

实际先复现两个失败：password-token-baseline.log中，旧邮箱更换归属后旧链接仍返回200；password-token-claim-race-baseline.log中，令牌已领取而密码保存尚未开始时换邮箱，旧请求仍返回200。现在绑定真实模型类型、连接配置（含Schema／search_path）、表、主键、重置邮箱、密码摘要和可选凭据版本；旧快照在凭据变化后才写入仓储也不能得到有效链接。即使两个归属密码摘要同为空，实际主键仍区分。

Password::bind_user_tokens_using<M>为具名代理注册应用级、具名可组合规则，已缓存代理即时读取同一注册表；同名同类型规则可重复注册，其他类型保留原生语义。回调执行前释放注册表锁，空键／空值及回调错误明确拒绝。Authenticatable转为不同CanResetPassword包装类型时，必须显式注册该具体类型规则，不能静默绕过。默认独立密码代理未注册规则时仍使用原生哈希输入。套件bind_password_tokens<M>由生成Provider及手动API路由装配幂等调用，旧标准Provider可无force升级，自定义业务boot不覆盖。

绑定材料先摘要为固定64字符，再交给原有bcrypt／Argon，避免长字段触发bcrypt截断。共同用户写锁的内部凭据快照升级为user-credentials:v2并纳入重置邮箱，领取后变更也在保存前拒绝；PAT持久化token_stamp保持原v1，不因此无差别撤销现有PAT。既有短期确认／挑战及未绑定重置链接需重新申请。节点须使用一致模型／代理规则，动态租户连接及自定义包装由应用明确装配。

password-token-matrix-fourth.log中，SQLite3.54.1、独立MySQL8.4.13、PostgreSQL18.7数据库仓储及array／file会话、独立Redis8.10.3缓存与会话共五组各5项通过，覆盖邮箱再分配、旧快照迟到、同摘要不同主键、仅版本变化及已领取后换邮箱。临时容器已退出，未改现有数据库。核心password_user_bindings另覆盖具名规则组合、缓存代理、长材料、包装类型和错误关闭。

password-token-final-gates.log已通过核心1、安装13、完整套件45、SQLite凭据生命周期1及无Sanctum网页11；最终Clippy因32GB停线中断，不计通过，恢复日志为password-token-final-clippy-resumed.log。password-token-docs-final.log三语站构建及两挂载／桌面手机12组正文浏览器通过。实际运行与最终提交结果继续补入本节，不据此提前闭合A01-02／03。

最终password-token-final-clippy-resumed.log在安全清理后恢复，workspace严格Clippy1分34秒、Fortify+Sanctum56.66秒及格式退出0。原生仓储回归中的无环境外部目标不计MySQL／Redis实测，以上述显式矩阵为准。password-token-runtime-resumed.log生成full应用实际重建、真实Artisan升级原标准Provider并再重建通过；首次--locked因旧生成应用锁文件落后于已同步依赖声明失败，随后只在隔离生成应用中--offline更新，主仓库依赖未改。

password-token-runtime-http-fourth.log／password-token-runtime-http.json两个应用进程、独立file缓存／Session目录、本地SMTP、14次真实TCP通过：更换邮箱并由另一用户注册旧邮箱后旧链接422；新归属原密码仍可登录；新邮箱收到的新链接跨节点并发一200一422；旧PAT401、新密码登录及另一节点当前用户200。前两轮夹具复用了file缓存导致原生6/min共享限流429，CACHE_PREFIX不隔离原生file路径；改用本批独立APP_STORAGE_PATH，并等待正常限流窗口再测并发，不关闭或放宽生产限流。失败日志保留，本地SMTP捕获不代表真实供应商投递。

本项仅闭合重置身份边界，未闭合整个用户套件。继续网页事务更新／确认时快照、设备枚举、头像／注销可靠清理、队列邮件、可选认证和最终安装矩阵。交付清单／源码SHA见password-token-files.json／password-token-final-source-sha.json；本批文档dist直接删除，正常增量缓存与数据保留，停线清理及实际剩余空间见password-token-delivery-space.json。


## A01-02／03／04：网页事务更新与确认快照（本子单元验收通过）

复核固定[Fortify 1.43.0 ConfirmablePasswordController](https://github.com/laravel/fortify/blob/v1.43.0/src/Http/Controllers/ConfirmablePasswordController.php)及[Framework 13.40.0 RequirePassword](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/Middleware/RequirePassword.php)。沿用原生Session确认时间、确认Action、路由、响应和错误包；套件额外绑定确认时的身份状态，不改未装配套件的原生语义。web-sensitive-baseline.log先复现：其他设备完成邮箱变更后，旧设备仍可用旧确认修改资料，返回200而非预期423。

用户套件在所属应用容器保存三个类型化回调，将原生网页资料／密码控制器接到已有同连接用户事务与Action钩子，不另建用户存储或Manager。JSON API与网页共用锁定、持久化复核及当前Session同步；网页不强制JSON，仍使用原生重定向、闪存和错误包。新增RequirePassword::challenge只提取原生挑战选择，原Middleware继续复用它；未确认HTML请求保存正确的GET返回位置，不将PUT当作确认后的跳转地址。

原生密码确认成功及Cookie API确认成功时，在Session中保存确认时间和已有安全摘要，绑定当时的用户、邮箱、密码／版本与双因素状态。读取时同时验证时间及摘要；其他设备更改邮箱／因素后，旧时间戳不足以授权，旧版本没有快照的确认也须重新完成。缺失、到期或不匹配返回本地化423；浏览器跳转到原生确认页。密码账户网页还按实际选中的Fortify会话Guard验证current_password，先验证近期确认再执行这项旧密码检查，未确认会话不会通过422／423差异探测当前密码。字段验证失败不消耗PAT证明的原有约定保留。此项仅接通密码确认；Passkey近期确认及其他可选因素完整接线仍归后续单元，未宣称仅有原生时间标记即可通过套件保护。

账户页增加确认入口及密码表单，关闭对应原生功能时不引用不存在的路由。保留机器可读的七种Fortify闪存状态码，在视图中映射到user语言组显示；应用可覆盖user.json条目，其他状态仍沿用原翻译键。JSON业务响应的本地化不混入这些网页状态。

web-sensitive-gates-third.log：安装13、完整套件47、无Sanctum网页11通过；原生Session目标报告7，其中无MySQL参数的目标不计真实MySQL。web-sensitive-matrix-first.log对独立MySQL8.4.13／PostgreSQL18.7各4、file／Redis8.10.3各2项通过。web-sensitive-final-matrix.log再次在已提交公共依赖的组合下通过上述12项，并用独立MySQL实际执行原生accounts_http全部4项及auth_http真实数据库／Eloquent Provider1项，证明装有Sanctum但没有套件回调时仍保留原行为；先前无环境提前返回的日志不作为此证据。

web-sensitive-combination-gates.log吸收4d45b5609的公共依赖（不含其他任务WIP），完整套件47、已发布Action2与原生认证目标通过；workspace严格Clippy1分01秒、Fortify+Sanctum44.46秒及fmt退出0。最后的确认顺序收口见web-sensitive-confirmation-order.log：新增2项（含未确认时正确／错误旧密码都423、确认后错误旧密码422）及feature严格Clippy3.40秒通过；web-sensitive-order-matrix.log独立MySQL／PostgreSQL、file／Redis四组各2项通过，不重复未受影响的原生目标。

web-sensitive-runtime-final.log用生成full应用实际重建、真实Artisan init:resources --extensions=user-web,api,fortify --force --no-interaction发布视图并再重建，web-sensitive-app-browser.json三语及法语回退、桌面／手机共8组真实表单闭环通过：注册、确认跳转、资料更新、密码更新、当前会话保留、退出、旧密码拒绝、新密码登录、无横向溢出及无脚本错误。第一次6组通过后的手机截图暴露password-updated原样显示，已补齐三语七种状态并逐组断言；最终手机截图人工核对通过。仅在隔离应用显式force，不覆盖主仓库用户文件。web-sensitive-docs-final.log三语正文构建及两挂载／桌面手机12组浏览器通过。

过程中32GB守卫停线，等待其他构建退出后持锁清理，最后一次记录释放79,840,591,872字节、清理后111,710,760,960字节；源码／日志／下载／数据库保留。因反复冷构建和多套debug信息使缓存触线，本工作流后续通过target/tmp/user-starter/accept.py仅设置CARGO_PROFILE_DEV_DEBUG=1与CARGO_PROFILE_TEST_DEBUG=1，保留增量、正常并发与调试断言；主仓库及IDE配置未改。最终清单／SHA／空间分别为web-sensitive-files.json、web-sensitive-final-source-sha.json、web-sensitive-delivery-space.json。临时数据库和Redis容器已退出，文档dist直接删除，正常工作流缓存保留。

本项闭合网页资料／密码事务和密码确认时快照，尚不闭合完整A01阶段。继续可靠邮件队列、数据库设备列表、头像、DeleteUser可靠清理，以及Passkey／第三方登录／完整因素页面和S10安装矩阵。已验收且未受影响路径复用上述证据。


## A01-02／04：可选认证邮件队列与开发预览（本子单元验收通过）

沿用固定[Framework13.40.0 SendQueuedNotifications](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Notifications/SendQueuedNotifications.php)及[ResetPassword](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/Notifications/ResetPassword.php)：原生默认同步不变，套件只有显式设置user.mail.queue／USER_MAIL_QUEUE后，才为实际用户能力类型分别装配重置与验证通知的队列策略。公开AuthMailQueueOptions与ResetPassword／VerifyEmail::queue_for复用既有通知、JobOptions、AfterCommit及加密，不另建邮件队列系统。可单独恢复一类通知同步发送，缓存回调所属应用隔离；URL／to_mail自定义保留。未类型化ResetPassword::send保持同步。

安装生成config/user.rs的mail组，连接缺省遵循queue.default。拒绝空／空白／逗号名称、无效类型、sync／null／deferred／background及循环或非持久化叶节点failover；实例化所选真实连接确认注册。包装能力不默默失效，自动套件明确拒绝，允许按实际能力类型手动装配。Provider标准旧模板可升级，自定义Provider保留并给出接线错误；既有配置不覆盖，三语教程要求合并新组。关闭套件邮箱验证不创建验证任务，密码找回可继续入队。

命令均经target/tmp/user-starter/accept.py／项目build_guard、Rust1.94执行。auth-mail-full-gates.log中旧无Sanctum配置夹具缺auth.guards导致4项失败，未计通过；补齐真实web／admin会话Guard夹具，没有放宽生产校验。auth-mail-full-gates-resumed.log完整安装13、配置6、Sanctum套件49；无Sanctum配置6／网页11全部通过；workspace严格Clippy55.24秒、Fortify+Sanctum43.19秒与fmt退出0。SQLite3.54.1正式jobs列类型验证加密、App／模型／通知类型隔离、凭据不进入任务、Worker投递及链接消费。auth-mail-matrix.log独立MySQL8.4.13、PostgreSQL18.7各2项database队列，Redis8.10.3实际redis队列／Session2项全部通过；非仅改变量或无环境跳过，临时容器已回收。

生成full应用的auth-mail-runtime-build.log实际cargo重建、Artisan init:resources --extensions=user-web,api,fortify --force --no-interaction升级及再重建通过（仅本任务隔离应用force）。auth-mail-runtime-final.log／auth-mail-runtime.json使用独立SQLite、file缓存、三个应用进程与三个真实Artisan queue:work --once --tries=3：SMTP首次451后任务释放且未误报已投递，Worker重试成功；生产者错误端口／假凭据不影响Worker自身正确端口及无认证配置；生产者退出后重置通知仍可消费；验证204、重置200、重放422、旧PAT401、新密码登录201及当前用户200。首轮数据库文件路径拼错被保护断言阻止，改为本任务既有acceptance.sqlite，未访问开发数据库。日志不记录明文链接／令牌，验收摘要13条含3次Worker，供应商受理2封，待处理0。

Mailpit固定v1.34.0，镜像digest sha256:23a2ef85bc50853ab3093645d1d64679afd8887e89da5e48a88940c172591a4e；原共享旧镜像保留。auth-mailpit-runtime-final.log／json三语真实注册201、三次真实Worker退出0，API确认每封真实验证链接及各语言主题。首轮比较大小写收件人失败，修正验收断言为邮件地址规范化比较，没有修改生产逻辑。auth-mailpit-browser.json三语桌面／手机共6次打开实际邮件，脚本错误为空；手机截图人工复核显示繁体正文与按钮。容器只暴露随机回环端口、tmpfs存预览、没有真实供应商转发；结束直接删除本批容器，不动数据库／数据卷。教程提供宿主机Mailpit配置、容器网络区别和MAIL_URL优先级。auth-mail-docs-final.log三语站构建及两挂载／桌面手机12组正文浏览器全部通过。

边界：请求成功只表示入队；SMTP受理后、队列确认前崩溃仍可能重复邮件，不宣称恰好一次，重置单用与验证幂等分别保证。真实SMTP账号、生产TLS、外部投递及手机／邮箱最终收信仍属外部发布验收。本项未闭合A01整体，后续仍需数据库设备列表、头像／DeleteUser可靠清理、Passkey／第三方登录及完整因素页面、S10自定义用户与安装矩阵；复核还须将旧DisableVerificationMail事件策略限定到套件用户，不能以当前队列类型隔离代替全局事件范围核对。清单及最终SHA见auth-mail-files.json／auth-mail-final-source-sha.json，清理与剩余空间见auth-mail-delivery-space.json。未推送。


## A01-02／05：验证邮件策略的模型隔离（本子单元验收通过）

修复上节复核登记的DisableVerificationMail范围问题：不再以全局NotificationSending事件按VerifyEmail字符串拦截所有用户。复用所属应用的认证邮件回调表，增加VerifyEmail::send_when::<T>类型条件，在生成URL、渲染或入队前执行；其他类型默认保留原生行为。错误直接向外返回，回调在释放注册表读锁后调用。套件configure_mail_queue无论同步或队列模式均按实际用户类型注册验证开关；关闭验证但使用包装能力时明确拒绝并要求手动接线，避免策略静默失效。移除旧事件监听器及其文件，默认Provider调用顺序与路由保持不变。

verification-policy-first／resumed／second／third保留初次路径、trait导入及无From夹具失败，不计通过。修正本任务测试夹具后，verification-policy-final-gates首项通过但后续触32GB守卫退出75，不把中断计为完成；守卫因其他构建仍活跃而拒绝删除占用产物，恢复空间后继续。verification-policy-gates-resumed.log启用／未启用Sanctum各1项模型与App隔离、队列回归2项及workspace／feature严格Clippy／fmt最终退出0。不同模型使用独立验证URL回调完成原生邮件投递，当前套件被禁止类型零发送；同一类型在另一个应用正常发送，条件错误不增加发送记录。

随后吸收已提交公共依赖到be4cf61a0，保留其他代理未提交改动与下一批设备源码在工作区，实际组合见verification-policy-combination-head.txt。verification-policy-combination.log独立MySQL8.4.13和PostgreSQL18.7各执行认证队列2项及隔离1项，完整Sanctum套件50项，workspace严格Clippy1分08秒、feature37.32秒及fmt通过。无环境提前返回不计外部数据库实测。前批Redis队列2项未受存储实现变化，复用既有证据，不声称本批重跑Redis。

verification-policy-docs.log三语正文构建及两挂载／桌面手机12组通过，说明send_when公开调用及非全局阻断。交付清单／源码SHA见verification-policy-files.json／verification-policy-final-source-sha.json。清理仅本批文档dist与缓存，真实剩余空间见verification-policy-delivery-space.json；可复用Cargo缓存、源码、日志、数据库与数据卷保留。此项只闭合邮件策略范围，继续数据库设备管理、头像与注销、可选因素及安装矩阵，不闭合A01整体。未推送。


## A01-03前置：数据库会话的只读元数据与归属核验（底层子单元验收通过）

参考[Jetstream5.x会话表读取](https://github.com/laravel/jetstream/blob/5.x/src/Http/Livewire/LogoutOtherBrowserSessionsForm.php)的已有user_id／IP／UA／last_activity结构，并核对固定[Framework13.40.0 SessionGuard](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/SessionGuard.php)。Jetstream仅作设计参考，行为基线仍为项目固定框架。沿用原生DatabaseSessionHandler、SessionManager、EncryptedStore、Store和SessionFingerprint，不另建解密或JSON会话解析器。

新增DatabaseSessionRecord（不实现自动序列化）及DatabaseSessionHandler::sessions_for：按user_id读取未过期候选元数据，原生写连接、秒单位、最近活动排序；user_id不是完整归属证明。SessionManager::database_handler只对原生database驱动提供能力，同名自定义驱动明确拒绝。read_database_session使用独立原生Store解密／加载，不保存、不改当前请求、不刷新活动或GC；非法ID报错，缺失／过期会话没有登录属性。SessionGuard::session_belongs_to_user要求实际Guard登录身份和当前安全指纹，拒绝无指纹、伪造、旧版本、其他Guard；不修改原生登录或记住登录流程。

device-primitives-first／second／third保存初次测试夹具缺用户表、insert调用及语法失败，均未计通过。改用独立原生database UserProvider和具名用户表，让真实Guard登录时重新读取用户行，而非绕过登录指纹复核；device-primitives-fourth.log SQLite明文／加密2项通过。首轮MySQL一项并发建表失败，夹具从时间戳命名改成进程号+单调计数，保持正常并发；device-primitives-matrix-second.log独立MySQL8.4.13、PostgreSQL18.7各2项通过，不使用开发数据库。覆盖实际metadata、正确／错误归属、其他Guard、无／伪造／旧版本指纹、非法／缺失／过期ID、读取前后完整行不变、同名自定义驱动拒绝。schema沿用正式字段类型，表按用例独立并清理，未以扩大列类型掩盖方言问题。

device-primitives-final-gates.log SQLite2通过，严格Clippy发现测试显式函数指针类型复杂，改为现有Builder的分支配置形式，不加allow。device-primitives-gates-resumed.log SQLite2再次通过，workspace严格Clippy10.90秒、Fortify+Sanctum49.99秒及fmt退出0；冻结组合头见device-primitives-combination-head.txt，未含其他任务WIP。device-primitives-docs.log三语Session正文构建及两挂载／桌面手机12组通过，说明可信服务端使用、候选归属与敏感原始ID边界。

本项不是设备列表／撤销功能交付。公开设备响应仍需不透明ID、授权、近期证明与持久化撤销；删除sessions行不能防止在途保存复活。多Guard共用浏览器时单列user_id可能遗漏另一Guard会话，正式设备能力需处理该边界，不能仅用这批候选读取宣称完整枚举。后续方案／待验证条件保存在target/tmp/user-starter/device-design.md，未计完成。清单／SHA及真实清理空间分别为device-primitives-files.json、device-primitives-final-source-sha.json、device-primitives-delivery-space.json；本批只删除文档dist／缓存，保留正常Cargo增量、源码、日志、数据库／数据卷。继续设备生命周期及其余A01，不推送。


## A01前置复核：作用域栈占用与SQL异步类型边界（本子单元验收通过）

新增Guard校验组合验收时，既有认证邮件Worker用例栈溢出；session-validator-baseline-probe.log移除新增Guard功能后，已提交SDK组合仍在同一路径SIGABRT，因此不归因为未验收的Guard钩子。阶段探针确认发生在真实Artisan Worker，macOS崩溃符号与ARM64函数序言分析见queue-stack-crash-frames.json／queue-stack-frame-sizes.json。按序言估算，私有Facade隔离作用域单帧319,472字节、Container隔离182,656字节，两层TaskLocal poll各约91KB；嵌套泛型业务Future被多层内联。用例本身Future12,984字节，单独装箱测试或Worker外层、Guard解析外层均不能解决，试验改动全部撤销，没有提高线程栈或编译递归限制。

复用既有Facade::set_facade_application的固定布局方式：仅在公开Container::scope和私有Facade::isolated_scope进入async状态机前装箱业务Future。保留原生嵌套共享、独立隔离、取消关闭和清理顺序。起初更广的五层装箱缩减为这两个必要入口；其他作用域不改。Scope公开调用仍为scope(future).await，并保留非Send的本地Future支持。

严格检查还发现，恢复原始作用域的已提交组合也有Fortify三个消费者布局查询深度130溢出，见observation-clippy-baseline.log，不将编译器建议增加recursion_limit作为修复。为SQL执行新增私有Execution::run_boxed，只在既有observed_query宏的执行边界擦除具体驱动Future类型，复用原来的run逻辑与结束／取消收尾。保留其他观测路径的泛型／非Send契约，未全局增加Send约束；没有夹带其他协作者的Worker／Console在途改动。该SQL接缝属于原生DatabaseFuture已保证Send的边界。

scope-sql-boundary-gates.log：Rust1.94工作区严格Clippy1分00秒、Fortify+Sanctum1分03秒通过；门面生命周期8、新布局／输出／未poll取消／非Send支持3、原容器scoped14、完整用户套件50与fmt全部通过。原始邮件模板未改，默认测试线程栈下两个邮件队列用例已恢复。scope-sql-matrix.log独立MySQL8.4.13／PostgreSQL18.7各2项数据库队列及会话，Redis8.10.3队列／会话2项、file会话+SQLite数据库队列2项全部通过；本批专属容器已回收。该矩阵不代表生产SMTP或供应商投递验收。

scope-sql-observer-final.log进一步通过真实SQLite查询的观察者用例1：成功、失败、in_scope实际poll，及独占事务耗尽单连接池时取消待执行查询，只收尾一次Cancelled；事务随后正常回滚。追加测试后workspace严格Clippy1.79秒、feature0.24秒及fmt再次通过。既有成功／错误消息与数据库行为沿用原机制，无需新增开发者配置或迁移。

源码清单／SHA／组合头为scope-stack-files.json、scope-stack-final-source-sha.json、scope-stack-combination-head.txt；只交付已验代码。独立Blade深层渲染仍有默认栈问题，新增Guard校验、持久化设备功能及其他A01剩余项继续处理，未混入本项或计为闭合。空间记录scope-stack-delivery-space.json；本批没有需废弃的专用构建profile，按当前规范保留可复用增量缓存、源码、日志、数据库及数据卷。未推送。

## A01前置复核：Blade递归渲染栈占用（本子单元验收通过）

原始渲染器将全部42种节点的临时值集中在同一递归帧；ARM64调试构建的序言分配约65,344字节，40层合法条件模板在默认测试线程栈上SIGABRT，证据view-stack-baseline-resumed.log／view-stack-baseline-crash-frames.json／view-render-stack-prologue-before.txt。没有增大RUST_MIN_STACK或放宽原生64层解析限制。按原节点职责拆分输出、控制流、布局、表单、条件及组件的私有执行函数，保留Native节点的Cow作用域、循环跳转、绑定与错误传播；不改变公开模板契约。

view-stack-renderers.log实际通过37项：新增深度测试覆盖1／16／40／64层与65层编译拒绝，及既有Blade应用、语句、表达式、翻译、原生值消费者。view-stack-final.log曾发现邮件夹具仍断言旧的单层错误文本，而已提交Worker使用attempt_exception_message完整原因链；只更新该旧断言，继续明确验证收件地址不泄露，没有修改运行时错误。view-stack-gates-resumed.log实际通过邮件33项（原2项外部测试仍ignore，不计通过）、完整用户套件50项、工作区严格Clippy及Fortify+Sanctum严格Clippy、fmt。没有重复宣称数据库矩阵或真实SMTP厂商验收。

首次后续门禁因32GB磁盘守卫安全中断，view-stack-gates.log记载清理锁繁忙E35，不计通过；空间回升后重新执行成功。本批交付空间记录view-stack-delivery-space.json，没有另删共享／活跃编译缓存，源码、日志、数据库及数据卷保留。清单与最终SHA见view-stack-files.json和view-stack-final-source-sha.json；新Guard策略、事件上下文及设备管理仍在途，不计入本提交或整簇完成。未推送。

## A01-03前置：原生会话身份策略（底层子单元验收通过）

AuthManager及Auth门面新增add_session_validator／remove_session_validator；具名异步回调接收实际Guard、Authenticatable与只读Store。同名替换保持注册顺序，按注册顺序短路AND；回调在锁外await，错误不降级放行。只使用创建Guard的所属应用管理器，不依赖当前全局门面；原生SessionGuard::new保持不带应用注册表。正常身份解析先核原有指纹，再检查策略；remember恢复在ID轮换前核验原会话。false沿既有拒绝路径清理会话状态、派发CurrentDeviceLogout并返回401；错误传播。已执行请求不追溯取消，原生请求内身份缓存不变。

session-validator-final-gates.log实际通过明文／加密SQLite会话2项，原生HTTP会话目标8项中新增真实HTTP用例验证拒绝、错误500、remember原ID拒绝／允许后原生轮换、第二应用不泄漏；其中原有MySQL入口无连接时提前返回，不将其算真实MySQL证据。完整套件50项、工作区与Fortify+Sanctum严格Clippy通过；格式检查沿冻结源码完成。新增回调排序、同名替换、移除幂等、错误及空名称行为也在session_devices的2项中实际执行。

session-validator-final-matrix.log通过独立MySQL8.4.13和PostgreSQL18.7各2项明文／加密真实会话与原生Guard解析；不是模拟驱动，夹具各自移除本批容器。session-validator-docs.log三语站点构建与12组浏览器检查通过（两挂载、桌面／移动），新锚点session-identity-policies可见、公开方法文本完整、无页面异常及横向溢出。三语session正文说明原生条件、应用隔离、remember及低层构造边界。

本项只是持久化设备撤销的原生接缝；HTTP测试的撤销集合属于测试内存状态，不冒充持久化设备API完成。事件上下文、设备索引与端点、头像、注销和其他A01缺口继续实现。源码SHA与清单见session-validator-final-source-sha.json／session-validator-files.json；空间和本批文档产物回收见session-validator-delivery-space.json。源码、日志、数据库、数据卷及可复用Cargo缓存保留。未推送。

## A01-03 浏览器设备：持久化撤销、原生中间件及网页

本单元沿S02／S03实现，仍不把A01-03或整个套件标为闭合。复核固定Framework的Router::pushMiddlewareToGroup职责与既有Jetstream数据库会话边界；旧openrust-v0.x的SessionGuard／guard_session测试提供remember、轮换与隔离参考，没有可直接移植的本人设备索引或持久化撤销端点。复用现有SessionManager／DatabaseSessionHandler、原生Guard、凭据事务锁、Sanctum及近期确认，不扫描缓存键或另建Session驱动。

新增Route／Router::push_middleware_to_group正式注册入口，去重且保留默认组、优先级、排除和元数据。TrackBrowserSessions<User>在web／api的原生Session之后检查并登记最终响应ID，覆盖注册后再次regenerate和remember恢复。既有普通请求只条件更新未撤销索引，不能重新插入；轮换在共同用户锁内复核来源和共享remember token。索引按解析后的数据库、表、模型、真实主键、Guard隔离。原ID加密保存，公开ID为不透明哈希；列表加载实际会话并核对原生身份指纹，不能把sessions.user_id候选元数据当完整归属。

GET／DELETE /api/user/sessions及DELETE单项接通权限、Cookie CSRF／Token用途证明和事务内复核。删除本人会话保留独立PAT；PAT没有当前浏览器，删除其他即删除全部浏览器。撤销共享remember token会使各浏览器旧remember失效，其他有效活动会话保留。自定义会话策略拒绝现只清除选中Guard，保留购物车及独立Guard；原生密码指纹失败仍沿既有全会话拒绝行为。已取得Cookie近期证明但尚未进入用户锁的PAT派发，在设备撤销后不得继续创建令牌。

full预设新增/user/sessions及原生DELETE表单、近期确认／本地化闪存；api预设不发布该账户页。原生database能力缺失时明确拒绝；file、Redis、array和替换database处理器须显式关闭user.devices.enabled。新config/user.rs读取USER_DEVICES；保留的旧配置需手动合并，三语已说明。新增索引迁移模板000023，实际包发布按当前时间戳命名，保留原始users迁移。auth:prune-browser-sessions --hours=24（或0）只清理到期索引，幂等，不删除用户／PAT／外部会话数据。

实际证据统一位于target/tmp/user-starter：

- browser-devices-gates-second.log：完整57项UI、13安装、6配置通过；工作区及Fortify+Sanctum两组严格Clippy、格式通过。
- browser-devices-capability-fourth.log：SQLite明文／加密各7项；browser-devices-final-matrix.log：MySQL8.4.13、PostgreSQL18.7明文／加密各7项，共28项。包含旧Session整行写回仍拒绝、remember拒绝、他人／独立Guard、已认领证明与撤销竞争、原生HTML／CSRF、到期清理和禁用能力预检。
- browser-devices-runtime-browser-second.log及runtime.json：真实Artisan init full、已安装资源、MySQL加密Session，23项Artisan／HTTP检查，包含重复迁移、回滚重建、两个真实应用进程、重启、跨节点确认及撤销。六组三语桌面／移动真实浏览器完成注册、remember登录、原生表单撤销，目标401而当前浏览器继续有效。首轮路径断言错误（误用模板编号）及浏览器请求缺Origin已修正验收脚本；未修改运行时以放行。
- browser-devices-docs.log：三语站构建、36组两挂载／桌面移动页面检查通过，无页面异常／横向溢出。迁移时间戳说明与旧配置合并指引补于相同章节。

设备新功能没有冒充Laravel核心内置端点；属于已确认套件扩展。APP_KEY轮换的本单元跨节点运行尚未专项重跑（既有Session解密证据仅复用），自定义Provider／主键及安装全面矩阵继续由S10收口；头像、注销、个人令牌网页和可选因素仍待后续单元。真实外部供应商／物理设备／生产TLS仍独立发布验收。源码清单／SHA、最终兼容回归、空间清理结果另见browser-devices-files.json、browser-devices-final-source-sha.json、browser-devices-core-final.log及browser-devices-delivery-space.json；保留记录、源码、数据库和可复用Cargo缓存，不推送。

最终兼容补充：browser-devices-core-final.log中的无Sanctum套件12项实际通过；其后追加的核心目标在编译阶段触32GB守卫中断，不能算本次追加回归通过。复用未再次修改的核心源码在browser-devices-ninth.log中的路由组1项、原生会话8项、会话设备2项，以及sixth.log的组排除证据；后续工作遵循守卫清理结果。最终文档新增迁移发布名称说明为事实修正，36组页面证据对应此前相同章节结构。

## A01-03／04 个人令牌网页

沿计划已引用的Jetstream API管理边界（https://jetstream.laravel.com/features/api.html ，本轮重新核对5.x官方概念页）补齐full预设/user/api-tokens及本人账户导航，不引入Livewire／前端依赖。该页面为用户套件扩展，不宣称Laravel Framework内置相同路由；固定Fortify／Sanctum原生契约不改变。旧版没有可直接复用的同契约令牌网页，复用本仓已验收账户签发、哈希、期限、近期证明、Eloquent生命周期与撤销事务。

account::tokens/create_token/revoke_token仍保留原公开API签名和返回，内部提取同一摘要／签发／撤销核心，网页仅通过私有浏览器模式选择原生web Guard及claim_session，不能以客户端参数切换身份校验。没有伪造TransientToken或解析HTTP正文绕行。事务继续核对本人模型、真实当前凭据、能力、证明和设备撤销点；Token API原有默认选择能力保持不变。网页要求显式勾选能力，空选择及越权失败；服务端白名单为空时隐藏新建表单，仍允许撤销已有令牌。

GET页面仅含安全元数据。POST原生表单创建后201展示一次明文，不放闪存／Cookie／URL；nativeSession添加private后仍包含no-store，页面另有no-referrer。后续GET没有明文。DELETE通过标准表单方法覆盖并保留CSRF及近期确认，事务核对归属后302回页和三语状态。API-only不发布页面；三语模板、教程和安装资源同步。设备HTML测试的form_request抽为共同测试助手，仅复用原传输，不变其断言。

实际证据target/tmp/user-starter：

- token-web-third.log首先3项通过，随后新增空白名单用例。token-web-final-gates-second.log最终61项套件、13安装、6配置通过；workspace及Fortify+Sanctum两组严格Clippy、fmt通过。首次新测试缺Facade trait导入已修正，失败不计通过。
- token-web-matrix-build.log独立MySQL8.4.13及PostgreSQL18.7原生database会话各4项通过，包括实际表单、一次性明文、不保存秘密、CSRF、跨用户、空／非法能力、空白名单撤销、API-only404。SQLite相同4项含在61项里。探针使用夹具明确发布的/api/token-me，原生web探针可能发送SessionCookie，不借用纯API无Cookie断言冒充；实际安装应用由后项验证真正/api/user。
- token-web-runtime-second.log及token-web-app-browser.json：真实init full首次／重复安装、真实MySQL加密会话、三语桌面／移动六组Chrome。每组原生注册／密码确认／创建表单、显式能力、明文只显示一次、独立无Cookie请求上下文Bearer访问/api/user成功、原生表单撤销后独立Bearer401、当前浏览器保留。首轮验收将no-store错误限制为单值，未允许原生追加private，已仅修正断言为Cache-Control指令集合；运行时未放宽。
- token-web-docs-final.log：三语站点构建及12组两挂载／桌面移动检查通过，token-management-page可见、正文与公开路径完整、无页面异常／横向溢出。

本批仅闭合个人令牌网页的上述入口，不把整个A01-03／04计完成。无Sanctum实际HTTP在本批仅复用前单元12项及本批默认feature编译证据，最终S10组合仍须复核；头像、可恢复注销、可选因素及完整安装矩阵继续。生产账号／设备验收独立跟踪。显式清单／SHA及空间记录为token-web-files.json、token-web-final-source-sha.json、token-web-delivery-space.json；保留源码、证据、数据和正常复用缓存，未推送。

## A01-03／05 用户资源清理前置

复用主线已提交def7278d7的QueueManager::dispatch_in_database_transaction，不重建outbox／轮询器／租约。旧版同名foundation::ResourceCleanup负责进程内资源作用域结束，不是用户文件清理，不能因同名而移植；未找到旧版同契约DeleteUser／profile_photo流程。本单元提供已安装套件的ResourceCleanup服务、000024账本迁移、专用任务及auth:prune-user-cleanups，头像与注销HTTP动作尚未消费，仍不能标记A01-03闭合。

清理账本和原生数据库队列在同一用户事务写入，专用user.cleanup.connection默认database／queue默认user-resources，不改变全应用默认队列（其他任务仍可用Redis）。严格派发拒绝同步、Redis、自定义替换或其他连接冒充同库原子。保存随机收据摘要，原文只交付一次；队列仅含操作ID、连接名称和范围摘要，Worker读取自己的数据库／磁盘配置。收据七天有效，仅返回有限pending／failed／completed及文件数，不依赖用户仍存在；文件路径不能作为公开状态返回。只接受profile-photos目录内独占随机文件名，禁止重新使用路径。所有账本读取显式use_write_pdo，避免副本延迟误判任务不存在；本批没有配置真实只读副本端点，不冒称副本专项实测。

Native Worker删除幂等，完成后重复任务为no-op；文件失败由原生重试／失败记录恢复，failed回调不覆盖已完成状态。只有completed且收据已到期的记录可被prune移除，pending／failed继续保留，命令不删除外部文件或用户。独立应用共用jobs表需具名队列隔离；同一应用节点须共享存储和数据库配置。生成config/user.rs包含USER_CLEANUP_CONNECTION／USER_CLEANUP_QUEUE默认值，既有配置需手动合并。

实际证据target/tmp/user-starter：

- resource-cleanup-final-regression.log：64项套件、13项安装、6项配置通过及fmt。resource-cleanup-delivery-gates.log：新增3项、workspace与Fortify+Sanctum两组严格Clippy通过。
- resource-cleanup-delivery-gates.log的SQLite／MySQL8.4.13／PostgreSQL18.7各3项实跑，包含实际迁移模板经公开Migrator首次、重复、回滚、重建（不是镜像Schema），原生Artisan::call(queue:work)实际消费、账本与jobs一起回滚、凭据／路径不进队列、错误收据拒绝、重复派发、已删除User后失败收据仍可查、恢复后重投完成，以及原生prune命令。Worker为真实原生消费但在测试进程内，尚不是新进程强制终止验收；提交前后强杀将在头像／注销最终消费者中继续。
- MySQL曾真实复现1059：长DB_PREFIX使自动owner_hash索引名称超过64字符。resource-cleanup-mysql-diagnostic.log保留原错；仅迁移使用Blueprint::get_prefix()配合短显式索引名，保留PostgreSQL前缀隔离，不修改全局Schema命名。native Migrator测试首轮传入相对过滤路径而筛掉全部迁移，已改为根目录下的绝对路径；上述最终矩阵使用修正后测试。
- resource-cleanup-docs.log：三语站点构建及12组两挂载／桌面移动页面通过，无页面异常或横向溢出，说明后台Worker、升级配置、范围及尚未交付的消费者。

冻结组合基于38285b7c1及此前已提交主线83个路径更新，未吸收Root在途Pulse／Webhooks／Cargo.lock。实际通过不代表其他协作者未提交功能已通过。源码清单／SHA及清理空间见resource-cleanup-files.json、resource-cleanup-final-source-sha.json、resource-cleanup-delivery-space.json。只清理本批文档输出，保留记录、源码、数据库／数据卷和可复用Cargo缓存，不推送。

## S09前置：图片值对象的单次限额

新增Image::with_limits，复用所属ImageManager／驱动注册、转换和Semaphore，只取当前与请求上限的较小者，不建立第二套头像解码器。输入／渲染／主色缓存会在收紧后重新校验，限制在原生解码与缩放前生效；不能用缩略图输出绕过原图限额。已渲染对象仍按既有流水线从结果字节建立下一阶段。此为Rust原生值对象扩展，不宣称固定Laravel Image门面存在同名方法。旧版没有对应ImageManager／同契约图片流水线；复用当前Limits、NativeDriver及既有错误类型。

image-limits-second.log：4项公开Image值对象测试通过，覆盖分支隔离、缩放前拒绝、缓存重新校验、不能放宽全局／先前限制、保留具名驱动及流水线选择；既有reads_exif_orientation_and_checks_inputs_and_limits专项1项通过。workspace和Fortify+Sanctum严格Clippy通过，最终格式检查随冻结源码完成。未重跑未受影响的全部编码器矩阵，也未把此项当作头像HTTP操作验收。首轮测试Probe缺少Driver必需的dominant_color实现已补齐，仅测试夹具变化；首次失败不计通过。

image-limits-docs.log三语构建及12组两挂载／桌面移动页面检查通过。应用可通过正式Image门面取得值对象后链式调用with_limits，示例区分HTTP请求体／类型校验和图片解码限制。头像控制器的门面消费、真实上传及故障补偿接续实现。清单／SHA和本批文档清理空间见target/tmp/user-starter/image-limits-files.json、image-limits-final-source-sha.json、image-limits-delivery-space.json；保留所有源码／验收证据及正常复用缓存，不推送。


## S09 头像网页、API与持久化上传预留

本单元沿用正式S09、既有Image／Validator／Storage／Url／原生事务数据库队列。旧版未发现同契约的头像附着／可靠回收流程，不将进程内ResourceCleanup移植为用户文件清理；不新建文件服务器、工作流、轮询器或第二套图片解码器。

- 显式安装发布000025头像列迁移，跟随应用User连接；已有任一头像列时在DDL前拒绝，不修改原始users迁移。完整预设/home增加原生multipart与DELETE表单；两种预设提供按API前缀配置的POST／DELETE本人头像。需要account:update及近期确认，PAT使用avatar用途单次证明；Cookie保留原会话、CSRF及近期确认，不能误用凭据变更同步导致反复确认。
- 实际解码JPEG／PNG／WebP，默认2048 KiB及每边4096像素，同时遵循Image更严格的全局限制。服务端随机64位名称，不信任原文件名、扩展或MIME。明确记录原磁盘和路径，默认仅公开磁盘；使用原生static_disk为同源本地URL服务，多节点／Worker共享FILESYSTEM_PUBLIC_ROOT或对象存储。默认资源增加可空avatar_url，不暴露路径／磁盘；关闭头像不解析历史元数据或解析存储服务。单独不含Sanctum的Fortify响应仍为原四字段。
- 写入文件前独立提交15分钟延迟预留，附着引用与pending→completed在用户事务内原子提交。到期回收和附着竞争同一状态更新；已附着文件不被迟到任务删除。取消先尽快补偿，并保留processing状态的最终回收以覆盖迟到写入。失败／超时与模型事件拒绝保留旧引用。旧文件清理任务与新引用同事务入队，读取真正锁定后的旧引用；并发替换不删除最终头像。I/O使用120秒请求上限，非任意无限期自定义存储写入的完成保证。
- 首次上传／无旧图移除返回200；有旧图待清理返回202及meta.cleanup收据。GET /api/auth/resource-cleanups/{operation}独立于Session／认证组，仅通过X-Operation-Receipt查询pending／failed／completed及文件数，不要求User存在。缺失／错误／过期收据404，query收据400；no-store／no-referrer。只使用IP命名限流，不在查询时启动Cookie会话。Telescope默认隐藏该标头和meta.cleanup.receipt，并验证实际请求／响应记录不留明文。其他认证字段的最终观测脱敏仍属S10安全组合复核，不据此声称所有自定义响应字段自动安全。

### 实际证据与边界

冻结组合基于15c9c1248c151d97269b265e3fa73a405cd7e366及本批覆盖，只吸收已提交主线122份实际变化，不拷入Webhooks／Horizon／Cargo.lock在途内容。源码清单／SHA为target/tmp/user-starter/avatar-files-draft.json、avatar-final-source-sha.json；应用安装产物SHA为avatar-installed-files-sha.json。后续独立主线提交不混入本单元验收结论。

- avatar-delivery-consumers.log：starter_responses10项、starter_ui76项通过。avatar-install-checks.log：安装13项及配置6项通过。覆盖原生网页与Cookie／PAT、CSRF、近期确认、真实解码含有效签名但截断内容、伪造类型、尺寸超限、路径穿越文件名、模型取消、存储写入失败、并发替换、保持当前引用、空／多级API前缀下HTML Accept仍为JSON错误、关闭头像的旧元数据投影，以及预留提交／回滚／迟写补偿／到期竞争。
- avatar-mysql-delivery.log／avatar-postgres-delivery.log：MySQL8.4.13与PostgreSQL18.7各12项真实HTTP／数据库／原生Artisan Worker通过，数据库会话；含正式迁移首次、重复、回滚及重建。SQLite覆盖同一12项并在76项组合中通过。Telescope公开请求观测专项在avatar-combination-observability.log通过，确认标头与响应收据被掩码且操作ID保留。
- avatar-runtime-delivery.log：尚未安装用户套件的基线应用骨架经真实Artisan init full、重复安装与自动重建／迁移，通过双应用进程共享MySQL与磁盘、B节点读取A节点图片、202收据查询和真实新进程Worker。实际SIGKILL应用发生在写文件后、引用提交前，原用户引用不变，强制推进独立夹具的到期时间后由新Worker回收孤立图片。实际SIGKILL Worker发生在外部文件删除后、队列确认前，过期保留任务由新Worker重投并结算。SQL门闩只在独立MySQL夹具上使用；MySQL可继续完成客户端死亡前已受理的autocommit更新，实测账本completed而队列仍reserved，这是允许状态，新Worker只结算，不假装服务器已提交语句回滚或承诺外部I/O exactly-once。
- avatar-app-browser.json：真实Chrome三语桌面／375px移动6组通过，原生上传／移除、图片实际加载2×2像素、无横向溢出。头像／清理三语教程在avatar-docs-final.log通过构建及24组双挂载浏览器检查；最后补充的资源字段文字／JSON经avatar-doc-json-check.json逐块解析，不将文字修订算新增运行行为。
- avatar-delivery-workspace-clippy.log及avatar-delivery-feature-clippy.log：Rust1.94.0 workspace all-targets、Fortify+Sanctum严格Clippy通过；avatar-delivery-fmt.log为空且退出0。前次组合Clippy曾被32GB守卫中断、停止时报Operation not permitted，不计通过；空间恢复后重跑上述最终门禁通过，不用扩大栈或关闭警告规避。

初期的测试夹具触发器语法、浏览器重定向目标以及冗余结构体更新等失败日志保留，均不计通过。生产对象存储ACL／CDN、真实云账号及Windows／Linux完整应用组合属于后续S10／发布环境验收；本单元不冒充其通过。S10还须把头像磁盘严格类型、权限／URL、同库队列和关闭开关纳入安装预检。

avatar-delivery-space.json记录直接删除本批文档dist／cache及独占模拟图片，共77,800,029字节文件；观测可用空间由56,054,763,520增至56,136,200,192字节（并行磁盘活动不全部归为本批释放）。正常增量缓存、源码、全部日志、依赖下载、数据库和数据卷保留；独立MySQL／PostgreSQL临时容器已由夹具退出收回。A01五阶段仍未整簇闭合，继续S04注销、S03可选因素及S10完整安装组合，不停在本单元。

### 后续S04参考固定

注销Action分工另外固定参考[Jetstream v5.5.3](https://github.com/laravel/jetstream/tree/61cac5cde455311890f6981fb2da47acd298e4e2)，SHA61cac5cde455311890f6981fb2da47acd298e4e2；实际读取DeletesUsers、DeleteUser、CurrentUserController、HasProfilePhoto和composer.json，快照与SHA在target/tmp/user-starter/reference/jetstream-5.5.3/。仅借鉴可替换应用Action及近期确认；不把Jetstream的先删外部头像再删用户顺序直接照搬为事务保证，也不引入团队或Inertia依赖。固定Laravel／Fortify／Sanctum基线保持原版本。

## S04前置：事务令牌物理清理与请求Guard缓存重置

沿用固定Sanctum模型与当前原生事务／Eloquent生命周期，新增TokenRelation::purge_in_transaction及PersonalAccessTokens::purge_owned_in_transaction。属于注销所需的Rust事务扩展，不冒称PHP有同名方法；原delete的全局Scope／批量删除行为保持。新入口要求同一连接、调用方持有用户锁并在错误后回滚，按真正主键、精确owner/type分批100锁定，包含Scope隐藏与软删除凭据，逐模型force_delete事件。模型拒绝、回滚及伪成功不物理删除均报错，不能无限循环或虚报完成。旧版无此事务凭据生命周期契约，复用现有模型适配器与事件。

AuthManager／Auth::forget_guards对应固定Laravel AuthManager::forgetGuards（固定SHA下src/Illuminate/Auth/AuthManager.php），清理当前请求已解析Guard缓存，保留选定Guard、配置和正在解析标记，不清Cookie、Session或CSRF、不代替注销／撤销。不同请求及应用隔离。已有内部reset_request用于完整分派边界，不能机械替代这个公开入口。

实际证据位于target/tmp/user-starter：token-purge-mysql-final.log、token-purge-postgres-final.log分别通过真实MySQL8.4.13／PostgreSQL18.7的软删除自定义模型专项和既有Bearer／SPA矩阵；token-purge-sqlite-final.log通过SQLite既有矩阵。各矩阵包括整数／UUID／ULID、scope隐藏、精确归属、错误连接、事件与取消回滚。sanctum_purge每库验证105个本人凭据跨批处理、软删除、模型伪成功和取消全部回滚、他人及大小写归属保留、重复清理0。token-purge-cache-final.log的3个缓存与2个既有扩展测试通过，覆盖HTTP正式门面、同请求刷新、选定Guard、跨请求／应用隔离、会话不变及解析中重入保护。

最终workspace all-targets及Fortify+Sanctum严格Clippy、fmt通过（token-purge-workspace-clippy.log／token-purge-feature-clippy.log／token-purge-fmt.log）；三语教程构建与24组双挂载桌面／移动页面检查在token-purge-docs-first.log通过。冻结源码沿用S09的15c9c1248基线与已验本批覆盖，其他在途Horizon／Webhook不混入。初期夹具失败均保留且不计通过。这是注销前置能力，未完成DeleteUser／Passkeys清理及最终安装组合，A01各阶段仍未整簇闭合。

## S04前置：通行密钥事务清理

Passkeys::purge_user_in_transaction复用当前注册的用户／密钥模型、原生Eloquent事务和force-delete事件，不另建凭据表或WebAuthn解码器。旧版无同契约事务清理；沿用本账本固定Passkeys／Fortify参考及前述凭据清理裁决。这是Rust注销事务扩展，不冒称PHP有同名接口。调用方持有用户锁，出错须回滚整个事务；注册类型、真实原始主键与用户连接严格匹配。可选表未安装返回0，已存在的跨连接表拒绝原子假设；包含Scope隐藏与软删除行，分批100处理，精确user_id、MySQL二进制比较，事件取消／物理删除伪成功均失败，不删除他人凭据。

passkey-purge-sqlite.log／passkey-purge-final-test.log通过SQLite专项及4个既有公开模型契约测试；passkey-purge-mysql.log、passkey-purge-postgres.log分别在MySQL8.4.13／PostgreSQL18.7通过相同105条跨批次、ULID密钥、软删除、模型取消回滚、伪成功、他人／大小写邻居保留及重复清理0，另覆盖未注册模型、主键改写和未安装表。最后一次测试夹具async签名整理后SQLite专项复验通过；数据库逻辑未变化。当前已覆盖字符串用户主键；整数／UUID用户及跨连接明确拒绝会随注销组合继续补验，不将未运行用例计入本单元。

workspace all-targets及Fortify+Sanctum严格Clippy、fmt在passkey-purge-*-delivery.log／passkey-purge-fmt.log通过。三语Fortify教程构建与12组双挂载桌面移动检查在passkey-purge-docs.log／passkey-purge-docs-browser.log通过。早期私有Builder误用及夹具主键、lint失败已修正，保留日志不计通过。源码组合沿用S09及已验覆盖，无其他协作者在途内容。仅完成注销前置，不宣称DeleteUser或A01整簇闭合。

## S04 本人注销、应用Action及独立进程恢复

沿用前述固定Jetstream5.5.3的DeletesUsers／DeleteUser分工，以及Laravel／Fortify／Sanctum、原生用户行锁、模型事务和资源队列。旧版没有同契约的可靠注销实现；不迁移其进程内回调或先删外部文件的顺序。公开starter::deletion::{DeletesUsers<M>,DeleteUser,delete_users_using}；安装发布app/actions/fortify/delete_user.rs及Provider选择。Action可预检业务依赖并在传入事务内删除；不擅自级联业务数据或远端身份。套件直接收集原头像，不另暴露无人使用的resources钩子，避免替换Action时意外绕过默认文件清理。

两预设提供DELETE /api/user，遵循api.prefix，需要account:update及delete用途近期证明；Cookie仍用CSRF和近期确认。完整预设提供DELETE /user／current-user.destroy与首页原生表单。用户写锁内重新核验身份和证明、预检、旋转版本／remember、物理清理全部PAT及已安装密钥、移除套件待用挑战、撤销设备索引，并将头像清理与用户删除原子提交。自定义Action伪成功且用户未删除即失败。最后再次检查原证明期限，不能用已经自行撤销的PAT／设备记录当作提交后的再次授权而误报失败。重置链接的原身份绑定阻止后续使用，旧重置记录继续由其仓储过期清理。

提交后逐个退出当前浏览器中同模型／主键／连接的原生Session Guard，清当前请求Guard缓存、轮换Session／CSRF，保留独立管理员与购物车；不按sessions.user_id删除整行。无文件API204／网页跳登录；有文件202和meta.cleanup，网页为no-store／no-referrer状态页。收据仅在响应／页面中交付，不写入URL、闪存或Cookie；刷新使用X-Operation-Receipt、credentials:omit、no-store和no-referrer，不依赖已删除User。既有RequestWatcher对HTML只记HTML Response、ViewWatcher仅记录数据键名，JSON收据沿用已验默认掩码。其他证明／令牌观测脱敏仍归S10，不能据此宣称全部已收口。

### 实际验收

- deletion-mysql-second.log与deletion-postgres-second.log：MySQL8.4.13／PostgreSQL18.7各10项真实HTTP／数据库测试通过。覆盖缺失／跨用途证明、全部令牌失效、Action拒绝／伪成功回滚、真实外键阻止删除且队列／账本／旧头像不变、并发证明仅一赢家、Action结束时过期回滚、多Guard／管理员／购物车保留、设备索引撤销、原生HTML与Cookie／CSRF、202收据及原生Artisan Worker。MySQL首轮夹具外键使用有符号列与users.id不匹配，改为unsigned_big_integer后两库通过；早期失败不计通过。
- deletion-ui-delivery.log：SQLite86项用户套件组合全部通过，包括上述10项。deletion-install-final.log：14项安装通过，含首次／重复／强制资源安装保留正式注销模块、原有API信息路由、自定义文件保护。初始Hook在Fortify模块发布前读取路径的实际缺陷已修复：用正式模块模板建立，普通Fortify不装套件不增声明，Fortify强制发布后幂等接回；真实应用安装与自动重建也验证通过。
- deletion-crash-evidence/deletion-runtime.json及deletion-runtime-third.log：真实新应用init full（含安装但明确关闭的Passkeys）、重复安装、自动重建／MySQL迁移，双独立应用共享加密数据库会话和磁盘。MySQL独立触发器＋连接GET_LOCK仅作为可观察门闩，实际SIGKILL应用发生在用户删除提交前；旧用户、两个PAT、密钥记录及照片保留，另一节点仍可认证。重新提交后两节点均拒绝旧PAT，关闭因素的历史密钥也已物理删除。实际SIGKILL Worker发生在删文件后、队列确认前；MySQL可能完成已受理的autocommit账本更新，实测completed且任务仍reserved，新Worker重投只结算并完成，未承诺SQL和文件exactly-once。原生存储失败／重试／幂等路径复用ResourceCleanup既有三库证据，不重跑未改变路径。
- 上述进程场景已通过后，Chrome脚本因未传Playwright环境变量退出；不把整次脚本退出计通过。保留其独立进程证据，修正环境后仅跑缺少部分，deletion-runtime-browser.log和deletion-app-browser.json：三语桌面／375px移动六组真实Chrome通过，原生注册、近期确认、上传、注销202、收据状态刷新、无Cookie／Referer、返回登录及无脚本错误／横向溢出。新进程Worker清理六个浏览器账户的照片，所有注销任务completed。初始HTTP IP地址被原生Passkeys安装预检拒绝，测试改为HTTP localhost后符合既有开发规则，不修改生产HTTPS要求。
- deletion-docs.log／deletion-docs-browser.log：三语教程构建和12组双挂载／桌面移动章节检查通过。deletion-workspace-clippy-final.log、deletion-feature-clippy-final.log及deletion-fmt.log：workspace all-targets、Fortify+Sanctum严格Clippy与格式通过。最后去掉无消费者resources钩子，直接收集同一头像，并合并嵌套条件，不改变上述默认Action可观察路径；86项HTTP组合已按最终源码重新通过。实际进程与浏览器结果复用对应未受影响路径，不将其扩大为所有自定义Action的运行证明。

冻结源码沿用15c9c1248及本工作流已验覆盖，不拷入Horizon／Webhooks等在途变化；清单／SHA见target/tmp/user-starter/deletion-files.json和deletion-final-source-sha.json。安装测试中一次32GB守卫中断不计通过，已持锁清理97,070,284,800字节，守卫当时可用129,130,803,200字节；后续重跑通过。最终文档dist／cache、独占模拟照片及已退出的通行密钥夹具残留清理计量另见deletion-delivery-space.json，保留源码、全部日志、下载、正常复用缓存及数据卷。

### 仍归后续组合的边界

不把该单元当作A01全部闭合。S10继续验证自定义User／字符串主键／连接和软删除应用策略、因素启停历史数据、严格头像磁盘配置与同库队列预检、完整与仅API安装及系统矩阵。完整网页预设的空API前缀可能与既有Fortify网页路径冲突，需要在安装预检明确裁决，不能把仅API空前缀测试当作完整预设通过。S03／A01-04继续无密码近期认证、可选因素管理及第三方绑定；真实供应商／设备／生产SMTP／TLS／存储ACL仍单列外部验收。

## A01-04前置：原生双因素Action事务入口

固定Fortify1.43.0的EnableTwoFactorAuthentication、ConfirmTwoFactorAuthentication、DisableTwoFactorAuthentication、GenerateNewRecoveryCodes四个Action已逐项读取（官方src/Actions，快照／URL／SHA在target/tmp/user-starter/reference/fortify-1.43.0-two-factor/）。同时核对固定TwoFactorAuthenticationController及routes，确认force仍取自request.boolean，未根据旧印象删除。旧版credentials/attach及challenge的CAS实现可作并发核验参考，但类型及JWT／Authority机制不符当前契约，不另移植一套因素引擎；复用当前四个原生Action、Provider、加密器、RecoveryCode、Model.save_in_transaction和事件。

四个Action新增call_in_transaction(user,transaction,...)，普通call委托同一实现但保留原有保存语义。事务入口即使无状态变更也先验证用户连接，模型取消保存明确失败，由调用方持有用户写锁、回滚整个事务并丢弃／刷新内存模型。密钥长度转换、force、生成8个恢复码、加密、确认日期、无效码错误包及事件执行顺序均保留。事件仍在保存后、调用方提交前派发，外部副作用由监听器使用既有提交后机制；验证码提供方可能已消费码，后续数据库失败须取新码重试。这是User Kit所需Rust事务扩展，不将上游Action强行改为隐含事务或新建工作流。

- two-factor-transaction-first.log：SQLite真实模型专项通过，覆盖四个Action的提交、启用／恢复码更新／停用回滚、验证码错误、同库要求（包括已启用的no-op）、模型取消及加密数据。
- two-factor-transaction-mysql.log：MySQL8.4.13相同事务专项通过，另运行原two_factor_model_actions_persist_encrypted_state_and_events_on_mysql实际模型／加密／回调／事件HTTP回归1项通过，未把缺环境的提前返回计为证据。two-factor-transaction-postgres.log：PostgreSQL18.7事务专项通过。
- two-factor-transaction-native-http.log：真实独立MySQL及TCP原生Fortify路由6项通过，含固定PHP挑战向量、管理／会话挑战、前缀／回调、错误加密及缺失用户、容器响应、CSRF与限流。
- two-factor-transaction-workspace-clippy.log的workspace all-targets严格Clippy通过。第一次Fortify+Sanctum检查被32GB守卫中断且停止报Operation not permitted，不计通过；核对本批无活跃Cargo、其他守卫已清理并恢复空间后，仅补跑未完成部分，two-factor-transaction-feature-clippy-resumed.log严格Clippy、two-factor-transaction-fmt.log通过。未重复已通过三库／HTTP路径。
- two-factor-transaction-docs.log／two-factor-transaction-docs-browser.log：三语教程构建、12组双挂载桌面／移动章节与无溢出检查通过。文件／SHA在two-factor-transaction-files.json、two-factor-transaction-final-source-sha.json；清理文档dist／cache计量在two-factor-transaction-delivery-space.json，保留所有证据、源码、下载、正常增量缓存及数据库／卷。

这里只闭合事务Action前置，不代表套件因素管理页面、API／近期确认或A01-04已完成。接续使用正式入口装配近期确认、Cookie／PAT、页面与API，不能让调用方从底层绕行冒充管理流程。

## 双因素管理网页与用户API（2026-11-25）

延续上一单元的原生2FA事务Action，未另建TOTP或恢复码引擎。套件为显式注册的用户模型装配共同用户写锁，在同一连接内验证近期证明、PAT归属／能力、凭据快照及模型保存；普通Fortify未装配套件时保留既有响应。固定基线再次核对Fortify1.43.0的TwoFactorAuthenticationController及TwoFactorAuthenticatable：保留force参数与密钥“非null”启用语义，不用PHPempty规则替代。

完整预设新增`user.two-factor`页面、账户页入口、三语文字及正式发布映射。原生启用／确认／关闭／恢复码表单继续使用CSRF、具名路由与可替换响应契约。设置二维码／密钥及八个恢复码仅在近期确认后显示；因素状态改变使旧通用确认失效，须按提示重新确认。非秘密Session设置标记仅控制展示，不代替认证证明；设置详情可隐藏，原生未要求TOTP确认的配置仍可展示新绑定详情。

新增八个跟随API_PREFIX的用户API：读取安全状态、启用、确认、关闭、读取二维码／设置密钥／恢复码，以及生成新恢复码。安全状态要求account:read；其他操作要求account:update与factors用途的单次证明，或当前Cookie会话的近期确认。证明放请求头或JSON正文；所有近期证明消费统一拒绝URL中的proof，返回本地化400，包括Cookie分支。API-only不注册管理页面；相同路径保留原生POST时GET按HTTP契约返回405，不假装存在页面。

### 最终源码验收

- `two-factor-management-regression-final.log`：SQLite3.54.1套件90项、安装14项、原生2FA路由6项全部通过。新增4项包括完整API启用／确认／TOTP登录挑战／恢复码轮换／关闭、证明重放／错误码／URL拒绝／权限／关闭功能、原生HTML表单及旧确认失效、API-only多级前缀和无账户页面。
- `two-factor-management-mysql-final.log`与`two-factor-management-postgres-final.log`：相同4项分别在MySQL8.4.13、PostgreSQL18.7及真实database Session驱动全部通过，均经回环TCP。测试数据库和容器由各自验收器独立创建及清理。
- `factor-management-runtime-first.log`、`factor-management-runtime-install.log`和`factor-management-runtime-repeat-install.log`：真实Artisan完整套件首次／重复安装、自动重建和MySQL迁移成功。`factor-management-app-browser.json`记录本机实际Chrome的简体／繁体／英文、桌面／手机共6组原生表单流程，覆盖近期密码确认、二维码和密钥展示、真实时间TOTP确认、敏感详情隐藏、读取与重新生成恢复码、语言和无横向溢出。真实硬件／生物识别不属于本2FA浏览器验收。
- `two-factor-management-clippy-final.log`及`two-factor-management-workspace-clippy-final.log`：Fortify+Sanctum所有目标和冻结workspace所有目标严格Clippy均通过；`two-factor-management-fmt.log`完整格式检查通过。冻结源码的基线与前序单元一致，本记录不声称其他对话后续未提交功能已验收。
- 三语教程补齐完整端点表、Cookie／PAT证明边界及curl示例；`two-factor-management-docs.log`文档站构建通过；`two-factor-management-docs-browser.log`覆盖两个部署基路径、三语及桌面／手机12组文章DOM验收通过。

早期夹具失败及中断日志保留：首次错误工作目录的构建已主动安全取消，不计最终证据；早期两次编译交错未计冻结证据。页面夹具漏复制新视图、未导入Session trait及模板中当前Blade不支持的三元表达式已纠正，未放宽安全校验。API-only夹具初次复用了要求GET/login页面的网页注册辅助方法，现改从真实API注册；无GET管理页面的405按原生路由保留。最后上述三库与严格检查使用冻结源码，无并行写入。

范围SHA为`target/tmp/user-starter/two-factor-management-source-sha.json`，独立提交清单为`two-factor-management-files.json`。已直接删除本批文档dist，逻辑文件大小78,390,368字节；清理后当时可用74,890,387,456字节。系统差额可能含并行构建，不全部归因本批；保留正常增量缓存、源码、日志、数据库及数据卷，详见`two-factor-management-cleanup.json`。

A01-04仍未闭合：后续继续Passkeys、第三方登录及无密码近期确认。S10仍需最终自定义模型／配置／驱动组合与观测记录敏感数据审查，不能由本轮页面私密缓存头推断所有日志已脱敏。独立Passkeys事务草稿不属于本批提交，也未被上述冻结检查计作通过；无推送。

## Passkeys事务基础与SQLite原生验证修复（2026-11-25）

以既有API-F02固定的passkeys-server0.3.0／2e7f893429d35acff45ac16d05ddcf883372a20e为准，重新核对StorePasskey、VerifyPasskey、DeletePasskey及两类Options Action；原文和SHA保存在`target/tmp/user-starter/reference/passkeys-0.3.0/`。旧版auth及对应测试仅找到Passkeys功能标记，没有可直接复用的WebAuthn事务实现；本批复用当前webauthn-rs-core0.6.2、所选Eloquent模型、锁、转换及事件，未另建验证器。

- StorePasskey／VerifyPasskey增加`call_in_transaction`；调用方持有当前用户写锁并负责整体提交／回滚，事务不得跨用户或凭据连接。动态Authenticatable经已注册UserModel适配器核对真实类型、持久化状态、原始主键及连接，使未启用Passkeys的泛型调用方不必额外实现该能力。
- `VerifyPasskey::call_discoverable_in_transaction`显式保留发现式userHandle校验；读取凭据归属后也不能降级校验。`get_passkey`与`Passkeys::user_for`提供所选模型／用户的正式读取入口，不能当作认证成功。
- DeletePasskey增加同库、持久化用户与真实归属的事务删除入口，保留模型删除与事件。注册／验证／删除取消，以及模型声称成功但未持久化创建、验证结果或删除时，事务入口均报错，要求回滚；包含真正软删除的模型仍按其删除行为处理。
- 发现原生VerifyPasskey在SQLite凭据事务内调用事务外Model::find查询用户。定向将这一处恢复旧查询后，真实签名用例在五秒内超时，日志`passkey-transaction-native-old-lookup.log`；正式实现同连接用户在原事务查询，修复后原生call也通过。原生跨连接用户读取保留原行为，没有因此承诺跨库原子认证；新增事务入口拒绝跨连接。
- 事件仍在模型保存后、外层提交前派发，匹配原生Action。回滚不撤回已调用的内存监听器；外部副作用使用现有提交后机制，调用方回滚后丢弃或刷新返回模型。

### 实际验收

`passkey-transaction-sqlite-final.log`、`passkey-transaction-mysql-final.log`与`passkey-transaction-postgres-final.log`分别通过SQLite、MySQL8.4.13、PostgreSQL18.7的6项：既有清理1项、契约4项及新增事务签名生命周期1项。新增用例用测试认证器生成真正ES256签名，由原生WebAuthn验证；覆盖UUID用户、ULID凭据、自定义表／字段转换、typed事件、计数器更新及回滚、相同计数器重放、错误userHandle、跨用户、错误连接、取消保存／删除、空保存及空删除，以及普通call的注册／验证／删除。测试签名器不是实体设备、生物识别或云同步验收；挑战过期及一次性存储属于上层HTTP流程，不由直接Action调用冒充完成。

最终严格门禁为`passkey-transaction-clippy-final.log`（Passkeys＋Fortify/Sanctum全部目标）、`passkey-transaction-workspace-clippy-resumed.log`（冻结workspace全部目标）和`passkey-transaction-fmt-final.log`，均通过。三语Fortify教程增加正式调用及服务器Options状态／回滚／事件边界；`passkey-transaction-docs-final.log`构建、`passkey-transaction-docs-browser-final.log`三语双基路径桌面／手机12组文章DOM检查通过。源码范围SHA为`passkey-transaction-source-sha.json`，提交清单为`passkey-transaction-files.json`，均在本工作流目录。

32GB停线中断的`passkey-transaction-final-fourth.log`不计通过；随后恢复在冻结源码上完成上述验收。该守卫取得锁时大部分产物已由并行守卫清理，本守卫日志只记录自身释放102,400字节及当时可用127,188,271,104字节，不重复归因其他批次释放。末次直接删除本批文档dist及已退出测试临时目录，逻辑大小78,435,415字节；当时可用61,819,203,584字节，详见`passkey-transaction-cleanup.json`。源码、验收日志、正常增量缓存、下载、数据库与数据卷保留。

早期编译和夹具失败保留：SQL表达式需传引用、Listener签名及数据库Value匹配已修正；同步Mutex断言缩短为独立作用域，未用allow屏蔽Clippy。内部HttpError的Display隐藏源错误，空保存测试改为检查正式Internal源，不修改生产错误暴露行为。

本批仅交付原生事务基础与已复现的SQLite修复；用户套件的Passkeys页面、无会话登录／近期确认和因素撤销证明绑定仍继续实施，A01-04没有闭合。已按同一固定API-F02清单下载验证官方客户端0.4.1及SimpleWebAuthn13.3.0，构建了下批17.33kB候选，但尚未接线，未夹入本提交。未推送。

## 用户套件通行密钥网页与绑定确认（2026-11-26）

本批接入固定Passkeys服务器0.3.0与官方客户端0.4.1（c4ee7572993aadb40c630dedc349fe74cca0c89f），复用既有API-F02版本清单及前一批原生事务动作。SimpleWebAuthn/browser为13.3.0。客户端源档与锁文件摘要已按前批固定值核对，npm发行包也核对为同版本、MIT元数据；两份上游客户端归档均未附单独LICENSE，不杜撰版权年份。编译包去掉构建机路径注释后为15,213字节，随包发布NOTICE并逐字保留SimpleWebAuthn原许可。参考资料与构建脚本在本工作流reference目录，未引入运行时CDN或另一套WebAuthn引擎。

- PasskeyActions提供可替换的原生HTTP动作接线，独立Passkeys默认仍委托原生Action。套件仅在启用user.passkeys时绑定当前应用及其选定用户模型，注册、删除与确认通过共同用户写锁及既有事务动作执行。两类Options新增同事务读取，避免SQLite单连接内嵌套查询。
- 原生近期确认写入实际凭据模型、真实主键及公钥／userHandle指纹；指纹不包含签名计数器，普通后续认证不使其误失效。每次敏感操作重新核对用户安全状态、当前会话、所选模型及仍存在且归属正确的凭据；撤销本次确认所用凭据后，下一次敏感操作必须重新确认。不是只设置一个时间戳。
- 原生通行密钥登录通过应用授权回调后继续同一个Fortify双因素入口，不绕过已启用因素。发现原Cookie第二因素阶段只有login.id／remember，现以已有ChallengeStore补齐持久化一次性阶段，绑定主因素凭据快照、当前会话和期限；Cookie和Token挑战不能混用。TOTP、恢复码及限流继续复用既有实现。Cookie完成只创建会话，不额外签发PAT。
- 完整预设发布/user/passkeys账户页、本地JS及NOTICE，登录和确认页提供通行密钥按钮。使用命名路由、原生CSRF和SDK的options／redirect协议；这些原生会话路由不是Bearer Token接口。套件Fortify路由通过现有缓存头中间件设置private／no-store。
- 无本地密码账户的页面不再要求填写一个不存在的旧密码，也不因此跳过近期确认。已绑定通行密钥可确认并安全设置本地密码。本轮真实HTTP夹具提供nullable密码列；未改动默认Laravel users基础迁移，第三方账户创建所需的正式迁移仍属后续Socialite单元。

### 验收证据与边界

证据统一保存在target/tmp/user-starter/。passkey-management-regression-final.log最终SQLite3.54.1套件94项通过，包含真实ES256注册／登录／确认／撤销、错误来源和挑战重放、凭据撤销后重新确认、无本地密码确认和设置密码、密码轮换后拒绝旧Cookie第二因素，以及合法第二因素完成后没有多发PAT。签名测试仅在内存生成私钥。passkey-management-native-regression.log的原生契约4项及事务生命周期1项通过，保留48组固定PHP向量。

passkey-management-runtime-third.log记录真实Artisan完整预设首次及重复安装、自动重建和独立MySQL8.4.13迁移成功。passkey-management-app-browser.json当时记录三语／桌面和手机6组实际Chrome流程：本地官方SDK、CDP虚拟驻留凭据、注册、双因素恢复码登录、通行密钥近期确认及撤销后确认失效；没有把虚拟认证器计为实体设备、生物识别或云同步验收。passkey-management-published-assets.json核对真实发布的JS与NOTICE和内置文件逐字节一致。

三语starter-kits正文增加user-passkey-pages章节；passkey-management-docs-first.log文档构建通过，passkey-management-docs-browser-first.log覆盖两种部署基路径、三语和桌面／手机12组正文DOM验收。新增正文只说明已交付的会话路由，不把尚未接通的无会话接口写成可用。

早期浏览器夹具错误保留：退出遵循原生重定向到首页，夹具错误假定已在登录页，现明确导航登录页，未修改产品退出契约。最终原生路由测试选择two_factor_routes，早期命令误写不存在的two_factor目标不计通过。32GB停线中断的原生回归不计通过；归属识别瞬间遇到已退出进程，随后其他守卫按同一规则持锁清理全部项目产物，事件记录释放86,569,783,296字节、清理后可用118,311,071,744字节，不重复归因本工作流。源码、日志、锁、下载、数据库及数据卷保留。

本单元仍不闭合A01-04或整个A01：无会话Passkeys登录／近期确认／管理、Socialite及S10最终模型／驱动矩阵和观测脱敏继续实施。生产HTTPS、实体认证器及真实身份供应商账号验收单列，不以本地模拟冒充完成。未推送。

最终补验：passkey-management-clippy-final.log与passkey-management-native-clippy-final.log分别通过Fortify/Sanctum、Passkeys/Sanctum全部目标严格Clippy；passkey-management-install-final.log通过安装14项和原生2FA路由6项。passkey-management-mysql-first.log和passkey-management-pgsql-first.log的最终覆盖均为新增通行密钥／无密码两项及Cookie第二因素两项，分别在独立MySQL8.4.13、PostgreSQL18.7和database Session中全部通过。passkey-management-final-tests.log退出0，包含范围rustfmt检查；没有把先前中断或错误目标命令计入。

最终清理仅直接删除本批文档dist，逻辑大小78,452,952字节；当次可用空间增加81,645,568字节，清理后可用53,311,385,600字节，系统差额可能包含并行进程，不全部归因本批。正常Cargo增量缓存保留，详见passkey-management-cleanup.json。独立清单passkey-management-files.json，源码摘要passkey-management-source-sha.json；除Cargo.lock仅取本包两项既有测试依赖外，清单源码与实际冻结验收源码逐字节一致。

## 无会话通行密钥 API 与运行时 JSON 译文（2026-11-26）

该单元在套件 API 前缀下增加八个端点：POST auth/passkeys/options、POST auth/passkeys、GET user/passkeys、POST user/passkeys/options、POST user/passkeys、DELETE user/passkeys/{passkey}、POST auth/passkeys/confirm/options、POST auth/passkeys/confirm。固定 Passkeys 0.3.0 的网页控制器与官方客户端协议保持不变；无会话端点属于套件扩展，不声称它们是该版本原生路由。客户端返回 ApiResponse，删除成功是204空正文。

复用原生 WebAuthn Action、模型事件及共同用户事务锁。仪式状态只存既有 ChallengeStore，客户端只持有公开 options 和一次性 challenge；期限取原生 timeout，上限五分钟，注册还不能超过起始近期证明。管理动作复核真实认证令牌、能力、用户安全快照及确认所用密钥，事件后再次读取并检查。初始设备名称和请求能力由服务器保存，提交不能扩大授权。无本地密码登录仍须完成已启用第二因素；原生允许登录回调保留，撤销密钥后不能完成旧的第二因素挑战或使用旧确认。

应用生成的原生网页 passkeys 限流器在缺少 credential.id 时读取 Session，不能用于无会话端点。真实双节点验收发现该混用导致500，本轮改为独立 user.limiters.passkeys，默认10,1,user-passkeys；原生 fortify.limiters.passkeys 不变。回归专门保留一个依赖 Session 的网页限流器，验证 API 不再调用它。应用可设置原生命名／数字策略，空字符串或false关闭；自定义 API 策略须支持无 Session。

API-only安装同时发布固定官方客户端及NOTICE，导出 startRegistration、startAuthentication 供用户自己的界面调用；不发布账户管理网页。重复安装保护已编辑资源，--force才替换。完整或启用Passkeys的预设要求非空API前缀，避免与原生网页账户路由冲突；普通未启用Passkeys的API-only仍允许空前缀。安装预检在修改用户套件文件前拒绝不合法前缀。

新增 Localizer::add_json_lines 与 Lang::add_json_lines，按字面键合并完整JSON原句，先加载并保留其他文件译文；现有 add_lines 分组／点路径规则不变。固定上游 Translator.php 的 addLines 使用 Arr::set，不能把句点行为错误归为上游缺陷。新入口是Rust编译内置JSON默认值的明确补充，来源核对保存在target/tmp/user-starter/reference/Translator-1449bf24.php，对照[固定源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/Translator.php)。套件为八条原生通行密钥错误补齐英／简／繁编译默认译文，仅填缺项，正常应用lang/<locale>.json仍可覆盖。

### 实际验收

- passkey-api-regression-delivery.log：SQLite3.54.1回归138项通过，另4项外部环境用例保持ignored，不计通过。覆盖安装16、配置7、响应10、UI／客户端97、原生第二因素路由6及挑战／凭据各1。
- passkey-api-mysql-delivery.log和passkey-api-pgsql-delivery.log：三个新增API用例分别在独立MySQL8.4.13、PostgreSQL18.7及database Session配置通过。包括真实ES256、无本地密码、能力限制、跨令牌／用途拒绝、到期、重放及撤销后不能完成旧第二因素。
- passkey-api-json-delivery.log：新增字面JSON2项及既有复数选择2项通过；实际FileLoader和Lang门面验证完整标点键、参数替换、保留已有内容、非法locale拒绝及原分组入口不变。
- passkey-api-runtime-fifth.log及当时的passkey-api-app-browser.json：真实Artisan API-only首次／重复安装、自动重建和迁移通过；两个应用实例共享MySQL8.4.13和Redis8.10.3，Chrome中英／简／繁及桌面／手机6组，每组13个API请求交替转发给两个节点。实际运行三语教程中的函数，完成注册、绑定、登录、近期确认和删除；请求没有Cookie或CSRF标头，密钥私钥没有写入日志。使用CDP虚拟认证器，不计真实硬件或生物识别验收。匿名默认限流跨批次生效，最后一组等待自然进入下一窗口，没有放宽产品默认限制。

- passkey-api-clippy-delivery.log：Fortify全部targets、Sanctum特性严格Clippy通过；passkey-api-core-clippy-delivery.log：框架lib及两组受影响翻译测试在Sanctum特性严格Clippy通过。范围rustfmt检查通过。三语JavaScript示例与实际Chrome消费源码逐项一致。
- passkey-api-web-client-regression.log：更新后的15,263字节本地SDK继续通过三语／桌面和手机6组完整网页流程，包含双因素恢复码登录、近期确认及撤销后确认失效；真实Artisan完整预设的首次／重复安装和MySQL迁移通过。与API-only六组分开记录。

- passkey-api-docs-head-build.log和passkey-api-docs-head-browser.log：最终三语正文构建和两种部署前缀下24组桌面／手机DOM检查通过；本次新增user-passkey-api及runtime-json-messages章节。passkey-api-source-sha.json记录本批输入，passkey-api-published-assets.json核对JS／NOTICE字节。最终完整A01仍须基于当时已提交HEAD重新组合，不将隔离源码的局部检查冒充整个当前工作区已通过。

首次三轮本地运行暴露网页限流器混用，保留失败日志；临时应用诊断已去除。第四轮触32GB磁盘停线后安全中断，不计通过。守卫协调清理后恢复，用户套件守卫最后得到释放差值-7,733,248字节、可用118,247,784,448字节，说明大部分释放发生在另一个守卫的清理中，不重复归因本批。源码、验收日志、下载及保留的数据不受影响。

交付清理passkey-api-cleanup.json：持本批Cargo锁，直接删除已闲置的两份翻译测试可执行文件、文档dist和核定哈希的隔离应用发布JS／NOTICE，删除逻辑字节265,008,862、分配字节267,902,976，磁盘净增加273,936,384字节，清理后可用37,167,075,328字节。保留后续可复用的增量缓存、源码、证据、依赖下载、数据库和数据卷，未移入废纸篓。

A01-04和整个A01仍不闭合：Socialite、S10最终自定义模型／驱动组合和观测脱敏继续实施。真实供应商账号、生产HTTPS和实体认证器属于另列的外部验收；本轮没有推送。

### 已提交本地化重构后的最终组合

提交前已合入主线的07b4ed8d3重构了语言目录、Localizer命名和默认词条源。原本按旧服务运行的证据保留为历史，最终采用归档基线21a5bcd7981b4b3e81553071fe7cf25aff07f5a4（含该重构）加本批明确清单：passkey-api-combination-base-verification.json核对3036份其他Rust源码／清单与该提交blob完全一致，未引入其他代理的未提交代码。旧隔离源码和应用备份分别保存在passkey-api-before-localization及browser-devices-app-backup-before-localization，没有覆盖或删除。

- 公开入口使用localization::Localizer和Lang；消息注册转为同一Localizer实例。add_json_lines按新默认词条源的运行时覆盖／释放机制接线，先在锁外读取输入迭代器。新增配置词条源刷新及移除后的实际读取回归。
- passkey-api-localization-json-delivery.log：复数选择2、既有默认词条源2及本批字面JSON3，共7项通过。passkey-api-localization-regression-delivery.log：138项套件回归通过、4项既有外部项忽略；对应mysql-delivery和pgsql-delivery各3项通过。passkey-api-localization-clippy-delivery.log：Fortify全部targets严格Clippy通过；passkey-api-localization-core-lib-clippy.log：框架lib严格Clippy通过。范围fmt通过。被磁盘守卫中断的框架lib加测试Clippy不算通过，后续只补未完成lib检查，已通过的普通测试和Fortify检查没有重新计数。
- passkey-api-localization-runtime.log及passkey-management-localization-runtime.log：真实Artisan API-only与完整预设首次／重复安装、自动重建、MySQL迁移通过；新组合分别在Chrome以三语桌面完成3组API双节点／共享MySQL和Redis流程及3组网页／恢复码流程。独立结果保存为passkey-api-localization-browser.json与passkey-management-localization-browser.json。运行器原通用browser JSON是滚动输出，旧六组手机／桌面结果以当轮已保留日志为准；本次客户端和全部模板的字节未变（unchanged-ui.json记录实际数量与SHA），因此复用此前手机布局证据，不将桌面重跑冒充手机实机。
- passkey-api-localization-docs-build.log与docs-browser.log：最终新命名下三语文档构建及24组两种挂载前缀／桌面手机正文通过。三语JS示例仍与实际运行源码相同。localization-published-assets.json核对当前实际发布JS／NOTICE与内置资源完全一致。
- 组合期间再次触32GB，RustRover的本项目cargo check曾阻塞持锁清理；仅安全中断该次检查，没有关闭IDE。框架lib加测试的检查被守卫停止，随后其他守卫完成项目编译产物回收，未将其释放重复归因本单元。源码、日志、依赖下载和数据库保留。
- passkey-api-localization-cleanup.json：再次持本批Cargo锁直接删除闲置文档dist及核定哈希的隔离发布资源，删除逻辑80,853,811字节、分配83,644,416字节，磁盘净增加83,980,288字节，清后可用38,104,809,472字节；保留下一单元需要的正常增量缓存与全部验收记录。

本批只交付上述API／本地化接线，不闭合A01；Socialite和S10继续按主计划执行。新起草的socialite/identity.rs尚未接线／验收，不纳入本次提交。


## HTTP敏感诊断前置单元（已验收，A01继续开发）

为套件的认证及账户路由接入ProtectSensitiveData和sensitive别名。Kernel在RequestReceived与全局中间件之前按已有路由匹配和已解析中间件识别，保守覆盖方法覆写与405；Router在直接分发及转发时兜底，标记在同一请求内只增强。复用请求时清除旧标记，离开任务作用域后不可读取。HTTP输入、响应、认证、CSRF及状态码不变；这是Rust诊断保护扩展，不声称上游有同名中间件。

Telescope保留被允许采集的安全请求元数据，去除原始URI参数、正文、请求／响应头、标签及用户姓名邮箱；原生Query、Mail、Model、Event、客户端和日志等诊断入口在组装敏感条目前跳过。公开record和record_update及内部水合计数共同遵循边界，首次识别时清除先前待写入数据。日志Origin使用路由模板。普通路由及既有过滤／暂停策略继续生效。没有改动Pulse的占位SQL或路由模板记录，也不将Telescope原本仅保存“HTML Response”的行为误报为HTML原文泄漏。

证据位于target/tmp/user-starter/：

- privacy-final-source.json：以c774e1e270b97c14dd4fe6dc76c457929d38d2a9为已提交组合，3058份其他Rust／清单文件与该提交一致，排除其他代理未提交改动；privacy-source-sha.json核对本批正式文件和实际测试源码一致。后续至提交父版本的受影响框架／Fortify／Passkeys／Telescope生产Rust未改变；不把该局部组合当作全仓所有功能已通过。
- privacy-final-regression.log及privacy-regression-excluding-baseline-stack.log：不重复计数，共148项通过（Fortify Provider 3、配置7、账户UI 97；框架敏感请求1；Telescope条目3、存储7、Watcher 15、敏感存储1、Provider 14）。Redis的1项既有外部用例忽略；另1项已确认的基线栈溢出明确过滤，不算通过。新增用例覆盖全局中间件前置识别、405、复用请求清标记、异步子任务隔离，以及包含密码、Bearer、Cookie、近期证明、路径／查询秘密、原始SQL和手工条目更新的实际持久化。普通记录正向对照通过。
- privacy-final-clippy.log：同一组合的框架／Fortify／Telescope及上述测试目标严格Clippy通过；范围Rustfmt与Git差异检查通过。
- privacy-http-runtime-fourth.log及privacy-http-build.log：在独立源码临时登记user-starter-sensitive-http二进制（指向tests/fixtures/sensitive_application.rs），真实Artisan migrate与serve、TCP HTTP、SQLite、array邮件发送均通过。200与403只存安全请求摘要；原始HTTP成功正文保持完整；普通请求仍实际保存SQL和邮件内容。运行器为vendor/openrust/telescope/tests/sensitive_http.py；退出后停止服务器并删除独立数据库，服务器日志保留在target/tmp/telescope-sensitive-http/。不使用真实SMTP或供应商账号。
- privacy-docs-build.log与privacy-docs-browser.log：三语Fortify正文构建及两种挂载前缀、桌面／手机共12组实际DOM检查通过。
- 首轮夹具的公开API调用、Watcher完整配置及异步延后写入等待问题已修正，失败日志保留；磁盘中断不计通过。privacy-cleanup-stop.log记录停线后持锁释放77,292,556,288字节、可用109,274,398,720字节。交付清理privacy-cleanup.json直接删除本批闲置测试／HTTP二进制与文档dist，磁盘净释放635,699,200字节、清后可用41,443,205,120字节；保留正常增量缓存、源码、日志、依赖、数据库和数据卷。

未闭合边界：此单元只保护HTTP作用域。后台认证邮件没有HTTP标记，须继续复用现有Context作用域与加密通知投递接线并用真实Worker验收。扩大回归发现native_event_gate_notification_model_view_and_dump_sources_are_observed在同步队列通知阶段发生默认测试栈溢出；privacy-baseline-provider.log/json已将本批正式改动逐项恢复为上述已提交基线后复现，privacy-provider-progress.log进一步定位到queued通知调用。该问题不是本批新增，不加大测试栈掩盖，纳入紧接的后台通知保护及S10收口；未将相关Provider整个测试组标成全过。Socialite、最终组合及其他A01缺口继续按主计划执行。


## 同步队列通知的语言作用域栈修复（已独立验收）

修复上节登记的基线默认栈溢出。Delivery::localized沿用现有Container::scope的布局做法：在构造语言作用域异步状态机前Box固定业务Future，随后仍通过原生Lang::with_locale运行。正常及排队通知的回调、错误、队列顺序、本地化与取消行为保持原契约；不新增公开调用或配置项，因此不向三语教程增加内部实现细节。

- 基线和定位：privacy-baseline-provider.log在恢复已提交代码后复现；worker-stack-trace-detail.log显示HTTP→同步队列→通知→语言作用域的嵌套链。worker-future-size-detail.log与worker-delivery-size-detail.log记录邮件及投递子Future实际尺寸；worker-stack-frames.log保留本机ARM64函数栈分配片段。短信分支与Event.until的尝试未修复，已恢复；最终只改变通知语言包装的Future布局，未改默认测试栈或并发。
- 最终独立候选：notification-stack-final-source.json核对3088份其他Rust／清单文件与11a2220e0edcffd1e06f335271829d54526d9f67一致；notification-stack-final-independent.py再把4份后台诊断保护草稿恢复为该基线并排除其新测试，只应用notification_sender.rs修复。notification-stack-final-baseline.json记录准确版本与排除范围；此前independent日志属于较早冻结组合，不冒充最终整仓验收。
- notification-stack-final-regression.log：原失败综合场景在默认环境中通过，Telescope Provider完整15项通过、Redis外部项1项忽略；事件、通知邮件及Fortify Provider选择目标也正常返回。notification-stack-final-clippy.log对应选择目标严格Clippy通过，范围Rustfmt及差异检查通过。
- notification-stack-independent-regression.log另保留SQLite／自定义队列与原生Artisan Worker暂停、恢复、失败重试、上下文隔离的本机记录。SMTP及MySQL／Redis／PostgreSQL／SQL Server的服务入口未在本轮提供必要环境，部分旧测试会提前返回，均不算对应真实服务验收；子进程夹具的重复输出不重复计数。
- notification-stack-cleanup.json：持本批Cargo锁直接删除7份闲置验收测试二进制，净释放1,332,297,728字节，清后可用45,953,073,152字节；保留正常增量缓存、源码、日志、下载、数据库和数据卷，没有本批待清理Docker资源。

该栈问题从后续缺口移除；后台认证邮件诊断保护、真实Worker与Socialite／S10仍在开发。Context草稿、自定义第三方身份草稿和其他代理在途文件未包含在本笔修复中。


## 后台加密通知诊断保护（真实 Worker 已验收）

沿用既有 Context／队列上下文脱水与恢复机制，不另建任务局部状态或队列协议。加密通知派发时在隐藏上下文加入 openrust.diagnostics.sensitive=true，非秘密布尔值随原生载荷传播，调用方原Context随后恢复；未加密通知不自动加入标记。处理器兼容旧加密载荷，在既有作业作用域内设置标记直到作业结束，失败和后续事件也受保护。原生Worker的逐任务隔离使下一份普通任务正常采集。Telescope构造时捕获当前Context仓储，采集回调不递归解析容器；作用域外管理端配置只对ScopeClosed保留无Context状态，其他绑定错误仍返回。已识别敏感操作的同一采集器保持保护，不因内层scope退出或取消重新泄漏后续响应。

实现复用本记录既有Context／通知／Telescope源码调研与固定上游契约；隐藏标记是Rust诊断扩展，不声称Laravel有同名内置键。不会自动处理业务代码显式写向其他日志或供应商的内容。

证据均保存在target/tmp/user-starter/：

- worker-privacy-fourth.log：与独立栈修复组合后，Fortify Provider 3、Telescope Provider 15、敏感Context 1及HTTP敏感记录1，共20项通过。worker-privacy-final-regression.log最终选择Telescope三个目标17项通过、既有Redis外部Provider用例1项忽略；worker-privacy-final-clippy.log对应lib／测试严格Clippy通过。两轮重复目标不累加。
- Context新增测试真实SQLite和公开Context门面覆盖内层错误、取消后恢复原隐藏值、采集器继续脱敏及下一普通请求恢复SQL／JSON记录。原Provider作用域外配置和默认栈综合场景也通过。worker-privacy-source-sha.json核对正式实现／测试与实际冻结源码；冻结基线沿用notification-stack-final-source.json的11a2220e0edcffd1e06f335271829d54526d9f67及已提交通知栈修复，不将其他代理之后的整个主线冒充已验。
- worker-privacy-build-second.log及fixture-clippy-second.log：新增真实运行夹具编译与严格Clippy通过；worker-privacy-runtime-second.log中的原TCP HTTP／SQLite成功、失败和普通对照通过。夹具仅在独立源码临时注册bin，不修改应用正式Cargo清单。
- worker-privacy-database-second.log：独立MySQL8.4.13、原生Artisan migrate／serve／queue:work、database持久化队列和固定摘要Mailpit SMTP通过。连续三个任务包含私密邮件发送成功、私密模板错误失败及普通邮件成功；两份加密载荷隐藏标记true、普通false，加密数据不含明文探针；Worker退出0、failed_jobs=1、队列为空，Telescope私密条目0、普通邮件1，SMTP实际接收2封。
- worker-privacy-redis-third.log：独立MySQL8.4.13、Redis8.10.3持久化队列与同一SMTP流程通过，成功／失败／后续任务隔离的结果相同。初轮Redis夹具把原生RedisJob外层误作直接Payload，解析前失败，没有运行Worker，不算通过；修正为读取既有payload字段中的JSON字符串后实际补验，未改变产品队列协议。
- 原始运行结果分别为target/tmp/telescope-sensitive-worker/e349c358f0fd-result.json与aeea20d9aced-result.json。运行器复用tests/mysql_acceptance.py和smtp_acceptance.py固定Mailpit摘要；本批Redis镜像固定实际已缓存ID。所有服务loopback、独占容器、标签核定且tmpfs无数据卷；退出后删除本批容器，结果确认containers_removed=true。未动共享数据库、数据卷、镜像或BuildKit缓存。SMTP为本机真实协议接收，不是生产SMTP、外部供应商或手机／邮箱实际收信验收。
- worker-privacy-docs-build.log及docs-browser.log：三语Fortify敏感诊断章节增加加密任务及Context::scope用法，内容门禁／文档构建及两种前缀、桌面／手机12组实际正文DOM检查通过。正文不含内部流水。

首轮夹具API类型错误、ScopeClosed和Redis封装假设的失败日志全部保留。此前已修复的通知语言作用域栈问题未通过加大测试栈绕过。本单元完成HTTP之外的后台通知保护；Socialite和S10最终组合继续开发，A01各阶段仍未整簇闭合，未推送。

交付清理worker-privacy-cleanup.json：持本批Cargo锁直接删除已闲置的运行夹具、敏感测试可执行文件和文档dist，净释放1,005,498,368字节，清后可用37,419,700,224字节；正常增量缓存、源码、验收记录、依赖下载、数据库和数据卷保留。
