# API-A01 用户认证入门套件

## 范围与状态

执行基线为[正式计划](../../plans/user-auth-starter-kit.md)及其S01至S10契约。2026-11-25用户授权持续实现A01-01至A01-05，遇到实现问题按固定基线与既有契约裁决；实测通过后分批本地提交，不推送。范围和完成门禁没有缩减。

| 批次 | 状态 | 剩余工作 |
| --- | --- | --- |
| A01-01 安装与基础认证 | 开发中 | 显式安装预设、基础账户流程与响应适配、安装保护、真实Artisan／HTTP。 |
| A01-02 邮件与生命周期 | 未闭合 | 邮件落地、资料变更、跨进程凭据撤销与失败恢复。 |
| A01-03 账户中心 | 未闭合 | Token客户端协议、重新认证、头像、设备与注销。 |
| A01-04 网页与可选认证 | 未闭合 | 完整Blade页面、2FA／Passkeys／第三方登录接线及三语教程。 |
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
