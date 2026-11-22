# 欢迎页浏览器语言识别（2026-11-22）

> 下方为历史实现与验收记录；d4a40d7f4 复核见末节。后续 BCP 47 迁移已覆盖旧下划线约定，当前行为与验收见[语言标识迁移](locale-bcp47-2026-11-22.md)。

## 实现范围

欢迎页按 Accept-Language 的 q 权重选择 en／zh_CN／zh_TW；权重相同时保持浏览器顺序，q=0 条目不参与选择，未知或非法条目跳过，无匹配时使用受支持的 APP_LOCALE，否则英文。英文地区变体归并英文，中文 Hans／Hant 优先地区，TW／HK／MO 为繁体、CN／SG／裸 zh 为简体；仅欢迎页自动选择，不改变全站默认语言。

WelcomeController 复用 canonical_locale（已有 ICU 规范化）及 Lang::getWith 的显式 locale 参数；一次性准备翻译数据，模板保持同一份，不引入管理器、中间件、依赖或共享语言状态。语言文件为 lang/{en,zh_CN,zh_TW}/welcome.json；八项文案与 html lang、Content-Language、Vary、文档和部署链接同步，名称／版本与原布局保留。

Docker runtime 增加 COPY lang/，三语部署命令增加复制 lang；避免二进制与模板部署后缺少翻译。三语国际化正文说明欢迎页的支持与回退规则。

## 调研与适配

已联网保存并阅读固定 Laravel 文档 960eedf0911155ad16326ade49c0ea5d18eda0b8 的 localization.md，以及 Symfony HttpFoundation v7.3.0 Request.php 的 getPreferredLanguage／getLanguages。前者用于语言文件与 fallback 组织，后者用于请求头优先级及语言变体协商思路。现有 OpenRust Request 没有 getPreferredLanguage，媒体类型 Accept 解析也不适用于语言范围，故在欢迎页控制器增加私有选择函数，未新增公开框架 API，也不宣称完整 Symfony 语言协商等价。HTTP qvalue 用整数千分位按标准语法处理，非法／重复参数忽略该条目；没有可选择项时仍返回默认欢迎页，不返回 406。

来源：
- https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md
- https://github.com/symfony/http-foundation/blob/v7.3.0/Request.php

## 验收

基线 fb5240e1ae08855857d9b9988ac5453d8623e608，保留此前未提交修改。全部临时源码、日志、独占 Cargo target 与 TMPDIR 位于 target/tmp/welcome-locale-20261122/；Rust 1.94.0，正常并发、保留增量，dev/test debug=0、strip=symbols，非优化 release 构建。最终相关输入逐字节核对一致。

| 范围 | 结果与证据 |
| --- | --- |
| 真实欢迎页 HTTP Kernel | tests/welcome.rs 四项通过：三语／地区／文字体系／优先级／重复请求头、无效和未知语言／q=0／默认回退、并发请求与独立应用隔离、缺键英文 fallback；welcome-tests.log |
| 原欢迎页入口 | tests/startup.rs 的具名视图查找／替换／缺视图行为回归通过；startup-welcome.log |
| 独立部署 | --features standalone 构建通过；二进制、resources、lang、public 复制到隔离部署目录，以空 PATH、异地 cwd 启动真实 HTTP，无开发库操作；standalone-build.log、http-server.log |
| 实际浏览器 | Chromium 12 组合：三语 × 桌面／手机 × 明／暗；文案、文档／部署链接、html lang、响应头、无横向溢出与页面异常通过；browser-complete.log、browser-results.json；截图已人工检查简体手机亮色与繁体桌面暗色 |
| 质量检查 | 受影响根应用 bin、welcome／startup 测试严格 Clippy 通过；全 workspace fmt --check 与 git diff --check 通过；clippy.log |
| 文档 | 三语站点构建（含内容检查）与全导航支持边界浏览器检查通过；docs-final-results.json、docs-build-final.log、docs-scope-final.log |

初次测试夹具误传 &PathBuf 而 configure 接收拥有所有权的 PathBuf，已改 clone 并实跑通过；初次文档 scope 缺少浏览器环境变量已补全；临时浏览器夹具误把 main 在桌面／手机的原布局都断言为同一 display，已按实际断点分别检查 grid／flex 并完成全部 12 组合。初次失败日志保留，不作为产品缺陷或成功证据。

## 边界与收尾

未重建 Docker 发布镜像、未运行 Windows 或优化 release 构建；本次验证为 macOS ARM64 的真实本地服务与独立部署目录，不能替代全部部署平台验收。浏览器与本批应用进程退出，未启停原开发容器、未访问开发数据库或删除卷。源码、日志和热缓存保留。2026-11-22 用户授权修正文档语言标签与空行、复核后本地提交本轮改动，不包含推送。

提交前复核：三语国际化正文的地区缩写已改为完整 zh-* 语言标签，并移除多余空行。当前内容检查、隔离文档重新构建、三语全导航浏览器与 fmt 检查通过，暂存差异无空白错误；日志 precommit-results.json 与 precommit-*.log。全部 3,246 项受跟踪输入与隔离快照内容一致（artisan.bat 仅行尾归一化），新增欢迎页语言文件和测试另行逐字节核对一致。


## 统一请求与模板国际化

2026-11-22 用户要求把单页方案改为通用国际化。初版 WelcomeController 私有协商与逐条翻译已被替代；没有另造翻译管理器，也未改默认应用 Provider。

### 核对与决定

- 固定 Laravel Framework v13.40.0／1449bf241017087fbd38c118ecaabef3f6797fdc：读取 Foundation/Application.php 的 setLocale、Foundation/helpers.php 的 __／trans／trans_choice、View/Compilers/Concerns/CompilesTranslations.php、View/Concerns/ManagesTranslations.php 及 tests/View/Blade/BladeLangTest.php。带参数 @lang／@choice 按上游原样输出，双花括号走现有 HTML 转义；翻译在渲染时执行，不把结果编进缓存。
- 已核对固定文档 localization.md、Symfony HttpFoundation v7.3.0 Request.php 语言优先级，以及 mcamara/laravel-localization v2.3.0 的请求中间件／支持语言配置思路。仅采用请求选语言和共享 Translator 的职责划分，不引入 URL 前缀、会话、Cookie 或额外依赖；浏览器协商是本应用策略，不宣称是 Laravel 默认自带行为或完整 Symfony 协商移植。
- 旧版 openrust-v0.x/vendor/openrust/framework/src/translation.rs 的 select_from_catalog 与 with_locale 已核对。其 MessageCatalog／AppConfig 属于旧架构，不搬入现主线；复用当前 canonical_locale、Lang::withLocale、现有 Facade／容器快照和 Blade 表达式树机制。
- 本应用新增 app/http/middleware/set_locale.rs，由 bootstrap/app.rs 注册；config/app.rs 的 supported_locales 默认三语，合法标识规范化，非法或空列表显式失败。未匹配时优先使用受支持的 APP_LOCALE，否则列表首项。既有浏览器权重与中文文字体系优先规则保留，其他语言支持完整标识／基础语言匹配。
- 阻塞模板渲染原先只传 Facade／容器快照，不会继承 Tokio 的临时语言变量。新增内部 LocaleSnapshot，以 sync_scope 在既有渲染线程恢复本次语言，不修改共享默认语言。控制器主动 setLocale、子模板、匿名组件和缓存模板均通过实际消费者验证。
- 模板翻译助手按现有字符串翻译契约返回文本，替换值支持标量／null；复数支持数字和数组计数。分组／列表仍通过 Rust Lang::array 读取。无参翻译器访问、@lang 翻译块、PHP 任意函数／命名参数未实现，三语正文明确边界，没有计为完成。

上游来源：
- https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate
- https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/View/Blade/BladeLangTest.php
- https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md
- https://github.com/symfony/http-foundation/blob/v7.3.0/Request.php
- https://github.com/mcamara/laravel-localization/blob/v2.3.0/src/Mcamara/LaravelLocalization/LaravelLocalization.php

### 本次验收

隔离快照与可复用独占 CARGO_TARGET_DIR 继续在 target/tmp/welcome-locale-20261122/；子进程 TMPDIR 使用该目录 tmp/，Rust 1.94.0，保留增量与正常并发，dev/test debug=0、strip=symbols。所有测试均实际运行，无 ignored／跳过项；本次没有共享数据库或缓存驱动变更，不将文件翻译测试视为数据库验收。

| 验收 | 结果与证据（均在上述目录） |
| --- | --- |
| 根 welcome／localization／configuration／configuration_cache／application_skeleton | 25 项通过；shared-locale-root.log。欢迎页新增通用路由、校验消息、自定义支持语言、非法配置、响应头合并和并发隔离验收。 |
| 框架 Blade／View／Translation 定向目标 | 31 项通过；shared-locale-framework-final.log。包含新翻译助手 3 项测试，验证语言覆盖、占位符、复数、HTML 正反例、缺键／回退、缓存／子模板／组件、无翻译服务及非法调用。 |
| 原 welcome startup 消费者 | 1 项通过；shared-locale-startup-final.log。夹具同步注册真实应用中间件。 |
| workspace 全部目标严格 Clippy | cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings 通过；shared-locale-clippy-final.log。第三方补丁依赖的既有 cfg／deprecated 诊断仍保留原日志，不声明第三方告警为零。 |
| 格式与差异 | cargo +1.94.0 fmt --all --check、git diff --check 通过。 |
| 三语正文与实际站点 | 构建、/ 和 /docs/ 三语全导航，以及桌面／手机检查通过；shared-locale-docs-build-final.log、shared-locale-docs-scope.log、shared-locale-docs-browser.log。 |
| standalone 独立目录实际 HTTP／浏览器 | 三语×桌面手机×明暗 12 组合通过；shared-browser.log、shared-browser-results.json。空 PATH、独立 cwd，部署携带 resources／lang／public，已人工查看简体手机浅色截图。 |

首轮失败均保留：新消费者夹具的 ConfigLoader 函数指针、Response Header 类型及 Validator trait 调用已修正；缓存版本断言 34 已同步为 35；文档花括号示例用 v-pre 保护；startup 夹具同步使用新增中间件以消除严格 Clippy 的未使用诊断。修正后针对性复验及最终工作区 Clippy 通过，不用首次失败日志充当通过证据。

未重建 Docker 镜像，未验 Windows 与优化 release；本地验证不等于正式部署验收。standalone 浏览器服务已停止，原开发容器／数据库／数据卷未操作；未触及 32 GB 停线，保留可复用缓存。源码范围与最后磁盘空间见 shared-locale-closure.json。本次未暂存、提交或推送。


## 独立核对时的问题线索（2026-11-22；最终状态见下节）

核对已提交初版 `443f40af4` 与当前未提交实现，并在线读取固定框架 Application.php、LocaleUpdated.php、CompilesTranslations.php、固定骨架 config/app.php 及 Symfony v7.3.0 Request.php。主对话同时修改应用语言入口；下表是本次读取时的结论，不将旧快照验收扩展到正在修改的源码。

| 范围 | 核对结论及处理 | 尚需验证 |
| --- | --- | --- |
| app.supported_locales | 固定骨架仅提供 locale、fallback_locale、faker_locale，没有此键。移除新增配置与动态候选列表契约，三语候选直接留在应用中间件；三语正文、测试与规范同步 | 最新整体实现的欢迎页回归 |
| 已提交控制器逐条翻译 | 初版逐条 Lang::getWith 并非错误译文，但偏离 Laravel 模板直接调用翻译助手的常用职责划分。当前在途代码已移至模板 __，控制器仅准备数据 | 当前 Blade／欢迎页实际消费者回归 |
| 自定义浏览器协商 | Symfony getPreferredLanguage 接受候选参数；当前应用自写协商包含中文地区别名、跳过 q=0 和 APP_LOCALE 回退，是明确的应用策略，不能宣称完整 Symfony 等价 | 保留三语、权重、非法输入及回退用例 |
| App 语言入口 | 当前在途代码已新增 getLocale／currentLocale／isLocale／setLocale／setFallbackLocale 和 LocaleUpdated，setLocale 按配置→译器→事件顺序。此前验收不能证明这次新代码已通过 | 语言事件、请求／任务／应用隔离、错误路径及最新消费者 |
| App 语言标识一致性 | 当前 setLocale 会把 zh-CN 规范化为 zh_CN，isLocale 却按原参数直接比较；因此 setLocale("zh-CN") 后 isLocale("zh-CN") 为 false。固定 Laravel 原样保存参数，不存在该不一致。尚未修改主对话正在实现的入口 | 通过公开 App API 复现，明确标识契约并补正反例 |
| Config Repository 语言覆盖 | 当前仅 set(app.locale／app.fallback_locale) 写作用域覆盖；随后 set(app, 整个对象) 不清除已有覆盖，可能导致 get(app.locale) 与新对象意图不一致。需要用公开 Config API 复现并修复，不能仅依据现有并发测试关闭 | 父键覆盖、all/get(app) 一致性和嵌套作用域 |
| Blade 翻译支持边界 | 固定 CompilesTranslations 同时支持 @lang 区块／@endlang；当前只支持带参数 @lang，文档已声明不支持区块。这是尚未实现的上游能力，不能归为 Rust 无法表达 | 区块翻译与嵌套回归；字符串助手既有回归 |
| 文档与证据 | 本次读取时正文和当前交接仍称中间件使用 Lang::withLocale，但源码已切为 App::setLocale；本次同步该段说明。上文旧验收仅适用于其源码快照 | 主对话最新 App 实现完成后的完整受影响验收 |

本次不提交或推送，不修改其他审计草稿和基类控制器改动。未把主对话正在实现的功能标为已验收。

本次已实际执行：受影响 config/app.rs、SetLocale、welcome 测试的 Rust 1.94.0 rustfmt --check、git diff --check、三语文档内容检查均通过。未运行最新 Rust 行为测试、Clippy 或浏览器验收；上述待验证项均保持未验收，不沿用前一快照结果。主对话同时修改相同国际化文件，后续验收须以最终源码组合为准。


## 按固定 Laravel 教程复审与最终修复（2026-11-22）

本节覆盖上述独立核对的所有在途项；此前 57 项及缓存版本 35 的结果仅代表旧快照。在线重新读取固定文档 localization.md／middleware.md、固定 Application.php、LocaleUpdated.php 和 FoundationApplicationTest.php，材料保存在同批 research/tutorial-*。App 语言操作按应用配置→译器→事件的职责闭环；不把浏览器协商当作 Laravel 默认功能。

| 核对项 | 最终处理 | 实际验证 |
| --- | --- | --- |
| 默认语言配置 | 保持 APP_LOCALE／APP_FALLBACK_LOCALE 和 app.locale／fallback_locale；删除自增 supported_locales 配置，三语候选留在应用中间件 | welcome 的无额外配置消费者，以及无中间件的教程路由通过 |
| 教程 App 入口 | Application 与 App Facade 补齐 getLocale／currentLocale／isLocale／setLocale／getFallbackLocale／setFallbackLocale，复用既有译器与事件服务；app 绑定通过 Weak 工厂避免容器持有应用形成循环 | application_locale 4 项；文档原始路由示例单独编译并实际处理三语请求及非法语言 400 |
| 切换事件与生命周期 | setLocale 更新配置和译器后等待 LocaleUpdated 监听器，提供 locale／previousLocale；setFallbackLocale 不发送语言切换事件 | 监听器读取配置／译器一致；非法设置保持原值；并发请求、独立应用、嵌套任务作用域通过 |
| 语言别名 | 沿用本项目统一语言标识，App 设置和判断均 canonical_locale；这是保留既有语言资源布局的明确适配，不宣称 PHP 原样保存语义 | setLocale("zh-CN") 后 currentLocale 为 zh_CN，isLocale("zh-CN")／isLocale("zh_CN") 为真，其他语言为假，非法输入返回错误；tutorial-alias-final.log |
| 配置父键替换 | 请求／任务中隔离语言配置；整体 set(app, …) 在同一作用域替换父组并清除旧子键覆盖，后续子键 set／push／prepend 保持一致；其他组沿用现有仓储行为 | get／get(app)／all／has、缺失字段、后续数组修改，以及退出作用域恢复默认值通过 |
| Blade 翻译区块 | 新增 @lang … @endlang 及 @lang([…]) 区块，渲染后按 PHP trim 空白范围查键，原样输出；嵌套区块沿用上游共享替换数组行为；缓存版本 36 | blade_translations 4 项含缓存、子模板、组件、替换、复数、转义、非法输入和嵌套区块；旧缓存失效回归通过 |
| 浏览器策略 | SetLocale 调用 App::setLocale，控制器读取 App::currentLocale；仅显式启用本应用策略，可移除注册后按教程使用默认语言／路由切换 | 实际 standalone 服务三语×桌面手机×明暗共 12 组合通过，移动简体截图人工检查无溢出 |
| 文档与规范 | 三语本地化以官方配置和 App 方法为主线，配置章节说明语言作用域，Blade 章节指向翻译用法；规范明确默认骨架与应用策略边界 | 三语构建、/ 与 /docs/ 全导航、桌面手机检查全部通过 |

### 最终证据与边界

- Rust 1.94.0，隔离源码与独占 target 继续复用 target/tmp/welcome-locale-20261122/；70 项定向测试实际通过（框架 44，含文档示例 1；根应用 25；startup 1），0 失败／0 ignored。别名精确场景追加后重新运行 application_locale 4 项通过，未重复计入总数。命令及退出状态见 tutorial-results.json、tutorial-last-checks.json，日志为 tutorial-framework-final.log、tutorial-root-final.log、tutorial-startup-final.log、tutorial-alias-final.log。
- workspace／all-targets／locked 严格 Clippy、fmt、git diff --check 通过。第三方补丁依赖既有 cfg／弃用诊断不视作本批新增告警；不宣称全依赖零诊断。文档完整结果见 tutorial-docs-results.json，欢迎页见 tutorial-browser-results.json。
- 为验证嵌套替换状态，使用已有 php:8.5.12-cli-alpine、无网络／只读挂载运行固定 CompilesTranslations／ManagesTranslations Trait，4 项对照通过；使用最小容器与译器桩验证 Trait 的编译／缓冲区／替换参数行为，不冒充完整 Laravel PHP 测试套件。宿主 PHP 因缺 libtidy 无法启动，已由上述隔离容器完成该项检查；容器自动回收。
- 初次夹具误用了路由参数类型、Delivery::Inline 和私有 Kernel 访问；随后补齐夹具配置。父键数组操作初次缺 trait 导入、初次 Clippy collapsible_if 均已修正并按最终源码重跑，不能将此前失败或磁盘停线前结果计为最终通过。
- 曾触及磁盘停线，安全等待构建结束并持全部已登记 Cargo 锁清理项目编译产物，保留锁 inode、源码、日志、下载、独立部署及全部数据；净释放 38,006,120,448 字节，清理后可用 68,855,119,872 字节。详见 tutorial-disk-cleanup.json；恢复后对最终源码重新执行上述检查。
- 本次没有数据库／缓存驱动行为变更，未使用开发数据；未重建应用 Docker 镜像、未验 Windows／优化 release。语言组使用 JSON 而非 PHP 文件；异步事件使用 await／Result；App Facade 当前只暴露语言方法，模板未支持任意 PHP 函数／Facade 方法调用、命名参数或无参获取译器。三语正文明确这些真实边界。
- 最终验收时尚未暂存、提交或推送，输入一致性、空间和后台资源状态见 tutorial-closure.json。2026-11-22 用户随后授权将本次国际化实现、测试、三语文档及验收记录本地提交；本记录随该提交保存，不包含推送。


## d4a40d7f4 基线复核

2026-11-22 从 `d4a40d7f46e708e6e2b12dee3033148042984638` 重新核对；开始时工作区与索引干净。本节取代前述旧快照对当前交付的结论。本轮没有暂存、提交、推送或发布授权。

### 核对与实际修改

| 核对项 | 结论与修改 | 失败回归与验收 |
| --- | --- | --- |
| 已修复项 | `app.supported_locales` 已移除；候选留在应用中间件；isLocale 别名归一、整体 app 替换、翻译区块均已存在，本次不重复实现。语言标识继续 en／zh_CN／zh_TW；浏览器别名及 q 权重策略不变 | application_locale、welcome 与既有翻译区块测试重新通过；不宣称完整 Symfony 协商等价 |
| 深层父键覆盖 | 复现 app={} → app.ui.title=旧标题 → app.ui={theme:dark} 后旧 title 回流。改为在已覆盖的 app 根值上按调用顺序写入，父对象替换自然丢弃旧子键；沿用现有仓储与作用域 | config-red.log 三项实际失败；新增 config_scoped_overrides 4 项覆盖正反顺序、重复替换、null／标量／列表、任意深度、相邻前缀、get／get(app)／all／has、并发任务及退出作用域恢复 |
| 数组与列表操作 | 原点路径写入会丢弃数组兄弟项。复用现有 data_write，不扩展通配符；复用 Arr 的追加／前插处理 null 与关联数组，读取当前语言覆盖值后写回 | mixed-arrays-red.log 实际失败；数组下标、稀疏及前导零索引、push／prepend、错误后原值保持、关联键保留／整数键重排通过。push(null) 初始化列表，prepend(null) 报错；其他标量拒绝，不模拟 PHP 已弃用的 false 自动转数组 |
| 关闭浏览器识别 | APP_LOCALE=fr 时不能把应用语言直接当作文档语言。标题所属语言决定 HTML lang／Content-Language；文档语言依次使用已有标题语言、配置回退语言、英文，不改变 App 当前语言 | welcome-red.log 实际失败；无 SetLocale 的 fr/en、fr/zh_TW、正常中文、自定义法文目录及非三语回退通过；不存在 docs/fr 链接；缺失的个别词条仍独立回退 |
| 翻译助手与返回值 | 补齐 __()／__(null) 原生 null、翻译分组／列表值、@lang 第四参数关闭回退、空语言使用当前值、复数 count=null 使用实际计数。共享现有译器查找、缺键回调、替换与复数选择，不新增管理器 | blade-red.log 实际失败；blade_translations 6 项包含缺键回调上下文、原生分组／列表消费、替换、复数、转义／可信原样输出、区块嵌套、缓存模板、include 与组件、非法参数／缺失服务 |
| 事件顺序及错误 | App 按配置 → 解析并更新译器 → 解析并分发事件执行；同语言重复设置仍发事件，回退语言设置不发事件。监听器失败保留此前更新并传播原始错误；Source 提示改为翻译或语言设置失败，不误报为目录加载失败 | event-display-red.log、service-order-red.log 实际失败；服务绑定失效、监听器 yield 后失败、来源链、HTTP 500 与下一请求默认值恢复均通过 |
| 首次解析隔离 | 新增复现：首次翻译前先覆盖请求 app 配置，会把请求语言保存为常驻译器默认值。Provider 首次构造分别读取应用默认值和当前作用域值；复用 ScopeSnapshot 排除所属应用作用域，仅在默认配置读取期间生效 | lazy-translator-red.log 下一请求读取到 zh_TW 而非 en，真实失败；application_locale 7 项通过，含首次请求和任务解析、退出后默认值、并发请求与独立应用。没有提前解析全部服务或更改全部配置隔离模型 |
| 编译缓存与正文 | AST 字段新增后缓存版本更新为 37，旧版缓存失效断言同步；三语国际化和配置正文同步用法、作用域与迁移边界 | blade_arithmetic／blade_statements 的当前缓存和旧缓存回归；三语构建、两种部署前缀导航与桌面手机检查通过 |

### 固定来源与适配

在线读取固定 Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 Config/Repository、Collections/Arr、Foundation/Application、Foundation/helpers、Translation/Translator／TranslationServiceProvider、View 的 CompilesTranslations／ManagesTranslations 及对应 Contracts；核对 RepositoryTest、FoundationApplicationTest、TranslationTranslatorTest、BladeLangTest。来源清单、原文及 SHA-256 见本批 research/sources.json、more-sources.json、final-sources.json。最初两个错误的 Config 测试文件路径返回 404，后通过固定提交树定位并实际取得 tests/Config/RepositoryTest.php，不把 404 当作已核对。

关键上游入口：[Config](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Config/Repository.php)、[Application](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Application.php)、[Translator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/Translator.php)、[TranslationServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/TranslationServiceProvider.php)、[翻译编译](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Compilers/Concerns/CompilesTranslations.php)、[翻译区块](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/View/Concerns/ManagesTranslations.php)。另检查 v0.x 旧版配置实现，其结构不适合当前仓储，复用现有 data_write／Arr／ScopeSnapshot，未迁入旧管理层或引入新依赖。

具体适配与未实现必须分开：下划线语言归一是既有项目约定；请求／任务隔离和阻塞渲染语言快照是常驻 Rust 运行时所需；JSON 目录、Result／await、参数数量与类型校验、直接输出数组报错是明确适配。trans()／trans(null) 返回译器服务对象尚未在模板值模型实现，并非 Rust 无法表达；Rust 中可用 Lang。任意 PHP 函数、Facade 方法及命名参数仍未实现，不把本轮称为任意 PHP 执行器或全部翻译 API 等价。

### 最终验收与边界

证据目录 `target/tmp/welcome-locale-review-20261122/`；隔离源码和独占 Cargo 缓存复用 `target/tmp/welcome-locale-20261122/{source,cargo-target}/`。最终验收子进程 TMPDIR 均显式设为项目内目录；Rust 1.94.0，保留增量、默认并发及热缓存，沿用本工作流 dev/test debug=0、strip=symbols。本轮没有触及32 GB停线或清理缓存。

| 验收 | 最终结果及证据 |
| --- | --- |
| 配置、App、Blade 与支持函数 | final-framework.log／json：14 个目标、58 项通过 |
| 根应用配置、配置缓存、翻译、欢迎页与骨架 | final-root.log／json：5 个目标、27 项通过 |
| 具名欢迎页启动 | final-startup.log／json：精确运行 1 项通过 |
| 容器作用域 | final-scoped.log／json：scoped:: 14 项通过 |
| 合计 | 100 项通过、0 失败、0 ignored；不累计重复执行次数，不把过滤项计作通过 |
| 真实 HTTP 与浏览器 | 实际 Kernel 经随机回环 TCP 接入 Chromium；三语、禁用识别后英文／繁体回退、自定义法文共 6 场景 × 桌面／手机 × 明／暗 =24 组合通过。验证文案、HTML／响应头、Vary、文档和部署链接对应仓库文件存在、无页面异常或横向溢出；browser-results.json、browser.log、browser-server.log。人工检查禁用识别后的繁体手机截图，临时夹具已删除 |
| 固定 PHP 对照 | 既有 php:8.5.12-cli-alpine，无网络／只读挂载，26 项通过；php-golden.json。使用固定上游类与最小 trait／Str／Loader／容器夹具，非完整 PHPUnit；ASCII Str 夹具不证明 Unicode 全覆盖，容器自动回收 |
| 质量 | workspace／all-targets／locked 严格 Clippy、fmt --all --check 与 git diff --check 通过，final-clippy／final-fmt。第三方补丁依赖仍有既有 cfg／弃用诊断，不冒称全依赖零诊断 |
| 三语正文 | 六份正文同步；docs-results.json：构建含内容检查、根与 /docs/ 全导航范围检查、三语桌面手机检查全部通过 |
| 交付一致性 | closure.json 保存 HEAD、输入指纹和逐字节比较、测试统计、资源及 Git 状态；最终交付代码与本轮验收代码一致。工作区另有协作者的 locale-bcp47 计划及 full-parity 审计，本轮保留，不执行语言迁移 |

早期浏览器临时脚本只给 Cargo 子进程设置了 TMPDIR，外层 Node 仍继承系统临时路径；最终已为整个浏览器进程树设置项目内 TMPDIR，并重跑24组合，浏览器调试日志确认 profile／artifacts 使用项目内路径且退出时清除；系统临时目录未发现近期相关残留。过程中的夹具类型错误、测试目标拼写错误及一次嵌套作用域预期修正均不计为产品失败或通过。最初按顺序移动 App 服务解析后暴露了首次解析隔离回归，随后在 Provider 根因处修复，最终源码重新运行上述验收；不以中途快照或仅编译通过关闭问题。

未重建应用 Docker 镜像、未执行 Windows／优化 release 或本轮独立部署；浏览器为实际 Kernel 消费者，不冒充完整部署验收。任务语言隔离验证使用队列共用的容器生命周期，未新增真实队列 worker 专项。没有数据库／共享缓存驱动行为修改，未操作开发数据库、Redis 或数据卷，不把本轮当作三库／三缓存矩阵。完整 Laravel PHPUnit、真实部署平台及上述未实现的模板服务对象能力仍未验证或未实现。当前已实施修复没有未解除的外部阻塞。


## 欢迎页回退职责修正（2026-11-22）

基线 `02fdc4ce83cbe6514b3d8492a376006973b7dc19`。用户要求处理控制器中通过 `welcome.heading` 推断页面语言，以及手工选择文档语言的重复逻辑。本节覆盖历史记录中相应的页面语言与链接回退结论，其他历史验收不扩大为本轮结论。

### 修改及来源

- `WelcomeController::index` 仅读取当前语言、准备页面数据和响应元信息；删除 `Lang::hasForLocale` 探测、回退语言读取和文档语言列表。HTML `lang`／`Content-Language` 跟随当前应用语言，单个词条的回退不改变应用语言。
- 模板通过 `__('welcome.documentation_url')`／`__('welcome.deployment_url')` 消费三语 `welcome.json` 中的链接；沿用译器逐键回退及Blade转义。增加语言或定制链接不需要修改控制器，不新增管理器、框架配置、全局composer或单页专用回退接口。
- 当前语言和配置的回退语言都没有资源时，继续返回普通翻译键；不额外猜测英文可用。默认三语都提供完整链接，应用新增语言须提供资源或配置有效回退。三语国际化正文与项目规范同步说明。
- 延续本主题已核对的固定Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` [Translator](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/Translator.php) 的逐键回退职责及固定骨架 `06d016a364a37430eec9cbc52209adce3d7667ce` [welcome视图](https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/resources/views/welcome.blade.php) 读取当前应用语言的做法；链接本地化是本应用资源策略，并非上游默认欢迎页的原样功能。没有引入第三方路由本地化依赖。本轮收尾再次联网读取未获得可用正文，不把该次请求计作新增上游验证。

### 验收与边界

证据目录 `target/tmp/welcome-resource-links-20261122/`，源码隔离在其 `source/`；独占复用已空闲的 `target/tmp/welcome-locale-20261122/cargo-target/`。Rust1.94.0，保留增量及正常并发，进程树TMPDIR为本批项目内目录。

- `tests.log`：欢迎页11项、startup28项，共39项通过，0失败／0 ignored。新增正反例验证部分法文翻译不依赖标题、单独缺失链接按键回退、双语言缺键不额外兜底、URL中的 `&` 正常转义；原并发语言隔离及浏览器协商测试继续通过。
- `browser-results.json`／`browser.log`／`browser-server.log`：真实Kernel经回环HTTP接入Chromium，9场景×桌面手机×明暗=36组合通过。验证标题、HTML及HTTP语言、Vary、实际文档路径、无横向溢出／脚本错误、名称版本间距；人工检查关闭识别、法文当前语言／英文回退的手机深色截图。临时服务正常退出，夹具仅在隔离快照生成，退出后移除。
- `clippy.log`：根应用main及welcome／startup目标执行locked严格Clippy通过；`fmt.log`：fmt --all --check通过。第三方补丁依赖仍有既有cfg诊断，不宣称全依赖零诊断或本轮运行了全workspace Clippy。
- `docs-build.log`／`docs-browser.log`／`docs-scope.log`：三语内容检查、静态站构建、根与 `/docs/` 两前缀的三语桌面手机12组合、全部导航路径及显式支持范围检查通过。复用已有Node依赖，构建产物位于隔离源码内。
- `input-comparison.json`：9份代码／资源／测试／文档验收输入与交付文件逐字节相同；`results.json`记录各阶段命令、退出码及耗时。
- 未变更框架译器、共享数据库／缓存驱动、队列、Provider或请求语言选择策略；没有执行三库／三缓存矩阵、Docker重建、Windows或正式部署验收。现有协作者todo及全量审计／计划保留。验收结束后，用户明确授权将本轮代码、测试、三语文档及项目记录本地提交，并将解决方案、建议和验收结果发送主对话框；不包含推送。

## 手机端名称与版本间距修复（2026-11-22）

手机端原先使用固定宽高比容器和绝对定位版本图形，应用名称换行时会与版本重叠；真实 HTTP Kernel 接入 Chromium，在 320px 视口复现两者边界间距为 -45.89px。`resources/views/welcome.blade.html` 沿用桌面端的纵向 Flex 排版，将名称与版本纳入正常文档流，统一保留 16px 间距；移除手机端固定宽高比，容器随内容撑开，保留版本叠层效果。

- 失败与修复证据：`target/tmp/welcome-mobile-spacing-20261122/{red-results.json,green-results.json,browser-results.json}`；320／360／390／430／768／1024／1440px × 三语 × 明暗模式，共42组合通过，边界间距均为16px，无版本裁切、横向溢出或页面脚本错误。人工检查390px简体浅色及1440px英文深色截图。
- Rust 1.94.0 隔离源码及独占构建目录复用既有 `target/tmp/welcome-locale-20261122/`；欢迎页8项HTTP测试全部通过，0失败／0 ignored，见 `target/tmp/welcome-locale-review-20261122/mobile-welcome.log`。交付模板与验收模板逐字节一致，临时浏览器夹具已移除，`git diff --check`通过。
- 本次仅调整页面样式，无开发者API或用法变化；未修改三语正文，未操作数据库。用户随后授权本地提交手机端间距修复及本节记录；不包含推送，其他在途改动保留。

## 全链路设计与复用审计（2026-11-22）

审计基线为 `8277bdb4a4d8a5156780ea8fc959a140d7bc56c6`，包含已提交的逐键回退与 Controller trait。用户本次要求审计，不包含实施修复、暂存、提交或推送。本节不覆盖历史验收事实；下列新发现仍未修复。

结论：欢迎页当前分层基本符合固定 Laravel 契约与项目既有决定。继续沿用路由→应用语言中间件→控制器准备数据→Blade→同一译器／Loader 的调用链。未发现为该页新增平行语言管理器、全局页面 composer 或重复翻译字典的必要。发现两处功能边界及两组规范／简化建议，不能以现有测试通过认定全部边界已经闭合。

### 在线调研与取舍

本次通过 HTTPS 实际下载固定源码及生态项目正文，参考材料位于 `target/tmp/welcome-full-audit-20261122/research/`；来源、响应及内容摘要见该目录的 `sources.json`、`ecosystem-sources.json` 和补充来源记录。固定版本和本次读取的动态分支分开记录，不把 GitHub 热度或文档描述作为兼容性验收。

| 来源 | 本次核对内容 | 采用判断 |
| --- | --- | --- |
| 固定[应用骨架](https://github.com/laravel/laravel/tree/06d016a364a37430eec9cbc52209adce3d7667ce)：routes/web.php、bootstrap/app.php、Controller.php、welcome.blade.php | 默认路由返回视图；中间件扩展点；空基础控制器；当前应用语言及配置名称进入模板 | 保持既有装配。独立 WelcomeController 和名称／版本数据由用户要求；trait、Result、请求参数及 .blade.html 是 Rust 适配，不回退成 PHP 执行器 |
| 固定[框架](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc)：Application、Translator、helpers、CompilesTranslations、TranslationTranslatorTest | 配置→译器→LocaleUpdated；配置回退；缺键；__／trans／trans_choice；@lang／@choice 的输出方式 | 文案与本地化链接继续逐键翻译，不恢复控制器标题探测或额外英文兜底 |
| 固定[本地化教程](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md) | locale／fallback_locale、App 语言接口、目录及翻译助手 | 浏览器识别是本应用显式策略。BCP 47 及 en／zh-Hans／zh-Hant 是用户确认的标准调整，不以教程的旧目录示例否定迁移 |
| [Symfony HttpFoundation Request](https://github.com/symfony/http-foundation/blob/7.4/Request.php) | getLanguages／getPreferredLanguage、语言组合匹配 | 仅作为协商机制对照。当前框架 Request 未提供这两个接口；应用中文别名、权重和回退策略不声称与 Symfony 完整等价，不直接替换现有三语行为 |
| [mcamara/laravel-localization](https://github.com/mcamara/laravel-localization)、[niels-numbers/laravel-localizer](https://github.com/niels-numbers/laravel-localizer)、[codezero-be/laravel-localized-routes](https://github.com/codezero-be/laravel-localized-routes) | 浏览器检测、语言路由、重定向、Cookie／Session、路由缓存职责 | 当前欢迎页只需同一路径的三语响应，不引入这些完整路由机制。mcamara 本次读取的 README 已说明其动态路由及缓存边界，并推荐 successor；不能按知名度直接搬入 |
| [spatie/laravel-translatable](https://github.com/spatie/laravel-translatable) | 模型 JSON 属性翻译职责 | 不用于替代界面词条译器，欢迎页不需要模型／数据库翻译依赖 |
| 固定[Goravel Translator](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/translation/translator.go) | 文件加载、上下文语言与回退 | 仅参考编译型框架职责划分，不引入另一套语言状态或覆盖固定 Laravel 行为 |

### 覆盖范围及核对结论

| 范围 | 核对结果 |
| --- | --- |
| routes/web.rs、bootstrap/app.rs、默认 Provider | 路由引用 WelcomeController；SetLocale 显式 append；未把欢迎页翻译或可选扩展装进默认 Provider。Laravel 默认骨架没有该浏览器策略，现有三语说明已区分 |
| 控制器、config/app.rs、.env.example | 名称和版本从现有 Config 读取；控制器只传页面数据和语言元信息。基础 trait 为用户已确认的 Rust 适配，不增加路由约束或平行控制器机制 |
| SetLocale 与 HTTP 元信息 | 保留请求级语言设置、中文文字优先规则、权重、同权顺序、下游主动切换、Content-Language 与多值 Vary 合并。发现数字变体误判，见 W02 |
| App／LocaleUpdated／作用域 Config | 顺序、监听器失败传播、重复设置事件、深层覆盖和跨请求／应用隔离由实际回归覆盖；未发现需要欢迎页专用配置仓储的理由 |
| Translator／FileLoader／缓存／复数 | 复用同一规范化入口、精确资源查找、配置回退与缓存；地区和文字不混同。请求／任务状态及阻塞渲染快照服务于 Rust 并发隔离，不应为模仿 PHP 改成进程全局语言 |
| Blade 编译／渲染 | __／trans／trans_choice 与翻译指令调用同一译器；普通 echo 转义，指令／raw echo 的可信 HTML 边界已写正文；缓存模板、子模板、组件、复数、分组及区块由定向测试覆盖 |
| 模板／语言资源／页面布局 | 本地化链接在语言资源内逐键回退，HTML lang 使用应用当前语言。默认三语布局与间距正常；长自定义名称裁切见 W01；普通文案与缓存 meta 建议见 W03／W04 |
| 三语正文／文档目录／部署资源 | 当前文档和资源使用 en／zh-Hans／zh-Hant；本地链接目标存在。Dockerfile 复制 resources／lang，沿用已交付部署机制；本轮不重新执行镜像或独立部署大矩阵 |

### 新发现与最小处理方案

| 编号 | 优先级与状态 | 证据、影响及建议 |
| --- | --- | --- |
| W01 | P2，已复现，未修复 | `resources/views/welcome.blade.html` 的 .wordmark 没有长词换行，父 .artwork 使用 overflow:hidden。隔离真实 Kernel 将 app.name 设为 InternationalizationDemonstrationApplication，390px Chromium 手机视口测得 clientWidth=335、scrollWidth=1210，名称右侧被裁切；页面本身没有横向滚动不能证明文字完整。使用现有 CSS 允许长名称换行，维持名称／版本 16px 间距，并补长英文／中文名称、窄屏及桌面回归；不需要文字测量脚本或布局库。见 long-name-result.json、long-name-mobile.png |
| W02 | P3，后续已修复，见下节 Request 拆分验收 | `app/http/middleware/set_locale.rs` 第112行仅凭长度4查找 script，将框架当前接受的 zh-1901／zh-CN-1901 的四位数字变体当成未知文字体系，最终选为 en。这里确认的是当前 ICU 入口接受的良构标签，不声称该变体组合经过 IANA Prefix 有效性验证。对照 zh-pinyin／zh-CN 正常选择 zh-Hans；新的期望回归实际失败，见 numeric-variant-red.log 和 policy-observations.json。最小修正是在已有规范化结果上按 script 的字母语法识别，或复用可用的语义组件，不重新实现完整标签解析，不改中文别名策略 |
| W03 | P3，规范改进，未修改 | 三语 lang/*/welcome.json 的 description 仍主动用 Laravel 介绍默认产品，与当前“以 OpenRust 为主体”的规范不一致；translation/mod.rs 的 InvalidKey／InvalidCount／Unavailable 等未国际化人类错误提示也仍为英文。普通介绍和人类提示可按既有规范同步三语／简体中文；原 MIT 许可、上游来源及协议／机器契约必须保留，不做全局品牌替换 |
| W04 | P3，简化建议，未修改 | welcome.blade.html 第31–33行用三个 http-equiv meta 声明禁缓存，真实响应未包含 Cache-Control；这些页面内容不能代替 HTTP 缓存头，也不是固定欢迎页必需内容。建议删除遗留 meta；只有明确需要 HTTP 缓存策略时，才通过现有 Response 设置响应头。保留按语言变化响应已有的 Vary，不新增缓存中间件。见 policy-observations.json |

另外观察到 `en;q=0, *;q=1` 仍可经 wildcard 选择默认 en。当前项目已明确把浏览器匹配作为应用策略，因此本轮将其列为**待裁决的通配符／排除语义**，不混同完整 RFC 4647 协商缺陷，也不在未确认需求时改动默认规则。若后续要求明确排除优先于通配符，应先补双方期望并修改应用策略。通用 Request 的 getLanguages／getPreferredLanguage 属于 API 候选，未在本轮实现或宣称对齐。

### 本轮实际验证与未验证范围

证据目录为 `target/tmp/welcome-full-audit-20261122/`，源码在其 source/；Rust 1.94.0，复用独占 `target/tmp/welcome-locale-20261122/cargo-target/`，子进程 TMPDIR 在本批目录。全部新探针仅在隔离源码生成，不写入交付源码。

- root-tests.log：welcome11、startup28、localization13、configuration2、configuration_cache2，共56项通过，0失败／0 ignored。
- framework-tests.log：application_locale7、blade_translations6、locale_bcp476、config_scoped_overrides4、translation_choice1，共24项通过，0失败／0 ignored。合计80项不是完整框架全部 API 的完成结论。
- render-failure-recovery.log：新增真实 Kernel 回归验证无效模板返回500时仍保留语言及 Vary，恢复模板后下一英文请求正常，通过。
- numeric-variant-red.log：新增 W02 回归失败，保留为未修复证据，不计通过。policy-observations 首次因探针输出变量名不一致失败，修正隔离调用环境后 retry 通过；这是验收脚本问题，不记为产品缺陷。
- browser-results.json：实际回环 HTTP／Kernel＋Chromium 的原9场景×桌面手机×明暗=36组合通过；另一个长名手机场景实测并截图确认 W01，不能计作布局验收通过。临时服务正常退出。
- fmt.log／clippy.log：全项目 fmt、main与welcome／startup／localization定向严格Clippy命令退出0；依赖仍输出既有警告，不称全依赖零诊断或全workspace Clippy通过。
- 三语内容检查通过；8个实际源码／更新日志／三语文档及部署链接HTTP请求返回200，记录在 external-links.json。未将此扩展为正式托管、远端提交一致或全部外链内容验收。本轮未重新构建文档站，前节构建与导航记录仍仅代表前节源码。
- 未变更共享数据库／缓存／队列驱动，未启动三库／三缓存矩阵，未操作开发数据／数据卷；Docker、Windows及发布目标未重验。
- 复核期间出现协作者的 API throttle 中间件及三语说明改动，已审阅并同步这4个文件至隔离快照，重新完成上述80项、fmt及定向Clippy；首次结果保留为 initial-*，最终输入见 final-inputs.json／final-source-comparison.json。浏览器36组合及新探针来自同步前快照，欢迎页／语言／模板输入未变，不能据此宣称 API 限流专项通过；新增 api_throttling 专项仍由其工作流验收。本轮未修改或提交协作者代码。

`trans()`／`trans(null)` 返回译器服务对象、模板中的任意 PHP 函数／Facade 调用和命名参数仍未实现，三语正文已说明。当前模板值模型不暴露服务对象是实现边界，不能概括成 Rust 天生无法表达；它不影响默认欢迎页的实际调用，也不表示所有教程中的 PHP 模板代码都能直接运行。JSON 分组资源、异步事件及请求／任务隔离则属于已说明并验收的 Rust 适配。

收尾曾观察到磁盘低于32 GB；同期 API throttle 工作流已执行项目停线清理，记录在 `target/tmp/api-throttle-20261122/disk-cleanup.json`，实际净释放约61.40 GB。本轮核对登记的源码／日志仍保留、编译目录被清空及锁 inode 保留；恢复后有工作流重新持锁构建，保持其占用，不重复删除。资源复查见 disk-cleanup-confirmation.json；本轮额外删除0，未因审计清理开发数据或独立交付物。

2026-11-22 用户随后授权将本轮审计记录暂存并本地提交；不包含上述问题修复、协作者在途实现或推送。


## Request 语言协商与应用策略拆分（2026-11-22）

用户授权补齐通用请求语言能力，并让应用中间件复用。验收基线 `8277bdb4a`；开始时保留了欢迎页审计、API throttle、三语说明及全量审计计划的并行改动。期间协作者提交了 `8c5e5de40`／`3c91d93b7`，本轮未暂存、提交、推送或转发其他任务。下述结果覆盖前节“通用Request接口未实现”和W02未修复的旧结论，不关闭W01／W03／W04。

### 实现与边界

- 框架在 `http/request/languages.rs` 提供 `getLanguages()` 与 `getPreferredLanguage(Option<&[&str]>)`；按完整标签、语言／文字／地区组合、基础语言匹配。候选为空时返回首选范围，无匹配时返回首个候选；匹配不读取或修改应用语言。每个Request持有独立OnceLock，`parts_mut()`使语言缓存失效。
- `getLanguages()` 接管原应用的严格请求头解析与整数权重排序；应用 `SetLocale` 删除重复解析及quality函数，只保留三语资源映射、默认语言、App设置与响应头。注册继续位于 `bootstrap/app.rs`，不向框架 `foundation/configuration/middleware.rs` 内置三语或自动装配应用中间件，不新增管理器或依赖。
- W02：文字体系识别要求四位字母，数字变体不再误判。`zh-1901`／`zh-CN-1901`选择简体，`zh-TW-1901`选择繁体，显式未知文字体系仍不按地区猜测。原 `numeric-variant-red.log` 失败证据保留，本轮welcome测试及44组浏览器验收覆盖修复。
- 六份三语请求／本地化正文说明公开调用、候选错误、通配符／缺失回退及应用职责；项目规范增加框架解析／应用策略的边界。保留Symfony MIT声明及发行notices。

### 固定参考与明确差异

联网实际读取固定Laravel Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [Request](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Request.php) 与 [composer.json](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/composer.json)，确认继承Symfony Request及允许7.4／8.0依赖；读取HttpFoundation [v7.4.0 Request及测试](https://github.com/symfony/http-foundation/tree/v7.4.0)、[v8.0.0 Request](https://github.com/symfony/http-foundation/tree/v8.0.0)、AcceptHeader／Item／HeaderUtils及MIT许可。来源、SHA-256和完整源码在本批 `research/`；参考既有mcamara及Fluent调研，当前没有语言路由、持久化偏好或新增协商依赖的需要。检查同级 `openrust-v0.x` HTTP源码及本地v0.x分支，未找到可直接复用的这两个Request接口；复用当前应用已有qvalue规则和框架ICU规范化。

本轮不是Symfony语言API逐字节等价移植。以下是为了遵守既有BCP47规范及保留应用已验行为作出的明确适配，不称为Rust无法表达PHP行为：

| 范围 | 本项目行为及原因 |
| --- | --- |
| 语言标签 | 复用ICU输出BCP47，保留支持的变体及扩展；不输出下划线、不静默截断非法标签。非法请求条目忽略，非法候选返回TranslationError；Rust返回Result<Option<String>>及借用的语言切片 |
| 请求权重 | 保留既有浏览器策略：只接受0～1且最多三位小数，跳过非法／重复参数和q=0。上游会保留q=0并宽松转数值；实际PHP差异探针已保存，不能称为完全等价 |
| 多值及重复项 | 合并所有Accept-Language头，稳定排序，规范化后去重保留最高有效权重；上游HeaderBag默认读取首个头，重复原值由AcceptHeader覆盖。保留已有应用的多值／最高权重行为 |
| 通配符与回退 | `*`选择首个候选，应用仍以当前默认语言解释通配符；q=0不是永久排除清单，无匹配仍有默认值，不自动返回406。保留原策略，未偷偷增加新的排除语义 |
| 通用匹配 | 使用完整子标签边界，不混同文字与地区、不内置中文地区推断；中文资源映射留在应用。完整合法标签优先于组件组合，公开标签保留扩展 |

### 实际验收

证据在 `target/tmp/request-languages-20261122/`；源码隔离在本批 `source/`，复用空闲独占 `target/tmp/welcome-locale-20261122/cargo-target/`。Rust1.94.0，正常并发、增量保留，所有子进程TMPDIR在本批项目内目录；未触及32GB停线。

| 验收 | 结果与证据 |
| --- | --- |
| 框架定向 | `framework-final.log`：request_languages11、locale_bcp476、application_locale7、laravel_alignment_http7、request_query3，共34项通过；包括真实Kernel消费者、非法输入、候选校验、缓存失效与应用语言不变 |
| 应用消费 | `root-final.log`：welcome11、startup28、localization13，共52项通过；合计86项，0失败／0 ignored。原同权、多头、非法输入、中文策略、并发隔离及下游切换继续通过 |
| 上游运行对照 | 已有php:8.5.12-cli-alpine容器，无网络／只读挂载执行实际HttpFoundation组件；7.4与8.0各21个原始preferred-language用例通过，见php-golden*.json。Rust运行21个经BCP47标识适配的消费者场景：20个匹配原结果，1个上游含超长Unicode子标签的条目按现有校验整体跳过；差异明确记录而非宣称等价，见rust-upstream-results.json／rust-upstream.log |
| 实际浏览器 | 实际Kernel经回环HTTP接入Chromium，11场景×桌面手机×明暗=44组合通过；新增数字变体场景，核对文案、HTML／HTTP语言、Vary、文档链接、名称版本间距、无脚本错误／横向溢出。人工检查数字变体简体手机浅色截图。browser-results.json／browser.log；服务正常退出，临时夹具移除 |
| 质量 | 全workspace／all-targets／locked严格Clippy、fmt --all --check通过；clippy.log／fmt.log。依赖仍输出既有cfg／弃用诊断，不宣称全依赖零诊断 |
| 三语文档 | 内容检查、静态构建、根与/docs/全部导航及三语桌面手机12组合通过；docs-build／docs-browser／docs-scope.log |

运行材料、可复用缓存保留；没有操作开发数据库、Redis或数据卷。未更改共享数据库／缓存／队列引擎，未执行无关三库／三缓存矩阵；没有重建Docker应用镜像、执行Windows／正式部署或完整PHPUnit。这些不计本轮通过。最终输入对账与空间记录见本批closure.json；用户随后授权本轮代码与文档本地提交，并向主对话框发送实现、验收及后续建议；不推送。


### 框架中间件建议与主对话交接

2026-11-22 用户要求参考 [Laravel 13 中间件文章](https://learnku.com/docs/laravel/13.x/middleware/17061)，希望语言中间件纳入框架提供的能力。实际读取文章及固定 Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 `Foundation/Configuration/Middleware.php`：文章区分内置中间件、应用自定义中间件和全局注册；文章与固定源码的默认全局／web／api 栈均未提供自动语言选择，不能把此建议描述为 Laravel 默认行为。参考源码保存在本批 `research/learnku-middleware.html` 与 `research/laravel-middleware.php`。

建议主对话后续评估由框架提供通用、可选的语言中间件，复用本轮 Request API 与既有请求语言作用域，允许应用明确给出资源候选及映射策略；三语及中文地区映射不能硬编码进框架。中间件实现放在框架 HTTP 中间件模块，`foundation/configuration/middleware.rs` 只负责装配，应用在 `bootstrap/app.rs` 显式启用。新增可选能力与默认自动启用必须分开决策，记录相对固定 Laravel 的扩展边界。本轮仅完成 Request 能力与应用复用，尚未实现框架 SetLocale，不能将该建议计为已完成。
