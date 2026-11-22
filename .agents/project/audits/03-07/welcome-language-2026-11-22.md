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
