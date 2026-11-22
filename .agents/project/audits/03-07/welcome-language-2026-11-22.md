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


## 默认全局语言中间件迁移与关联复核（2026-11-22）

### 用户决定与范围

基线 `8e2810911`。用户明确要求整个 SetLocale 进入框架默认全局栈，覆盖前节“可选框架能力、应用策略注册”的建议；不是只搬响应头或保留应用策略回调。固定 Laravel Framework v13.40.0 的默认栈不含语言识别，本轮是用户确认的 OpenRust 默认扩展，不声称与上游默认栈相同。

重新联网读取固定 [Middleware.php](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/Middleware.php)、[中间件教程](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/middleware.md)和[本地化教程](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md)，并参考 mcamara/laravel-localization 的中间件职责。原始材料在本批 `research/`；采用现有 Request 解析、ICU、App::setLocale、请求作用域和中间件链，不引入路由重定向、Cookie、Session依赖或新语言管理器。本地 v0.x 翻译器／测试的架构不同，没有可直接复用的默认中间件；迁移当前已验实现。

### 实际发现及处理

| 关联位置 | 核对与处理 |
| --- | --- |
| 应用骨架 | 迁移中误删空入口的做法已纠正。保留 app/http/middleware/mod.rs、父模块声明、bootstrap/app.rs 空 withMiddleware 闭包；对照引入 SetLocale 前的 d4a40d7f4^，bootstrap 恢复原样。仅删除已迁移的应用 set_locale.rs、声明／导出及手动 append；应用目录可继续放业务中间件 |
| 框架完整实现 | http/middleware/set_locale.rs 保留原三语策略及头合并，公开导出；配置类只保存候选实例；默认在输入转换后、应用 append 之前装配。候选规范化、错误原子保留，空列表关闭；可复用实例用于路由，无应用回调和全局可变状态 |
| 最小应用 | 默认接入后，缺少 app.locale 的最小应用原本返回500；中间件现在使用译器已有英文默认，并允许浏览器选择。原 Blade 测试不改断言通过 |
| 下游配置替换 | Config::set("app", {}) 是合法操作，响应阶段读取被移除的 app.locale 原本把成功响应变500；现在沿用本请求选择语言。原 Config 测试不改断言通过，新增实际Kernel回归 |
| 默认语言与事件 | application_locale 和根 localization 中只验证配置语言／手动切换的夹具显式关闭浏览器识别，保留原精确地区与事件断言；默认启用、关闭、监听器失败与恢复、请求／应用并发由新测试单独覆盖 |
| CORS 与生命周期 | 新默认中间件会给经过它的响应增加 Vary: Accept-Language；CORS测试保留原Origin／Accept-Encoding断言，另外精确核对新增值和头数量，不将新增头误判为CORS未关闭。CORS预检、维护及大小检查的提前响应仍不进入语言选择 |
| 旧引用与生成路径 | 检查 app、bootstrap、根测试、框架默认栈、安装器、生成器、Docker资源复制和三语正文；移除现行应用注册引用。控制器、视图、路由、AppServiceProvider、Config仓储及译器未因本次迁移改变。历史审计保留原事实，由本节覆盖旧建议 |
| 正文与规范 | 三语本地化说明默认行为、候选、关闭、旧注册升级、事件及早返回边界；请求和中间件章节链接至同一正文。规范删除旧显式启用要求，新增迁移时保留骨架入口的约束 |

### 最终验收

Rust 1.94.0，所有临时源码、参考、日志、测试文件和浏览器材料在 `target/tmp/set-locale-middleware-20261122/`，独占复用 `target/tmp/welcome-locale-20261122/cargo-target/`；保留增量和正常并发。各阶段运行前检查空间，未触及32GB停线。以下均针对本轮交付源码，而非前节快照。

| 验收 | 实际结果与证据 |
| --- | --- |
| 配置、语言与默认接入 | `framework.log`：application_locale7、blade_translations6、config_scoped_overrides4、laravel_alignment_http7、locale_bcp476、request_languages11、set_locale_middleware8，共49项通过，0 ignored。新测试使用真实Kernel，覆盖默认自动接入、空候选／路由复用、非法原子保留、事件顺序／失败、最小配置、父键替换、404／500恢复、最终响应头及并发应用隔离 |
| 欢迎页与启动 | `root.log`：application_skeleton2、localization13、startup28、welcome11，共54项通过，0 ignored；与上一行合计103项。所有旧语言、Config及Blade行为断言保留；默认欢迎页不手动注册应用中间件 |
| 扩大关联检查 | `http-impact.log`／`app-impact.log`／`precognition-http.log`：CORS生命周期5、排除1、HTTP观察4、请求属性3、视图11、实际HTTP验证19、无外部服务Session12、SQLite数据库Worker／时长钩子2、根HTTP16、初始化14、文件维护2、Precognition7，共96项实际执行通过；Worker子进程夹具不重复计数。日志另外显示11项缺少服务环境后直接返回的“ok”，全部排除：Session MySQL／Redis5、验证MySQL1、Worker MySQL／Redis／Cluster／SQLServer4、维护Redis1。ignored的Worker入口由SQLite父测试实际派生执行；Precognition官方前端ignored不计通过，MySQL写入专项明确筛除1项 |
| 负向接入校验 | 仅在隔离快照临时移除默认栈接入，`negative-default-registration.log` 实际断言失败（英文而非简中），退出101；恢复原源码后8项全部通过，`restored-default-registration.log`。这是接入点负向检验，未将编译缺符号当作行为失败证据，交付源码不受临时操作影响 |
| 真实浏览器 | `welcome-browser.log`／`browser-results.json`：实际Kernel回环HTTP＋Chromium，11场景×手机桌面×明暗=44组合通过。覆盖默认三语、显式文字／数字变体／未知文字、关闭识别与法文两种回退／部分资源；检查HTML和HTTP语言、Vary、真实存在的文档／部署路径、名称版本间距、无横溢出及脚本错误；人工检查简中手机浅色截图。服务正常退出，临时Rust浏览器夹具移除 |
| 质量与三语站点 | `fmt.log`、`clippy.log`：全workspace fmt、workspace/all-targets/locked严格Clippy通过；`docs-build.log`：三语内容检查与构建通过；`docs-browser.log`：根和/docs/三语桌面手机12组合通过；`docs-scope.log`：两前缀全部导航通过。第三方依赖仍有既有cfg／弃用警告，不称全依赖零诊断 |

首次复验先后暴露最小配置和父配置替换500、默认识别改变精确配置语言、CORS新增Vary预期及缺少MySQL验收环境。前两项修实现，精确语言夹具只显式关闭浏览器识别且保留断言，CORS按新默认头增加断言；缺服务明确记未验。早期公开API夹具另有编译错误，不能作为有效行为失败；完整尝试退出码见本批 `results.jsonl`。本轮补充的配置、视图、Kernel、页面和生命周期回归已通过，不凭关键词消失关闭问题。

本轮没有改变共享数据库、缓存或队列引擎，未操作开发／生产数据及卷；未复验PostgreSQL／MySQL、Redis及Cluster专项、官方Precognition前端、Windows、Docker镜像、独立发布部署或完整上游PHPUnit，不将它们计为通过。默认语言接入只在HTTP Kernel，Console／Worker不自动装配HTTP中间件；SQLite实际Worker及既有语言作用域测试继续通过。最终源码与快照对账见本批 `closure.json`，仅内部验收记录在结果补录后同步；验收结束时HEAD为 `8e2810911`，索引为空。2026-11-22 用户随后授权本轮代码、测试、三语文档及项目记录暂存和本地提交，不包含推送或发布；提交前再次核对3,738项验收输入一致，仅补录本段及当前交接的Git授权。协作者的todo与framework-parity计划保留，不纳入本轮提交。

## SetLocale 实例级语言选择回调（2026-11-22）

### 范围与调研

基线 `b214e46be`。保留已完成的默认全局中间件、三语浏览器策略和应用骨架，只扩展现有 SetLocale 实例及配置入口。独立材料统一位于 `target/tmp/set-locale-callback-20261122/`；本次没有暂存、提交或推送授权。

设计前联网读取下列原始实现，下载内容保留在本批 `research/`，没有把实时分支名称作为固定版本：

| 来源 | 核对结果与采用方式 |
| --- | --- |
| [Laravel 本地化教程](https://laravel.com/docs/13.x/localization)及[固定文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md) | 语言由应用规则确定，通过 App::setLocale 设置；保持既有配置、译器与事件链。教程没有 localeUsing，本次是 OpenRust 扩展 |
| [Authenticate::redirectUsing](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Middleware/Authenticate.php) | 借鉴 Request 入参、Using 命名及可选回调；不复刻 PHP 静态属性，使用项目已有 Arc＋Send＋Sync 实例约定，避免跨应用状态 |
| [Symfony LocaleListener v7.4.0](https://github.com/symfony/http-kernel/blob/v7.4.0/EventListener/LocaleListener.php) | 显式请求来源优先，未提供时才协商；读取路由属性必须在路由之后，不能通过回调绕过执行顺序。保留本项目已确认的中文匹配策略，不声称完整 Symfony 等价 |
| [mcamara/laravel-localization](https://github.com/mcamara/laravel-localization/tree/dbfa563ea0ceb6d072adc280a8830999ade9ed74) | 核对显式候选、浏览器回退和路由／Session职责；不引入 URL 语言、重定向、持久化或 LC_TIME 全局切换。其不支持值回退行为不照搬，本轮用户要求显式错误 |
| [CodeZero Laravel Localizer](https://github.com/codezero-be/laravel-localizer/tree/013cab8762357f04ddfa4f62c5e836a9212c554f) | 核对 README、SetLocale 与配置：Session 之后、绑定之前的顺序有实际意义；采用职责划分，不引入其探测器、存储器与 trusted detector 机制 |
| 本地 v0.x | 检查旧 http/middleware.rs，依赖旧 locale_service 与 Request metadata，不具备本次实例回调契约；复用当前 SetLocale、Request、App 和现有回调模式 |

### 实际实现与决定

- `SetLocale::localeUsing` 与 `foundation::configuration::Middleware::localeUsing` 接受同步 `Fn(&Request) -> Result<Option<String>, HttpError> + Send + Sync + 'static`；闭包可捕获不可变应用规则。重复配置替换该实例回调，克隆后配置变更相互独立；闭包内部的共享可变用户状态仍须由应用避免。
- `Some` 复用 ICU 规范化，再精确核对候选成员；`zh-CN` 不因浏览器策略变成 `zh-Hans`，地区及支持的扩展必须完整出现在候选中。非法标签使用原 TranslationError，候选之外使用带中文说明的 InvalidInput 源错误，经既有 HttpError::Internal 处理；都不静默回退。用户输入需要4xx时可在回调里返回既有 HTTP 错误。
- `None` 才执行原浏览器协商；没有回调时原行为不变。`SetLocale::setLocale` 原子更新候选，配置类改为修改该实例字段而非整体替换，因此先后顺序不清除回调；验证失败保留原候选／回调。空候选沿用已确认的关闭语义，包含回调、语言、事件和响应头，仍保存回调以供重新启用。
- 同一个中间件仍负责 App::setLocale、事件及最终语言响应头。保持既有 Content-Language、多个 Vary 值、大小写不敏感去重及 `*`；继续添加 Accept-Language 以覆盖 None 的浏览器路径，不猜测自定义来源的缓存键。三语正文明确 X-Locale 需相应 Vary，Session／用户私有内容需 private／no-store 等应用缓存策略。
- 默认全局位置及应用文件不变；依赖 Session／认证须关闭全局，复用路由组实例安排顺序。同步回调无法 await，异步查询在前置中间件准备，不为假想需求新增异步协议。文档同时说明绑定前的规则需要显式组内排列，默认 web 后的路由实例不能提前影响已执行的绑定。
- 三语本地化正文同步全局回调及 Session 路由组示例、错误／回退、精确候选、空列表、实例隔离、执行顺序和缓存责任；设计和验收证据留在本主题记录。

### 验收记录

先以可编译的回调存储入口补失败回归，`red.log` 实际执行12项，8项原回归通过、4项行为断言失败，退出101：选定语言未生效、非法／不支持值未拒绝、回调错误被忽略、候选更新丢失回调。之后实现执行与原子更新，再扩充恢复、并发、精确地区／扩展及 Session 回归；没有把缺符号编译失败当作有效行为失败。

早期 Session 夹具先遇到异步闭包不能作为此处路由处理器的编译错误，改为项目既有函数处理器；其后无 Host 的请求导致 URL InvalidOrigin，补真实 Host 后通过，未修改生产代码绕过。完整阶段退出码在 results.jsonl，不将失败尝试计为通过。

最终验收采用 `b214e46be` 加本轮9份交付文件的 `source-final/` 隔离快照，不混入其他对话正在开发的 MessageBag、JsonSchema 等改动；它们完整保留在工作区，不能将本次结果扩展为这些在途修改也已通过。Rust 1.94.0、项目内 TMPDIR 和独占使用的复用 Cargo 缓存；曾遇另一工作流引用同缓存，等待 Cargo 锁并按实际冲突发出协调信息，没有并发改写构建目录。各阶段检查磁盘，未触及32GB停线，无清理数据或开发服务操作。

| 范围 | 验收结果与证据 |
| --- | --- |
| 框架与 Kernel | `delivered-framework.log`：application_locale7、blade_translations6、config_scoped_overrides4、laravel_alignment_http7、locale_bcp476、middleware_exclusions1、middleware_lifecycle5、request_languages11、set_locale_middleware19，共66项通过，0 ignored。19项中的11项为本轮新增；覆盖无回调、Some／None／错误、非法／不支持／精确地区扩展、两种配置顺序、失败原子保留、关闭及重启、克隆实例及多应用并发、下游切换和头保留去重、真实加密Session恢复与错误顺序。原错误源可由注册的异常渲染器识别并返回503，HTTP回调错误保留418状态 |
| 欢迎页与启动 | `delivered-root.log`：application_skeleton2、localization13、startup28、welcome11，共54项通过，0 ignored；与前行合计120项。不删除旧断言，不把缺环境返回算通过 |
| 三语示例 | `docs-examples.log`：从三语本地化正文逐份提取两段Rust示例，3项实际Kernel测试通过（全局X-Locale和默认web后的Session路由实例）；真实Cookie写入后请求/account，候选语言与响应头符合正文，缺值回退浏览器。临时夹具归档为本批doc-example-fixture.rs，未加入交付测试；正文后续仅增加扩展归属说明，示例代码保持一致 |
| 实际 HTTP／浏览器 | `welcome-browser.log`、`browser-results.json`：使用交付核心、当前欢迎页模板及真实Kernel回环HTTP，Chromium验收Some英文／简中／繁中和None浏览器回退4场景×桌面手机×明暗=16组合通过；核对HTML、文案、Content-Language、Vary、真实文档链接及页面无横溢出。另3种非法／不支持选择实际返回500，之后无选择请求恢复英文200。人工查看简中手机浅色截图。临时服务正常退出，夹具从验收源码移除 |
| 格式与静态质量 | `final-fmt.log`、`final-clippy.log`：cargo +1.94.0 fmt --all --check、workspace/all-targets/locked严格Clippy通过。依赖仍有既有cfg／弃用等警告，不声称全部第三方依赖零诊断 |
| 三语文档站 | `docs-final-build.log`：命令入口正反例2项、三语内容检查及构建通过；`docs-final-browser.log`：/与/docs/三语桌面手机12组合通过；`docs-final-scope.log`：两前缀全部三语导航通过。未调整文档站导航数量或支持状态 |

最终源码／快照对账、示例代码哈希、HEAD及空间实测见本批 `closure.json`。本次未改变共享数据库／缓存驱动、持久化模型或Worker，未启动无关数据库／Redis或操作开发数据；没有三库／缓存矩阵需求。尚未复验Windows、Docker镜像、独立发布部署、外部身份服务与完整上游PHPUnit，这些不计本轮通过。默认全局不能访问后续Session／认证，回调同步，应用负责自定义来源的共享缓存与私有内容策略，均为已说明的支持边界，不隐藏为无条件支持。本轮不暂存、提交、推送或发布。


## 仓库元信息与欢迎页链接调整（2026-11-22）

本节按用户最新决定覆盖此前“URL 通过语言资源逐键回退”的欢迎页约定；历史记录保留其当时语义。`Application::REPOSITORY` 为 OpenRust 扩展，紧邻 VERSION，直接使用框架 crate 的 `env!("CARGO_PKG_REPOSITORY")`。框架清单已有仓库字段，无新增访问方法、依赖或重复地址。控制器只增加 repository 数据，复用现有 currentLocale；模板用默认转义拼接文档、部署、源码及更新日志链接，页面路径仍归模板。

调研：联网核对 Cargo 官方 environment-variables.html 的 CARGO_PKG_REPOSITORY（值来自当前包清单），固定 Laravel 1449bf241017087fbd38c118ecaabef3f6797fdc 的 Foundation/Application.php 仅有 VERSION/version 对应职责，不声称存在同名仓库常量。沿用现有 Blade 转义与 HTTP Kernel 消费者，不引入管理器。旧版 welcome_controller 路径未提供可复用实现，本次复用当前页面调用链。

三语 welcome.json 仅保留显示文案，删除 URL 词条；应用、视图、资源、测试及开发者正文中无 documentation_url／deployment_url／documentation_path／deployment_path 剩余消费者。三语 localization 正文和项目规范同步。两处真实模板 console 夹具补 repository，保留原有 language。其他协作者的 MessageBag API 调整原样保留。

验收使用 Rust 1.94.0；临时文件、日志在 `target/tmp/welcome-repository-20261122/`，复用本流程空闲独占 `target/tmp/framework-version-20261122/cargo-target/`，设置项目内 TMPDIR，保留增量和正常并发。详细命令与退出码见 results.jsonl：

| 验证 | 实际结果 |
| --- | --- |
| 最新工作区隔离快照：`cargo +1.94.0 test --locked --test welcome --test console --test startup --test framework_version` | 48 项通过；welcome 12项含完整四链接、现有路径、文案缺键回退、转义、并发与非三语边界 |
| 同一最新快照：`cargo +1.94.0 test --locked -p openrust-framework --test application_version --test application_locale --test set_locale_middleware` | 29 项通过，含框架清单仓库来源／无尾斜线、现有回调、实例隔离和路由组顺序 |
| 真实 HTTP Kernel + Chromium | 44组合通过，三语／地区别名及关闭识别 × 桌面手机 × 明暗；四链接、HTML／HTTP语言、翻译、布局和无页面异常通过。默认三语路径存在，fr 路径不存在按预期单列；人工检查简中手机截图 |
| 范围隔离快照：`cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings` | 最终通过。排除其他未交付 WIP；不能扩展为全部协作者代码质量已通过。当前实际修改 Rust 文件另行 rustfmt --check 通过 |
| 三语文档 | 内容检查、npm --prefix docs run build、check_browser.mjs（12组合）、check_scope.mjs（根路径和/docs/三语全导航）通过 |
| 最终源码一致性 | 本轮12个交付源码／测试／正文文件与最新工作区验收快照逐字节一致；closure.json 记录范围。内部记录不参与运行时编译 |

初次文档构建因隔离快照未连接已安装 node_modules 失败，复用项目依赖后重跑通过。首次质量检查与一次性浏览器夹具写入碰撞，夹具包含未格式化/type_complexity代码；浏览器验收结束删除夹具后重跑 fmt／严格 Clippy 通过，不改生产实现掩盖失败。范围隔离旧 MessageBag 夹具临时适配旧 API，仅用于排除其他 WIP；随后最新工作区原样快照的77项测试通过，最终代码未恢复旧API。

边界：默认候选 en、zh-Hans、zh-Hant 对应真实文档目录。关闭识别并配置 APP_LOCALE=fr、自定义候选含fr或下游主动切换fr，均可使当前语言为fr；译文可按配置回退，但链接仍为docs/fr（目录不存在）。按本次要求不改变语言选择、不做链接语言回退；应用需要自行提供对应文档或定制模板。未操作开发数据，未重新验收Docker／Windows／release发布；本轮未授权暂存、提交或推送。


收尾磁盘检查触及32 GB停线；同时进行的 framework-parity 工作流已在无Cargo进程、取得排他锁并保留inode后完成本轮项目编译产物清理，包含本流程framework-version缓存及文档dist/cache。已核对并复用该清理结果，不重复删除IDE恢复后新建缓存；本轮净释放63.52 GB（59.15 GiB），清理证据见本批cleanup-observed.json及其引用的disk-cleanup-content.json，源码／日志／截图保留。随后复查可用空间超过90 GB。本轮全部测试和页面验收在清理前完成，产物清理不改交付源码。


2026-11-22 用户随后授权本轮本地提交，不包含推送。提交按文件片段隔离欢迎页仓库元信息改动；VERSION／version、SetLocale回调及其他协作者在途功能不纳入本次提交。仓库元信息回归只暂存对应测试函数，其余版本测试原样保留在工作区；MessageBag新API调整也保留未提交。上述77项属于当时完整源码组合的关联验证，不表述为其他在途功能已提交。


## URL 语言参数与欢迎页切换（2026-11-22）

### 范围与调研

执行 [已确认计划](../../plans/locale-query-and-welcome-switching.md)，实施起点 HEAD `2c3b8102e`。保留既有框架版本、SetLocale 回调、仓库元信息以及其他协作者的框架对齐改动；本次授权仅开发与验收，未暂存、提交、推送、发布或操作开发数据。

联网读取固定 Laravel Framework v13.40.0 / `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 [InteractsWithInput::query](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Concerns/InteractsWithInput.php)、[Request 的 query 合并／删除](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Request.php)、[Arr::query](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Arr.php) 、[HttpRequestTest 的 query与合并删除测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Http/HttpRequestTest.php) 和 [Authenticate::redirectUsing](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Middleware/Authenticate.php)。复用本项目 Request 查询集合、Arr 的 RFC3986 编码、Config 仓储和现有实例回调；不复制 PHP 静态回调状态，也不新增管理器。

核对 [Symfony LocaleListener v7.4.0](https://github.com/symfony/http-kernel/blob/v7.4.0/EventListener/LocaleListener.php) 的显式路由语言优先及路由后执行顺序；其 `_locale` 属性不是 query API。[mcamara](https://github.com/mcamara/laravel-localization) 的 Session／Cookie 重定向和 [CodeZero](https://github.com/codezero-be/laravel-localizer) 的有序探测、Session／绑定顺序用于界定职责，不整体引入持久化、路由前缀或探测器链。生态 README 按本日联网快照核对，不作为固定 Laravel 契约。旧版不存在可直接复用的同路径 SetLocale；采用当前主线现有机制。参考资料仅存在本批 `target/tmp/locale-query-20261122/research/`。默认全局 SetLocale、URL 参数与配置键均是用户确认的 OpenRust 扩展，不是 Laravel 默认功能。

### 实际实现与决策

- `config/app.rs` 相邻语言配置增加 `locale_query_parameter: "lang"`。缺键为 lang，JSON null只关闭 query；配置缓存仍由现有仓储加载，不逐请求扫描配置文件。
- 同一个 `SetLocale` 按 callback Some → query → 浏览器选择，经既有 `App::setLocale` 更新配置、译器与事件。Some 不读取低优先级语言值，None 继续 query；非法 callback 仍沿用开发错误契约。空候选早返回，连错误 query 配置也不读取。
- query 仅读 `Request::query`，不读请求体。ICU 规范化后须精确属于候选；空值、数组／对象、非法或不受支持标签返回400。`zh-CN` 不等于候选 `zh-Hans`；重复标量按原解析器后值覆盖前值。精确地区和 Unicode 扩展候选保留完整标签。
- 参数名必须为非空顶层键，拒绝空白、控制字符、点与方括号。这是既有 PHP 式解析及 Arr 点路径可往返的约束，不是 Laravel 标准。错误类型或名称走现有 ConfigError／异常处理，返回500。
- 最小请求扩展 `LocaleSelection` 提供实际参数名、候选及 from_query，欢迎页只据此生成显示链接，不再做选择。callback 已选择语言时隐藏 URL 控件，避免生成无法覆盖回调的入口。null、停用中间件及无三语候选同样隐藏；候选子集仅显示真实可用项。
- 控制器用 Arr::forget／set／query 保留其他参数，生成当前应用基路径下的首页 GET 链接；自动模式仅删除配置键。使用相对路径，无 Host 或重定向来源依赖。Blade 保留默认转义，三语资源仅增加显示文案。
- 控件区分显式／自动模式（边框）及当前语言（下划线），有可访问名称、键盘焦点和换行布局。没有 Cookie、Session、数据库或 localStorage 写入。原应用名称／版本布局及仓库链接方案保留。
- Content-Language 使用下游最终语言且保留既有值；Vary 维持去重行为。URL 不等于用户私有缓存隔离，仍保留现有自定义来源的缓存策略说明。

### 实际验收

Rust 1.94.0；隔离源码、独占 `cargo-target/`、日志、浏览器脚本和临时 fixture 都在 `target/tmp/locale-query-20261122/`，子进程使用本批 TMPDIR、RUSTUP_TOOLCHAIN 及 CARGO_TARGET_DIR。保留增量和默认并发；本批磁盘未触及32GB停线。为避免重复大型调试符号，本批测试／Clippy临时使用 dev/test debug=0 和 strip=symbols，不修改项目 profile。

| 范围 | 命令／消费者 | 实际结果 |
| --- | --- | --- |
| 应用消费者 | `cargo +1.94.0 test --locked --test welcome --test configuration --test localization --test startup --test console` | 65项通过，0 ignored；真实Kernel、模板、缓存模板、关闭／自定义参数、候选子集、callback覆盖与三语repository链接 |
| 框架回归 | `cargo +1.94.0 test --locked -p openrust-framework --test set_locale_middleware --test application_locale --test request_languages --test blade_translations --test config_scoped_overrides --test locale_bcp47 --test laravel_alignment_http` | 66项通过，0 ignored；其中SetLocale25项覆盖优先级、错误、配置缓存、事件、body隔离、并发及两应用隔离、路由组顺序和原响应头契约 |
| 负向对照 | 快照中关闭 query 选择，再运行 `query_is_exact_and_overrides_browser_with_one_event_per_request --exact` | 实际断言失败：预期 en，实际 zh-Hant；恢复源码后SetLocale25项通过，真实工作区未被注入故障 |
| 欢迎页浏览器 | 临时真实 HTTP Kernel listener＋Chromium，`browser.mjs` | 36组合通过：三种浏览器语言及6种配置变体 × 桌面／手机 × 明／暗。实际点击候选／自动、刷新、URL保留、安全编码、当前模式、键盘Enter、HTTP／HTML语言、四仓库链接与本地文件存在、无溢出／脚本错误；另4类非法query400。代表截图经人工检查 |
| Rust质量 | `cargo +1.94.0 fmt --all -- --check`；`cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings` | 范围快照全部通过；真实工作区本任务Rust文件另行rustfmt --check通过 |
| 三语正文 | `npm --prefix docs run build` | 最终正文内容检查、入口测试及VitePress构建通过 |
| 文档浏览器 | `node docs/tests/check_browser.mjs`、`node docs/tests/check_scope.mjs` | 根／docs双前缀、三语、桌面／手机、切换／搜索及全导航渲染通过；最终正文更新后重新构建及运行 |

范围快照为 HEAD 加本任务文件及既有回调配置文件；为排除并行 MessageBag API 改造，仅快照 `tests/welcome.rs` 的旧消费者采用 HEAD 的 first 调用，未更改真实文件。另使用包含当时全部WIP、且本任务文件字节一致的 `source-current/` 复验，结果见下方最终核对，不将范围快照结果扩大为其他协作者全部工作已完成。命令、退出码和磁盘实测见本批 `results.jsonl`，不将临时源码哈希清单或patch复制到长期审计。

首次fixture用闭包捕获配置参数，与既有ConfigLoader fn指针契约不符；改为仅测试使用的临时JSON fixture，未扩大生产加载器。首次欢迎页断言过度限定query字段顺序，以及用非法query测试仅fr候选下的隐藏控件，修正为原编码顺序与有效无参数请求，保留所有非法400回归。以上失败均保留日志，不能计作通过。

### 支持边界

默认三语所有仓库路径存在。应用主动增加 fr 等语言，或停用识别并设 APP_LOCALE=fr，仍可能产生 docs/fr 无效链接；本计划要求不改变应用语言或增加文档回退层，需应用提供对应文档或定制欢迎页。回调Some时隐藏控件属于既定优先级的直接结果。

本功能无数据库或缓存驱动变更，未启动无关数据库矩阵或操作开发数据。Windows、Docker／独立部署、本机物理移动设备、正式HTTPS和性能压测本轮未重验；现有结果仅覆盖macOS隔离HTTP／Chromium消费者，不声称吞吐或零开销。现有发布／真实环境待办保持独立。


初次失败与修正据实保留：

- 测试配置加载器是 fn 指针，最初测试闭包捕获参数导致编译错误。改为本批测试根目录的配置 fixture，通过既有 loader 读取；生产配置接口未改变。
- 首次欢迎页测试的两个失败是错误期望：强行要求query键顺序，以及仅fr候选时误认为非法query会被忽略。修正测试为验证实际链接字段和无query的控件隐藏场景；未放宽生产400契约。
- 范围测试通过后首次组合快照的Config HTTP回归3项在请求前因TestApp缺app.url而返回InvalidOrigin；当时其他协作者正在修改testing入口，工作区随后已有fallback修复。刷新后曾遇到在途TestResponse缺assertLocation的编译错误；未删除断言或将失败记为通过，待工作区新增response_metadata实现后再刷新复验。各批原始失败保留在current-framework.log、current-framework-revised.log及results.jsonl。

边界：默认三语链接分别对应已有目录；非三语语言（例如关闭识别并使用APP_LOCALE=fr，或候选包含fr且用户选择fr）仍可生成不存在的docs/fr链接。本轮按用户要求保留当前应用语言，不引入文档语言管理器或额外回退。未验性能、Windows、Docker、发布、远程正式文档站、真实移动设备及其他浏览器引擎；没有数据库／缓存共享行为变更，未启动无关服务。参考PHP源码与测试用于核对契约，未运行完整上游PHPUnit。


最终组合复验：重新从最新工作区取不可变快照，保留 MessageBag 新调用及所有当时在途实现，`current-framework-final` 66项、`current-root-final` 65项全部通过，均0 ignored。前述在途错误在更新后的快照中未再出现；未更改协作者的testing实现。交付范围18个配置／源码／测试／语言资源／三语正文文件与该组合快照逐字节相同（scope-consistency.json），真实文件单独rustfmt检查通过。范围快照仅用于隔离质量结果，最终组合验收不再替换MessageBag调用。此结论只覆盖实际运行的上述131项，不表示其他Agent的整个全仓对齐任务完成。


组合质量复验补充：新 testing 辅助代码将 Jiff Timestamp 转成 SystemTime 时使用了 `.try_into().expect(...)`，Rust1.94严格Clippy报 unnecessary_fallible_conversions；仅将该表达式及 testing/app.rs 捕获响应时间的同类转换改为等价 `.into()`，保留协作者其余实现并格式化这两个文件。后续一次单文件刷新快照使metadata与旧TestResponse字段不同步，出现response_time缺字段的快照编译错误；改为从工作区完整刷新不可变组合快照后重新验收，不将该快照错误归为交付代码的新缺陷。原始current-clippy、delivery-clippy与accepted各批失败日志均保留。本轮没有放宽lint、删除断言或计入任何失败／跳过项。


最终收尾：两处直接转换修正后，`final-accepted-clippy-framework` 与 `final-accepted-clippy-root` 的上述受影响lib／tests在 `-D warnings` 下通过；`final-accepted-tests-framework` 74项（原66项＋TestResponse消费者8项）和 `final-accepted-tests-root` 65项通过，均0 ignored，共139项。本任务18个配置／源码／测试／语言资源／三语正文文件与最后组合快照仍逐字节一致；testing/app.rs与快照相同，response_metadata.rs本轮修改的转换表达式一致。该metadata文件在取样后有其他协作者追加重定向断言API，已保留，新增API不混入本轮验收结论，详见delivery-consistency.json。不为追随无关在途功能重做或扩大本计划。根／完整范围fmt、范围workspace all-targets严格Clippy、当前组合受影响目标严格Clippy的范围分别据实记录，不声称全仓其他改动已经完成。

三语最终构建、文档浏览器与全导航分别见docs-build-final.log、docs-browser-final.log、docs-scope-final.log；欢迎页最终重复36组合验证及中文手机截图见browser-results.json、initial-zh-CN-light.png、initial-zh-TW-dark.png。所有证据保留于本批target/tmp/，本次无Git写操作及开发数据操作。本计划适用功能与验收无剩余阻塞，发布／性能／真实设备等未验边界按上文保留。


2026-11-22 用户随后明确授权本轮本地提交，不包含推送或发布。仅提交 URL 语言选择、欢迎页切换及必要的实例回调依赖；其他版本 API、MessageBag、复数选择器、测试框架和新增环境变量改动留在工作区。提交范围保持已验收的静态 lang 默认配置。

提交前核对：将实际暂存树导出为本批 source-index，与已验收 source 的3,272个非内部记录文件逐一核对，运行源码、配置、测试及资源全部一致；差异仅为三语正文排除其他协作者的复数选择器段落。实际暂存树重新通过 fmt、三语内容检查及 git diff --cached --check。沿用上述一致源码的131项范围回归与严格Clippy证据，不重复构建。config/app.rs 的后续环境变量改造、MessageBag消费者迁移及testing辅助转换保留未提交；本次不将组合139项结果解释为其已纳入提交。

## 欢迎页页头、母语名称与 Blade 链接生成（2026-11-23）

用户要求语言名称独立维护、页头图标切换语言和主题，随后明确要求将整个 `language_links()` 的处理移入视图。控制器仅传入 LocaleSelection 原始字段、查询参数、基础路径及原有页面元信息；不预生成链接或翻译文案。Blade 负责三语遍历、实际候选过滤、当前语言／手动选择标记、语言参数覆盖和自动模式参数删除，已删除 Rust 的 language_links 函数。

复用与边界：

- 核对固定 Framework v13.40.0／1449bf241017087fbd38c118ecaabef3f6797fdc 的 Collections/Arr.php、View/Compilers/Concerns/CompilesEchos.php，以及 PHP in_array 文档。快照及成功获取字节记录在本批 `view/research.json`。复用现有 Arr::query 的 RFC3986 编码、Arr::except 的点路径删除和 Blade 数组／比较实现；新增白名单表达式入口 Arr::query、Arr::except、in_array，不添加欢迎页专用 Rust 助手或任意 PHP 执行器。保留默认 HTML 转义。数组联合 `[$parameter => $choice] + $query` 保留数字键，避免 PHP 展开数字键重排。链接仍为包含可信基础路径的相对 URL。
- Arr::except 模板参数支持字符串或字符串数组；in_array 的 strict 显式参数限定布尔值，未提供为宽松比较。这是有限表达式适配，不能据此声称支持任意 PHP 函数、静态方法或全部 PHP 弱类型转换。模板缓存 schema 38→39，并同步两项既有失效测试的版本断言。
- 名称使用 `lang/{en,zh-Hans,zh-Hant}/languages.json` 各自的 name，通过 `__('languages.name', [], $choice)` 读取目标语言；删除 welcome.json 内原英文／简体／繁体名称词条。支持范围仍由已执行中间件决定，不增加应用语言配置、不重做协商。
- 页头左侧品牌及应用名，右侧原生 details 语言／主题菜单；键盘、焦点、触控和窄屏可用。主题默认 CSS color-scheme 跟随系统，light/dark 覆盖保存在 localStorage.appearance，system 删除覆盖；禁用 JS 时语言链接正常，主题继续跟随系统。受限存储只影响持久化。
- 页头／主题参考当日 Laravel React starter-kit use-appearance.tsx、W3C 语言导航母语名称指引、MDN color-scheme 与 details；来源及快照保存在 header/research/。没有引入 React、Cookie 或额外依赖，不宣称固定 Laravel 核心已有此默认控件。

验收使用 Rust1.94.0、项目内 TMPDIR 及独占 Cargo 目录。材料在 `target/tmp/locale-query-20261122/`。新增助手回归先在旧实现失败：view-red-behavior.log 中4项因 Unsupported Blade expression 失败，非仅编译报错。后续修正测试夹具的相邻指令边界及38→39缓存断言，没有放宽生产逻辑；欢迎页的两处断言更新查询键顺序，仍核对参数内容、编码及自动删除。

范围快照 Blade 28项（含6项新助手回归）通过。最终完整工作区快照 source-current 基线076a32825，保留其他协作者的 MessageBag／测试框架等修改，不再替换消费者 API：

```sh
cargo +1.94.0 test --locked -p openrust -p openrust-framework \
  --test welcome --test configuration --test localization --test startup --test console \
  --test blade_query_helpers --test blade_expression_contracts --test blade_arithmetic \
  --test blade_statements --test blade_translations
cargo +1.94.0 fmt --all -- --check
```

上述96项通过、0 ignored；fmt通过。实际日志为 view-current-tests.log／view-current-fmt.log。覆盖真实Kernel语言切换、null／空候选／回调优先隐藏、关闭中间件、参数配置、嵌套查询及恶意字符编码、数字键、请求和应用隔离、错误传播、缓存／子视图／组件／并发消费者、翻译及启动。三语正文同步模板助手示例、页头与主题操作；三语内容检查／构建（view-docs.log）、桌面手机浏览器（view-doc-browser.log）、双前缀全导航（view-doc-scope.log）通过。

本批构建中曾触及磁盘停线，已协调清理全部本项目旧编译产物；清理后的新活动缓存受锁保护并保留。清理仅针对构建目录，源码、日志、依赖下载、开发数据及卷未动，详情见 header/cleanup-result.json。后续保持增量和正常并发，可用空间恢复至90GiB以上。

未修改数据库／共享缓存行为，未启动无关服务。本批结果不代表其他协作者全仓对齐任务已完成；Windows、Docker、正式发布、真实移动设备、其他浏览器引擎及性能未验。语言选择和非三语文档链接的既有边界保留。本次用户授权实施与验收，没有暂存、提交、推送或发布授权。

最终质量与浏览器复验：

```sh
cargo +1.94.0 clippy --locked -p openrust -p openrust-framework --lib \
  --test welcome --test configuration --test localization --test startup --test console \
  --test blade_query_helpers --test blade_expression_contracts --test blade_arithmetic \
  --test blade_statements --test blade_translations -- -D warnings
```

当前组合上述严格Clippy通过（view-current-clippy.log），未增加lint豁免；第三方已有依赖警告不代表本项目代码lint失败。真实HTTP Kernel＋Chromium最终36组交互通过（view-browser.log、header/browser-results.json），含无JS320px、存储不可用／非法持久值、自动系统切换／手动覆盖／刷新恢复、原生菜单键盘／互斥／点击外部关闭、三语所有链接、保留查询、四类非法query400和无横溢出。已实际查看 header/header-zh-CN-mobile-light.png，母语菜单、品牌及右侧控件布局正常。临时HTTP服务正常关闭，fixture已移除。22个本批源码／测试／资源／正文文件与包含协作者在途改动的 source-current 快照逐字节一致（view/delivery-consistency.json）；其他协作者的后续无关改动不混入本次通过结论。

夹具修正记录：编译器缓存版本变更后，两项原有硬编码38的断言更新为39；新增模板测试按已有指令边界分行，避免相邻指令／词末指令被当作正文。语言参数现在先进入数组，查询键顺序相应变化，两项旧顺序断言已同步，仍验证全部实际键值，没有删除语言覆盖或参数保留断言。最终通过结果均来自修正后的源码与测试，不把中间失败计为通过。


2026-11-23 用户随后明确授权本轮本地提交，不包含推送或发布。提交欢迎页、通用Blade助手、母语资源、关联回归和三语正文及本主题记录；保留框架版本及其他对齐任务的未提交内容。欢迎页测试中已有的 MessageBag::first(None, None) 消费者迁移随测试保留，因为当前HEAD的MessageBag已采用该签名；不修改MessageBag实现。

提交前复验：从实际暂存树导出全新 source-commit-welcome-20261123，96项定向测试、全项目fmt、受影响lib／tests严格Clippy和三语内容检查全部通过，分别见commit-welcome-tests／fmt／clippy／content.log。22个运行／测试／资源／正文文件与此前96项及36组浏览器验收源码一致。首次导出误用了既有source-index目录，发现历史文件未覆盖后已中断该次运行，改用全新目录重新导出；中断批次commit-index-tests不计通过。内部交接及待办仅暂存本主题追加内容，其余修改保留在工作区。


## 可配置语言请求头（2026-11-23）

用户授权完善 locale-query-and-welcome-switching 计划及实施验收，未授权暂存、提交、推送或操作开发数据。开始 HEAD 为4044b5364，期间协作者持续提交；验收使用本轮独立工作区快照，其他在途修改保留，不将其后续状态归入本任务结论。欢迎页页头、母语资源、Blade 链接及原 URL 能力已存在，核对后复用，未重新开发。

### 调研与实现取舍

重新读取固定 Laravel Framework v13.40.0／1449bf241017087fbd38c118ecaabef3f6797fdc 的 Authenticate 回调、固定 Docs 960eedf0911155ad16326ade49c0ea5d18eda0b8 的 localization；联网获取 Symfony HttpFoundation v7.4.0 Request、HttpKernel v7.4.0 LocaleListener 和 CodeZero Laravel Localizer README。来源分别为 raw.githubusercontent.com/laravel/framework/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Middleware/Authenticate.php、raw.githubusercontent.com/laravel/docs/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md、raw.githubusercontent.com/symfony/http-foundation/v7.4.0/Request.php、raw.githubusercontent.com/symfony/http-kernel/v7.4.0/EventListener/LocaleListener.php、raw.githubusercontent.com/codezero-be/laravel-localizer/master/README.md；下载原文保存在本批 research/，不升级上游基线。CodeZero 仅参考有序来源和依赖执行顺序，README 为当时 master，不宣称其任一版本的完整兼容。旧 v0.x 的 translation/application_locales 测试主要覆盖旧配置／翻译结构，无可直接复用的请求头配置机制，沿用当前已有 Request 解析实现。

固定文档以 App 设置语言；Symfony 的 getLanguages 读取并缓存标准 Accept-Language，LocaleListener 划分显式来源和协商。此次保留这些职责；locale_header、默认 URL 与 localeUsing 是 OpenRust 扩展，不虚构上游同名配置。未引入探测器体系、依赖或另一解析器。

- config/app.rs 语言区域新增静态 locale_header=Accept-Language；缺键同默认，null 仅关头识别，其他值由 http::HeaderName 校验，空／非法／非字符串报含 app.locale_header 的配置错误。保持现有 query 环境覆盖，不为 header 增加环境变量、Builder 或配置文件。配置来自已加载仓储／缓存快照，先校验启用实例的配置，再进入回调／URL；非法配置不会被更高优先级输入掩盖。空候选仍最先关闭整个实例。
- SetLocale 顺序为 callback Some → URL → 指定头 → 原默认／候选兜底；None 继续，值解析、中文映射、权重及候选规则不变。Vary 使用实际头名，默认保留既有 Accept-Language 输出拼写，自定义头按 HeaderName 规范化小写；大小写不敏感去重，保留原有头及通配符，null 不追加。Content-Language、下游切换及隔离继续复用原生命周期。
- Request 只抽出内部按 HeaderName 读取的同一解析函数；公共 getLanguages／getPreferredLanguage 永远只描述标准 Accept-Language。标准来源借用原 OnceLock 缓存，自定义头在实际选择分支解析，不读写该缓存，因此无混源缓存键或额外失效状态；parts_mut 仍使标准缓存失效。无额外自定义缓存，重复实例会按当前头重新解析，未作性能结论。
- 欢迎页生产控制器／模板／资源无需改动。扩充真实模板消费者，验证自定义头、null、language 参数、三语元信息及四仓库链接；三语本地化正文新增 API 请求头示例、配置契约和缓存边界，中间件正文链接该主题。

### 验收与证据

工作流为 target/tmp/locale-header-20261123/，完整独立源码在 source/；核对空闲后复用 target/tmp/locale-query-20261122/cargo-target。全部验收经根 tests/build_guard.py，Rust1.94.0、项目内 TMPDIR、正常并发与增量，未触及32GB停线。守卫入口为：

```sh
python3 -B tests/build_guard.py --workflow locale-header-20261123 --target-dir target/tmp/locale-query-20261122/cargo-target -- python3 -B target/tmp/locale-header-20261123/verify.py
```

verify.py 在独立源码执行下列实际命令，日志按 final-* 命名：

```sh
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 test --locked -p openrust-framework --test set_locale_middleware --test request_languages --test application_locale --test config_scoped_overrides --test config_facade_access --test config_facade --test locale_bcp47 --test blade_translations --test blade_query_helpers
cargo +1.94.0 test --locked --test welcome --test startup --test application_skeleton
cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings
```

| 范围 | 结果 | 证据／边界 |
| --- | --- | --- |
| 框架9目标 | 74通过、0失败、0忽略 | final-framework.log；默认／自定义头、大小写、多行、权重、无效值、优先级、错误配置、缺键、null、空候选、Vary、事件、下游切换、路由组、并发／多应用、重复请求头变更及标准缓存不串源；配置缓存变更不影响已有应用，新应用保留null |
| 应用3目标 | 49通过、0失败、0忽略 | final-root.log；welcome19、startup28、application_skeleton2，真实欢迎页文案／HTML及HTTP语言／四链接和本地目标路径，保留原query和自动行为 |
| 质量 | fmt及workspace/all-targets严格Clippy通过 | final-fmt.log、final-clippy.log；隔离快照组合通过不等于协作者后续WIP已验 |
| 真实浏览器 | 44组HTTP Kernel＋Chromium交互通过 | browser-runtime.log、browser/browser-results.json；原36组加自定义头／null各4组，桌面／手机、明暗、三语点击、自动、刷新、母语名称、参数保留、自定义参数、候选过滤、回调、禁用、实际Vary、键盘、无JS320、受限存储。浏览器夹具实际执行1个测试通过，菜单手机截图已人工核对 |
| 三语文档 | 内容检查与站点构建通过 | final-docs-build.log，npm --prefix docs run build 内含内容及命令入口门禁；最终12组文档浏览器与三语双前缀全导航通过，见 final-docs-browser／final-docs-scope.log |
| 源码一致性 | 11个生产／测试／正文文件与最终验收源码逐字节相同 | consistency.json仅保存在临时批次，无长期哈希清单；内部记录不影响验收源码 |

失败记录保留而不计通过：初次测试夹具重复helper及错误Request命名空间已修正；red-behavior.log在旧实现真实失败（预期zh-Hant却得到zh-Hans），证明自定义头回归可发现缺口。首次fmt受临时浏览器夹具未格式化影响；浏览器首轮编译错误、随后90秒启动超时，以及同时清除临时夹具导致的Clippy读文件失败均不计通过。已使用本批ready文件的端口正常停止超时后遗留的验收服务，改为执行已构建的本批测试二进制，完成44组交互并退出；移除临时测试夹具后再运行上述最终完整质量／测试命令通过。未修改开发数据或数据库／缓存驱动，因此未启动无关三库三缓存服务矩阵。

本范围无功能阻塞。真实移动设备、其他浏览器引擎、Windows、Docker发布和性能未验；自定义Cookie／身份回调仍须应用自行配置私有缓存策略，语言头Vary不足以隔离。非三语应用语言可能指向不存在的docs目录，继续保留原已确认边界，不悄悄增加文档语言回退。


## 请求偏好与应用语言职责边界（2026-11-23）

本轮授权仅开发与验收，无暂存、提交、推送、发布或开发数据操作。起点 HEAD 为 `3201057adfdca1ab857762ee112688e3d4254394`；其他协作者在本轮期间提交 Eloquent 改动至 `87b9338d27f7a1df91463bd5cf009e3477dae88f`，保留其在途文件，未将它们计为本轮修复。

### 上游与实际核对

联网重新读取固定 Laravel v13.40.0 的 [Request](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Http/Request.php)、[HttpRequestTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Http/HttpRequestTest.php) 和 composer.json：Request 继承 Symfony Request，没有重写这两个语言方法；该基线允许 Symfony HttpFoundation ^7.4.0 或 ^8.0.0。本轮核对既定参考 [Symfony v7.4.0 Request](https://github.com/symfony/http-foundation/blob/v7.4.0/Request.php) 的 getLanguages／getPreferredLanguage 及 [RequestTest](https://github.com/symfony/http-foundation/blob/v7.4.0/Tests/RequestTest.php) 对无头、候选、权重和组合匹配的断言。getLanguages 固定从 Accept-Language 读取并保存请求级结果，getPreferredLanguage 消费它，不读取应用配置。只审阅上游源码与测试，未声称执行 PHP PHPUnit。下载原文在本批 research/，没有升级基线。

保留项目明确的 BCP 47 输出、严格 qvalue 过滤与中文应用候选策略，不把这些适配宣称为所有 Symfony 行为等价。现有 Request 标准缓存和内部 languages_from_header 已正确分工：标准头复用 OnceLock，自定义头按需调用同一解析器且不读写标准缓存。SetLocale 的回调→URL→配置头→默认候选优先级、候选关闭、配置失败与响应合并已正确，未重复改造。

生产调用点核对覆盖应用／控制器、bootstrap、routes、框架及扩展：未发现业务消费者通过 getLanguages／getPreferredLanguage 再次选择当前应用语言。欢迎页已用 App::currentLocale；译器、验证器使用既有语言作用域。邮件／通知的 Lang::getLocale 用于保存或恢复显式翻译作用域，包含 Lang::withLocale 的独立语义，保留而不机械替换。既有 Session 偏好回调须位于 Session 之后，原测试实际保存 Cookie 再读取法文，并验证 private, no-store；没有新增自动持久化机制。

### 实际修改与证据

| 范围 | 核对结论与实际修改 | 验证结果 |
| --- | --- | --- |
| Request／SetLocale／App | 核心行为正确；补 API 注释，明确标准偏好、配置协商和当前应用语言的区别、自定义头缺失不重试标准头、停用中间件不执行配置 | 新增 standard_preferences_and_actual_locale_are_independent_in_http_consumers，真实 Kernel 同时输出标准列表／首选、App、Lang 和翻译；覆盖协商前预读与协商后首次读取、URL覆盖、下游切换、不同应用默认值恢复 |
| 环境与缓存 | 保留 config/app.rs 中 get(env, APP_LOCALE_HEADER, Accept-Language)，仅澄清注释；修正文档及计划中的静态配置旧说明 | 新增 environment_header_selection_and_cached_configuration_reach_real_welcome，使用实际 config::load 验证缺省、自定义、大小写、缓存优先于非法环境、运行实例不重读、空值配置错误、字面null头名、JSON null关闭、停用中间件 |
| 原有边界 | 不改变回调、URL、用户Session偏好、权重、多行、无效值、缺头兜底、请求和多应用隔离、最终语言响应及已有头优先 | set_locale_middleware 全30项及 request_languages 全11项通过；自定义头缺失不回退标准头，已有 Content-Language 仍由显式响应提供者负责 |
| 三语正文 | 修正“中间件直接调用 Request 标准方法”的过时表述，说明 APP_LOCALE_HEADER／缓存／null、App::currentLocale 与停用行为 | 六篇 requests／localization 正文同步；内容检查、构建、12组文档浏览器及双前缀六组全导航通过 |
| 真实页面 | 保留欢迎页原有语言与主题交互、链接及缓存语义；本批浏览器自定义头服务直接使用 APP_LOCALE_HEADER 环境配置 | 从最新快照重新编译服务，Cargo JSON 确认 fresh=false 的本批可执行产物；44组真实 HTTP Chromium 交互通过，含桌面／手机、明暗、三语／自动、query、自定义头／null、无JS、存储异常及非法query |

### 实际命令与结果

临时资料、完整隔离源码、日志及浏览器材料位于 `target/tmp/locale-boundary-20261123/`。复用已闲置的独占 `target/tmp/locale-query-20261122/cargo-target`，增量与并发保持原样，所有运行经 `python3 -B tests/build_guard.py --workflow locale-boundary-20261123 --target-dir target/tmp/locale-query-20261122/cargo-target -- ...`；文档工作流使用项目内 docs-target，子进程 TMPDIR 与 RUSTUP_TOOLCHAIN 由守卫传递。

- `cargo +1.94.0 fmt --all -- --check` 通过。
- `cargo +1.94.0 test --locked -p openrust-framework` 定向9个test目标：set_locale_middleware、request_languages、application_locale、config_scoped_overrides、config_facade_access、config_facade、locale_bcp47、blade_translations、blade_query_helpers，75 passed／0 failed／0 ignored。
- `cargo +1.94.0 test --locked --test welcome --test startup --test application_skeleton`：50 passed／0 failed／0 ignored，其中欢迎页20项。最终合计125项通过，没有把跳过计为通过。
- `cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings` 通过；既有外部 imgalifier-avif依赖的5条cfg告警不属于工作区新增告警，未修改依赖以掩盖提示。
- `node docs/tests/check_content.mjs`、`npm --prefix docs run build`、`node docs/tests/check_browser.mjs`、`node docs/tests/check_scope.mjs` 全部通过；文档在隔离源码构建，浏览器使用已安装的Chromium。
- 本批 browser-compile、browser-run.py 先重新编译临时网络服务，再运行 browser/browser.mjs；服务已正常停止，临时Rust服务入口已移除，然后重新执行最终fmt／125项测试／严格Clippy。最终日志为 final-fmt.log、final-framework.log、final-root.log、final-clippy.log；结果及指纹见 closure.json。

首次框架测试编译暴露新增断言对 Option<Value> 多借用一次，应用测试编译暴露缺少 Application trait 导入，随后严格Clippy指出多余 Repository trait 导入；均修正后完整重跑。上述失败不属于通过结果，没有跳过断言或删掉用例。

收尾逐字节核对15个语言相关源码／测试／配置／视图／正文文件与验收快照一致；已逐文件检查其他非内部材料输入；协作者后续修改的 database/eloquent/ 与 database/query/ 无关文件已保留，一致文件数量与差异清单保存在closure.json，不以本轮组合结果声称这些后续在途变更通过。closure.json 明确列出差异，语言范围无未验阻塞。最终磁盘约69.73 GB／64.94 GiB可用，高于32 GB停线，未无故清空可复用缓存。

未验：完整PHPUnit、其他浏览器引擎／真实手机、Windows、Docker发布和性能压测；本轮未改数据库或缓存驱动共享行为，未启动无关服务，也未操作开发数据库／数据卷。Cookie或用户身份内容的缓存隔离仍须应用的private策略及对应Vary；语言Vary本身不提供完整私有隔离。非默认三语的文档目录边界沿用现有说明。


## 自动跟随与持久手动偏好（2026-11-23）

### 依据与实现决策

本节覆盖此前“不持久化语言偏好”的历史限制。固定 Laravel framework v13.40.0／1449bf241017087fbd38c118ecaabef3f6797fdc 的 Middleware Web 顺序、EncryptCookies、CookieJar 作为基础；固定文档960eedf0911155ad16326ade49c0ea5d18eda0b8 的 localization、requests、session 与 middleware 配合核对。Symfony v7.4.0 LocaleListener 说明请求语言与应用 locale 的职责；CodeZero Localizer 的 Cookie 来源先于浏览器，以及 mcamara 的浏览器／Cookie／Session 分工仅作参考。没有照搬探测器体系或将自动检测结果持久化。SetLocale、storePreference、locale_cookie 配置及默认装配是 OpenRust 扩展，不声称是上游默认本地化接口。

研究材料位于 target/tmp/locale-preference-20261123/research/。参考来源：

- https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Configuration/Middleware.php
- https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cookie/CookieJar.php
- https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/localization.md
- https://github.com/symfony/http-kernel/blob/v7.4.0/EventListener/LocaleListener.php
- https://github.com/codezero-be/laravel-localizer
- https://github.com/mcamara/laravel-localization

默认同一 SetLocale 自动装配在已解析路由的直接 Cookie 解密／队列之后、常规 Web Session 之前，有状态 API 则进入其保护包装器后执行。纯 API、未匹配和公开静态请求不启用 Web 偏好存储。依赖 Session／认证的回调仍由应用关掉默认实例并将相同中间件放到依赖之后。此变更避免全局先选语言、Web 再选一次的重复事件；未装配 Kernel 的独立 Router 保留原错误返回契约。

Web 优先级：回调 → 配置 URL 参数 → 已解密且未豁免的偏好 Cookie → 配置请求头 → 默认候选。自动结果及 URL 不写 Cookie。LocaleSelection 将有效固定偏好、可写能力与实际语言分开；业务使用 App::currentLocale。保存方法只接受写请求和当前候选，复用 Cookie 队列；欢迎页 POST /locale 由原 Web CSRF 保护，成功303跳转到固定站内首页，不读取外部重定向目标。

app.locale_cookie 缺键默认lang，保留应用已有 APP_LOCALE_COOKIE 覆盖，JSON null 关闭。Cookie::forever 复用400天及现有路径／域／HTTPS／SameSite／HttpOnly，删除复用相同属性。无效、损坏、候选移除安全回到自动；显式非法提交400。过期由浏览器执行，Cookie请求头自身不携带到期时间，不承诺拒绝被重播的合法旧密文。安全 Cookie 参与的页面合并 Vary: Cookie，并复用 Cache-Control 解析合并设置private,no-store，路由缓存不能重新公开。回调的其他私有来源仍由应用提供相应缓存策略。

### 修改范围与失败回归

- 框架：set_locale.rs／LocaleSelection／storePreference；foundation/http.rs、configuration/middleware.rs、routing/router.rs 装配；set_cache_headers.rs 保护私有缓存。
- 应用：config/app.rs、WelcomeController、routes/web.rs、welcome.blade.html，保留既有母语名称资源与主题实现，新增明确 POST 表单和模式显示。
- 回归：locale_preference、set_locale_middleware、welcome。扩大CSRF验收暴露旧夹具MessageBag错误值为字符串，按既有Vec<String>契约改为数组，未删除断言；直接Router未匹配错误行为回归已修复并恢复原断言。
- 三语localization与middleware正文、计划、规范和交接；正文只放开发者用法及真实支持边界。
- 2026-11-23在验收专用源码暂时禁用Cookie读取，protected_preference_is_distinct_from_actual_locale_and_never_needs_session实际失败：得到en而非已保存zh-Hans；随后恢复原源码再执行完整回归。误写筛选名的一次0 tests仅作为诊断，不计通过。

### 本轮最终验收

最终结果如下；旧批次和失败运行不作为本批最终结果。


| 范围 | 实际命令／消费者 | 最终结果 | 未验证边界 |
| --- | --- | --- | --- |
| 框架与HTTP管线 | `cargo +1.94.0 test --locked -p openrust-framework`，显式17个test目标：set_locale_middleware、locale_preference、request_languages、application_locale、config_scoped_overrides、config_facade_access、config_facade、locale_bcp47、blade_translations、blade_query_helpers、middleware_lifecycle、middleware_exclusions、routing_groups、cookies、cookie_value_prefix、csrf、request_input | 129 passed，0 failed，0 ignored；含失败注入后恢复的真正消费者 | 没有执行完整workspace测试或完整上游PHPUnit |
| 应用 | `cargo +1.94.0 test --locked --test welcome --test startup --test application_skeleton --test http` | 71 passed（25欢迎页＋28启动＋2骨架＋16HTTP），0 failed／ignored；含真实TCP CSRF／保存／读取／删除 | 不代表所有应用业务或其他在途代码通过 |
| 有状态API | `cargo +1.94.0 test --locked -p openrust-framework --features sanctum --test locale_preference` | 8 passed，0 failed／ignored；可信源安全管线读取偏好，无源／非可信源保持无状态 | 外部移动客户端和账号同步未实现，由应用接入 |
| 质量 | `cargo +1.94.0 fmt --all -- --check`；`cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings`；`cargo +1.94.0 clippy --locked -p openrust-framework --features sanctum --test locale_preference -- -D warnings` | 均通过；初轮协作native Blade格式与bindings.rs的box_default阻塞由对应负责人处理后刷新组合复验，无放宽警告级别 | 外部vendor依赖现有cfg／弃用等warning保留，没有作为本功能新增错误掩盖 |
| 浏览器 | 最终隔离源码新编译的临时TCP服务器＋Playwright Chromium，`browser-compile.log`确认fresh=false，`browser-run.py`执行browser/browser.mjs | 23组通过；3语×2视口×2主题12组、JS开关／过期损坏2组、query与header变体4组、停用控件3组、Cookie关闭临时链接1组、Cookie改名1组；刷新／导航／URL临时覆盖／更新／自动／键盘操作／私有响应头／四类仓库链接及文件存在性通过，无页面脚本错误 | Chromium模拟视口，其他引擎、真实手机、Windows及性能压测未验 |
| 三语文档 | `node docs/tests/check_content.mjs`、`npm --prefix docs run build`、`node docs/tests/check_browser.mjs`、`node docs/tests/check_scope.mjs` | 内容／构建通过，根及/docs双前缀12组浏览器和6组全导航通过 | 不代表Docker发布和公网托管已部署 |
| 构建守卫 | `python3 -B -m unittest discover -s tests -p build_guard_test.py` | 10项通过；修复实际遇到的非UTF8进程参数、macOS comm截断漏检，覆盖持锁／并行构建阻止清理；after-exit测试按PID确认进程退出，消除已有时序竞态，未放宽断言 | RustRover可能重启根target构建，不能绕过其Cargo锁清理 |

临时源码、研究材料、命令日志、截图与`closure.json`统一位于`target/tmp/locale-preference-20261123/`；Cargo复用`target/tmp/locale-query-20261122/cargo-target`。命令统一经`python3 -B tests/build_guard.py --workflow locale-preference-20261123 --target-dir target/tmp/locale-query-20261122/cargo-target -- ...`，docs另用同目录docs-target和docs工作流；TMPDIR、工具链传递由守卫负责。保留正常并发、增量和可复用缓存。初期相同Cargo目录有协作锁竞争，已协调对方使用其独占目录；最终命令不存在交叉构建。

过程触及32GB停线，守卫实际停止构建；清理扫描暴露异常字节和macOS comm截断，已修复。Cargo锁占用时拒绝清理，识别到RustRover自动重启的项目构建，未绕过锁删除。协作清理后空间恢复再验收；收尾可用空间以closure.json实测为准。守卫修改没有关闭停线，也没有删除源码、日志、下载缓存、独立交付文件或开发数据库／数据卷。

最后重新运行verify.py，五阶段全部exit=0；208次Rust测试执行均0 failed／ignored。浏览器服务器已正常结束，临时welcome_browser_acceptance.rs已移除。语言功能、应用配置、模板、消费者与三语正文共30个文件逐字节／SHA256核对与最终验收源码一致，差异为空。验收快照外其他协作者随后变化单列于closure.json的other_later_changes_not_verified：原生Blade验收材料、三语Blade正文和log模块，不把这些后续变化计为本轮通过。

本轮不涉及共享数据库／缓存驱动变更，未启动无关数据库或缓存服务；Session／Cookie测试只使用项目内隔离夹具。未操作开发／生产数据。此次无暂存、提交、推送或发布；工作区其他协作者的改动保留。非三语且停用默认中间件时docs/fr等不存在的原有欢迎页链接边界仍按正文说明保留，没有擅自增加回退或新语言管理器。

收尾指纹时HEAD为292418a396f62497ab7d1d6c295b8a20aa6524a8（含其他协作者提交），可用49.35 GB／45.97 GiB；暂存区为空。本次未进行Git写操作。


## 默认语言名称统一为 locale（2026-11-23）

本节是用户明确确认的新默认命名决定，覆盖此前 lang 默认；此前验收仍只代表当时源码。开始HEAD为292418a396f62497ab7d1d6c295b8a20aa6524a8，最新验收快照采集时为f0cdf615034f7fc97250d9e3ae14c8eb269f4001；期间保留其他协作者提交和未提交改动，本任务未进行Git写操作。

### 核对和修改

- 联网读取固定1449bf241017087fbd38c118ecaabef3f6797fdc的CookieJar.php和Foundation/Configuration/Middleware.php，核对Cookie名称由消费者传入，沿用已有forever／forget和解密／队列管线，不新增兼容层。查询参数／偏好Cookie默认名称属于OpenRust用户决定，不宣称上游原生配置。
- config/app.rs的APP_LOCALE_COOKIE、APP_LOCALE_QUERY_PARAMETER默认值改locale；SetLocale中两个缺键回退同步。Accept-Language／APP_LOCALE_HEADER、自定义名称、null、优先级、BCP47和请求隔离保持原契约。
- 欢迎页已有POST /locale、store_locale、locale表单字段及动态参数／Cookie消费正确，复用而不增加重复入口。针对现有默认消费者更新断言，保留lang/、HTML lang、第三方协议和自定义／旧来源负例。
- 新增HTTP Kernel回归：旧加密lang Cookie和旧query默认忽略，旧新Cookie并存只用新名称；Cookie／query分别及同时显式lang仍可读取、临时覆盖、删除；缓存名称覆盖实时配置且不同应用不串用。欢迎页覆盖文案、语言元信息、CSRF保存、旧参数保留及本地303重定向。
- 根.env未显式设置两个名称；环境模板、Docker模板、安装／生成链未发现额外硬编码旧默认。没有修改敏感环境值。三语localization正文同步用法和升级；规范及计划第1–11节当前契约更新，历史第12–15节保留并标注新决定覆盖。

### 实际验收

所有临时源码、资料、日志和截图位于target/tmp/locale-defaults-20261123/；独占复用Cargo目录为target/tmp/locale-query-20261122/cargo-target，命令由tests/build_guard.py传入项目TMPDIR、CARGO_TARGET_DIR及Rust1.94.0。没有操作开发数据或启动无关数据库／缓存。

| 范围 | 实际命令／证据 | 结果 |
| --- | --- | --- |
| 失败回归 | `run.py red cargo +1.94.0 test --locked -p openrust-framework --test locale_preference locale_defaults_ignore_legacy_sources_but_explicit_lang_remains_supported` | 旧生产实现实际失败：找不到默认locale Cookie；red.log保留，不计通过 |
| 框架 | `verify.py`中17个显式test目标，含set_locale_middleware、locale_preference、request_languages、application_locale、Config作用域／Facade、BCP47、Blade、生命周期、路由、Cookie、CSRF与请求输入 | final-framework.log：131 passed，0 failed／ignored |
| 应用 | `cargo +1.94.0 test --locked --test welcome --test startup --test application_skeleton --test http --test configuration` | final-root.log：74 passed（26欢迎页＋28启动＋2骨架＋16HTTP＋2配置），0 failed／ignored |
| 有状态API | `cargo +1.94.0 test --locked -p openrust-framework --features sanctum --test locale_preference` | final-stateful.log：10 passed，0 failed／ignored |
| 质量 | `cargo +1.94.0 fmt --all -- --check`；`cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings` | 最终均exit=0；依赖既有cfg warning未转成项目豁免 |
| 真实浏览器 | `browser-prepare.py`，新编译welcome_browser_acceptance（browser-compile.log fresh=false），browser-run.py／browser/browser.mjs | browser.log／results.json：25组真实TCP Kernel＋Chromium通过；三语×桌面手机×明暗，持久选择／更新／URL临时覆盖／恢复自动、过期损坏、关闭／改名、无JS、CSRF、Cookie及缓存头、仓库链接；新增旧来源忽略和显式lang端到端验证 |
| 三语文档 | `node docs/tests/check_content.mjs`、`npm --prefix docs run build`、`node docs/tests/check_browser.mjs`、`node docs/tests/check_scope.mjs` | 内容／构建、双前缀12组浏览器和6组导航通过 |

过程修正：删除Cookie也经过加密，新增测试最初误断言值必须为空，改为校验正确名称和Max-Age=0，保留浏览器实际删除验收。一次旧mtime导致Cargo复用旧feature测试产物，已改验收快照复制为内容变化才更新时间，并强制重编译该测试；最终feature日志确认Compiling和10项通过。整仓Clippy初轮遇到协作native_logger测试调用在途API错误，负责人修正后重新采集最新组合并通过；后续日志测试变化单列closure.json，不冒充其通过。

收尾以closure.json记录31个语言相关交付源码／测试／正文SHA256，和最终验收快照逐字节一致。浏览器服务正常退出；快照专用服务器测试移除。没有把旧lang默认的历史验收改写为新默认通过。

升级：旧Cookie不默认读取、不迁移／双写，用户重新选择；书签改?locale=...。需保留旧名称时显式配置APP_LOCALE_COOKIE=lang和／或APP_LOCALE_QUERY_PARAMETER=lang；升级重建配置缓存，缓存显式名称继续有效直到重建。没有自动兼容读取。

范围内无剩余实现阻塞。未验完整workspace测试、完整上游PHPUnit、其他浏览器引擎／真实设备、Windows、Docker部署和性能压测；本轮不变更共享数据库／缓存驱动。未经授权未暂存、提交、推送或发布。


用户随后明确授权本轮本地提交，不含推送或发布。提交语言请求头配置、请求偏好职责边界、持久偏好及locale默认命名的完整依赖；框架版本查询和无关对齐审计改动保留工作区。提交前31个语言相关暂存文件与本轮验收SHA256一致，10项构建守卫测试及三语内容检查再次通过。


## 语言候选自动发现（2026-11-23）

用户授权移除欢迎页三语硬编码并按已议方案实施；本轮无暂存、提交、推送或发布授权。既有控制器已传递 LocaleSelection.supported，继续复用，不在控制器重复扫描或新增语言管理器、依赖和配置键。

### 调研与实施

- 固定 [Framework v13.40.0 FileLoader](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Translation/FileLoader.php)：沿用路径、分组／顶层JSON与命名空间职责；自动发现属于 OpenRust 用户确认扩展，不宣称为上游原生接口。
- [mcamara/laravel-localization](https://github.com/mcamara/laravel-localization) README 的 supportedLocales／getSupportedLocales／native：借鉴候选与页面共用及母语标签；未引入包、路由管理或完整语言元数据库。该 README 提及其他继任方案，不据此判断本项目必须迁入其生态。
- [spatie/laravel-translation-loader](https://github.com/spatie/laravel-translation-loader)：保留文件与数据库混合来源职责；本轮只枚举 FileLoader 资源，纯数据库语言使用已有显式候选。后两项按2026-11-23读取的默认分支文档核对，非固定依赖。
- 旧 openrust-v0.x 的 FileLoader 目录枚举仅作参考；实际复用当前 cap_std、BCP47规范化和既有 HTTP 生命周期。参考源码保存在本工作流 research/，sources.json 记录实际成功获取的原始地址。
- FileLoader::availableLocales 枚举应用分组／顶层JSON及额外登记路径，稳定排序去重；忽略包命名空间、vendor、隐藏项、符号链接、空目录及非规范标签。缺失来源为空，其他IO错误返回；不解析JSON或推断完整度。
- 默认候选在 Provider boot 后、HTTP路由管线建立前发现一次；显式候选含空列表直接跳过扫描。运行中资源改变不刷新，重启新Kernel发现；无资源则不选择语言。独立注册的 SetLocale 实例使用 new 显式指定候选，Default 的发现由Kernel负责。
- Blade 两个分支遍历实际候选；补充可调用的 Lang::hasForLocale，复用既有精确语言检查，母语名称缺少时显示代码。增加 RTL 方向元信息、逻辑定位和长列表滚动，保留CSRF／加密Cookie／临时URL／header／配置缓存职责。三语正文说明新增资源后重启及非三语文档须自行提供。

### 验收与失败修正

隔离源码位于 target/tmp/locale-discovery-20261123/source，独占 cargo-target 及临时日志位于同一工作流。所有构建／测试通过 tests/build_guard.py，Rust1.94.0，正常并发和增量；未启动无关数据库／缓存服务，未操作开发数据。验收快照外的其他协作者改动不计为通过。

- 首轮框架失败4项：旧测试将实际存在的de／fr当成不支持；更新发现后的预期，负例改用未提供的it，未降低非法输入／事件／隔离断言。
- 首轮应用失败2项：GET分支仍保留三语循环；修复为与POST同源后全部通过。
- 新辅助函数测试曾写成普通英文紧贴@endif，被现有指令词边界规则拒绝；改用标准Blade输出表达式，保留缓存、无回退、默认／null语言、类型及参数数量错误验证。
- CORS生命周期两项原先隐含固定语言候选，而夹具没有语言资源；明确指定en候选以继续验证CORS与语言Vary共存，保留原断言；无资源默认停用由独立发现测试覆盖。

| 范围 | 实际入口／证据 | 结果 |
| --- | --- | --- |
| 框架 | regression-final.log；11个test目标：application_locale、blade_query_helpers、blade_translations、locale_bcp47、locale_preference、middleware_exclusions、middleware_lifecycle、request_languages、set_locale_middleware、translation_choice、translation_locales | 93 passed，0 failed／ignored；包括8项资源发现、启动快照、显式覆盖及新增辅助函数消费者 |
| 应用 | application-tests-second.log；welcome、http、startup、application_skeleton、configuration | 76 passed，0 failed／ignored；28项欢迎页，18语言夹具、名称缺失、RTL、CSRF持久选择及显式关闭 |
| 浏览器 | 新编译的快照专用TCP服务器，browser/results.json、browser.log | 24组真实HTTP＋Chromium通过：3视口×2主题×en/ar/he共18组，JS开关2组，GET临时模式／显式候选／空列表／callback4组；检查19项含自动、滚动、布局、Cookie保存读取删除。已人工查看手机RTL与桌面暗色截图。服务器正常退出，临时测试入口移除 |
| 三语文档 | docs-results.log、docs-content-final.log | 内容、VitePress构建、根与/docs双前缀浏览器、全导航均通过；格式收尾后内容检查再通过 |

测试中的15种新增语言仅为隔离夹具，本轮没有交付18种正式译文，也不自动创建其文档。

### 收尾

Sanctum feature 的 locale_preference 10项通过，stateful.log；合计179次定向Rust测试执行，0失败／忽略。cargo +1.94.0 fmt --all -- --check、cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings 最终exit=0，见fmt-final.log及clippy-final.log。依赖既有sqlx-sqlite弃用及imgalifier-avif cfg警告保留，未新增项目级豁免。git diff --check通过。

17个技术交付源码／测试／三语正文与最终快照逐字节一致，closure.json记录SHA256及本轮HEAD／磁盘；当前可用约76.1 GB／70.9 GiB，高于停线。其他协作者的HTTP Client、第三方来源／PHP参考文件清理等后续改动未同步本轮快照，未计入通过、未修改其暂存状态。当前计划范围无剩余实现阻塞。未验完整workspace测试、其他浏览器引擎／真实设备、Windows、Docker部署和性能压测；本次没有修改共享数据库／缓存行为，不据此计入三库／三缓存验收。

新增语言：在实际生效的资源根加入规范标签目录与JSON资源，languages.name提供母语名称，可选languages.direction=rtl，重启应用后自动出现；显式setLocale继续优先。数据库独有语言、启用子集及排序通过同一显式入口配置。此次未暂存、提交或推送；既有协作改动保留。

用户随后授权本轮本地提交，不含推送。暂存期间其他任务已提交欢迎页视图数据合并（971d8fe94）；保留新契约，复验当前组合后提交语言发现增量。

提交前组合复验：保留971d8fe94已提交的locale_selection整体视图变量，快照同步两处共享文件及控制器。precommit-welcome.log的28项测试、precommit-fmt.log及precommit-clippy.log（应用welcome消费者与框架库严格Clippy）均exit=0；17个暂存技术文件及HEAD控制器与本次快照逐字节一致。未重跑浏览器，不扩大原24组界面验收范围。


## 语言候选配置（2026-11-23）

### 决定与复用

用户确认通过配置限制语言、无配置时自动发现。新增可选 `app.supported_locales`，根配置由 `APP_SUPPORTED_LOCALES` 生成：未设置／null／(null) 为自动发现，逗号列表按顺序启用，空值生成空数组并停用语言选择与菜单。直接配置只接受数组／null；标签规范化、首次顺序去重，非法标签、错误类型及列表空项阻止启动，不降级扫描。优先级为显式setLocale（含空列表）→配置→发现；后两种显式列表跳过文件来源。候选在HTTP引导时捕获，保留现有配置缓存和重启边界，不提供运行中动态刷新。

复用现有config::env::get的null解析、canonical_locale、SetLocale::configure、FileLoader与LocaleSelection。未增加依赖、管理器、通用策略管线或应用启动入口改动。既有APP_LOCALE_HEADER用于普通自定义头，其URL／受保护Cookie之后的优先级不变；localeUsing保留最高优先级业务回调。缺少配置自定义头不重试Accept-Language；三语文档明确两种方式的区别。

本轮实际在线读取固定应用配置06d016a364a37430eec9cbc52209adce3d7667ce的config/app.php（https://github.com/laravel/laravel/blob/06d016a364a37430eec9cbc52209adce3d7667ce/config/app.php）与mcamara/laravel-localization的src/config/config.php（https://github.com/mcamara/laravel-localization/blob/master/src/config/config.php，2026-11-23读取）。前者核对语言配置职责，后者借鉴supportedLocales配置及候选与界面共用，不引入其整套路由方案。app.supported_locales与资源发现仍为OpenRust扩展。旧v0.x配置已检查，没有可直接移入的候选配置；沿用当前实现。web检索接口未返回内容，实际来源以urllib成功保存的原始文件及research/sources.json为准，不将搜索失败当成完成验证。

### 实际验收

全部命令经tests/build_guard.py、Rust1.94.0在target/tmp/locale-config-20261123/source隔离源码执行；复用已确认空闲的target/tmp/locale-discovery-20261123/cargo-target，子进程使用项目内TMPDIR。

| 范围 | 证据与结果 |
| --- | --- |
| 框架 | framework-verified.log：translation_locales 12、set_locale_middleware 31、locale_preference 15，共58项通过；覆盖顺序／规范化／去重、显式覆盖含空列表、空资源／读取错误跳过、错误配置失败及修正重试、启动快照、配置缓存和应用隔离 |
| 应用 | application-final.log：configuration 3、welcome 30、startup 28、application_skeleton 2、http 16，共79项通过；通过.env限制／自动／关闭候选，真实欢迎页菜单顺序、CSRF保存及受保护偏好、显式覆盖均通过 |
| 有状态API | sanctum.log：开启sanctum feature的locale_preference 16项通过；保留Cookie管线及请求隔离边界 |
| 真实浏览器 | browser.log、browser/results.json：390／1280两视口共12组真实HTTP Chromium；自动／列表／空值／null／显式覆盖／配置头六种模式，菜单、保存／重置及请求头优先级均通过，临时服务器正常退出；临时Rust服务入口已移除 |
| 三语正文 | docs-results-final.log：内容、VitePress构建、浏览器及根／docs双前缀全导航均exit=0；正文保留其他在途语言偏好便捷入口说明 |

上述153项定向Rust回归均通过，0失败／忽略。首次测试夹具误将捕获闭包传入仅接受函数指针的withConfig，改用夹具文件和无捕获配置函数；第二次错误诊断断言只读到BootstrapError外层提示，改为沿Error::source核对底层ConfigError::InvalidType的app.supported_locales键，保留错误启动及修正重试断言。最终上述目标全部重新通过。


收尾质量检查：fmt-closure.log的cargo +1.94.0 fmt --all -- --check，以及clippy-success.log的cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings均通过。Clippy发现本轮断言的err().expect写法，已直接改用expect_err，无新增豁免。全workspace初跑另暴露快照内过期的Response::body调用；仅将工作区已有的5处消费者测试文件同步至隔离快照，未修改其活跃工作区代码。快照遗漏的本地忽略夹具tests/reference/json-schema.php也仅复制至项目内临时快照以恢复原有编译输入；不新增跟踪文件。原始失败日志保留于clippy-final.log、clippy-verified.log及clippy-closure.log，同步路径指纹见clippy-snapshot-sync.json。第三方sqlx-sqlite、rav1e、imgalifier-avif既有警告保留，不宣称依赖零诊断。

本批未运行完整workspace测试、其他浏览器引擎／真实设备、Windows、Docker部署或性能压测；未改共享数据库与缓存驱动，不将本次候选配置计为三库／三缓存验收。没有添加18种正式译文或运行中配置热更新。其他协作者在途改动保留，不计为本任务独立完成的交付；本轮未暂存、提交或推送。

断言收尾后translation_locales 12项再次通过，见candidates-closure.log。9个本轮技术交付文件与最终验收快照逐字节一致，SHA256及HEAD见closure.json；git diff --check通过。最终空间约33.0 GB／30.7 GiB，仍高于32 GB停线；保留源码、日志、依赖及可复用产物，未清理其他构建。本次配置功能范围无剩余实现或验收阻塞。

用户随后授权本轮本地提交，不包含推送。提交前按任务差异分离共享文件，仅暂存候选配置、相关测试、三语正文和本主题记录，不包含请求体保存／返回URL及其他协作者改动。仅含本次暂存代码的commit-source隔离快照重新通过框架52项（9＋31＋12）、配置3项和欢迎页29项，共84项，0失败／忽略，见precommit-framework-retry.log及precommit-application-retry.log；fmt和三语内容检查通过，9个技术暂存文件与提交快照逐字节一致，见precommit-closure.json。此次未重新执行欢迎页浏览器或完整workspace严格Clippy，先前组合快照结果及范围保留原记录。

提交前复验曾触及32 GB停线，两个守卫安全停止构建，退出75不计通过；按项目规则取得锁并清理编译产物，保留源码、改动、依赖和日志，释放约101.8 GB，清理后约133.5 GB可用，实际事件见target/tmp/build-guard-events.jsonl。上表提交前测试已在恢复空间后重新执行通过。

## 请求体语言偏好与返回 URL（2026-11-23）

### 范围与依据

本轮在现有 SetLocale 上补充两个独立便捷入口，保留已提交的 locale_selection 视图数据、语言资源发现及其他协作者的候选配置改动。未新增管理器、依赖、配置、路由或账号存储；页面和 API 响应继续由应用决定。开始及收尾 HEAD 发生协作推进，以最终隔离源码和工作区对应文件为验收依据，不把整个工作区或其他在途改动归入本轮完成。

设计先核对固定 Framework v13.40.0（1449bf241017087fbd38c118ecaabef3f6797fdc）：

- `src/Illuminate/Http/Concerns/InteractsWithInput.php` 的 post/query/input，及 `tests/Http/HttpRequestTest.php::testPostMethod`／testQueryMethod：请求体与查询分别读取，不能用合并输入替代只读请求体。复用现有 Request::post，严格区分缺字段与显式 null。
- `src/Illuminate/Validation/Concerns/ValidatesAttributes.php::validateString`：字符串类型不隐式接受数字或布尔。本轮不为一个字段建立平行验证器；直接匹配 JSON 类型，复用已有语言标签、候选及 Cookie 校验，错误沿用 HttpError::InvalidRequest（400）。
- `src/Illuminate/Routing/UrlGenerator.php::query`、`tests/Routing/RoutingUrlGeneratorTest.php::testQueryGeneration`：复用既有 URL::forRequest／query 的参数合并和编码，不手写查询序列化。仅将内部 json_parameters 的可见范围调整为 pub(crate)，未增加公开 URL 别名。
- `src/Illuminate/Cookie/CookieJar.php`、`tests/Cookie/CookieTest.php` 及 `src/Illuminate/Routing/Redirector.php`、`tests/Routing/RoutingRedirectorTest.php::testComplexRedirectTo`：Cookie 队列／属性与响应、303重定向分开负责，继续调用既有 Cookie::forever／forget／queue 和 Redirect::to。

固定源码链接统一为 https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc 。官方用法核对请求输入、URL、Cookies及重定向对应章节；CodeZero laravel-localizer 与 mcamara laravel-localization 的来源／存储分工仅作职责参考，不引入其探测器或路由体系。当前 v0.x URL 实现没有可直接替代的请求体语言保存入口，复用当前 Request、URL 与 Cookie 机制。上游和生态没有本轮同名 SetLocale 方法，两个接口均明确为 OpenRust 扩展。联网参考文本留在 target/tmp/locale-save-20261123/research/，未新增或提交 PHP 文件。

### 实际契约与消费者

- `SetLocale::storePreferenceFromRequest(&mut Request, field).await -> Result<(), HttpError>`：读取处理后的顶层请求体字段。缺字段、数字、布尔、数组、对象报400，query不能补值；null／空字符串调用已有清除入口，非空字符串复用规范化、精确候选、安全Cookie管线及写请求约束。读取整个body再get字段，避免既有单键post过滤null而混淆缺失。沿用TrimStrings／ConvertEmptyStringsToNull，默认纯空白也会先变为空值；需要保留原始空白校验时由应用按现有规则排除字段。不改变当前请求语言／preference或额外派发事件，保存结果在后续请求生效。
- `SetLocale::urlWithoutLocale(&Request, path) -> Result<String, HttpError>`：应用指定单个`/`开头的本地目标，拒绝外部／协议相对地址、反斜线和控制字符。从实际LocaleSelection读取query_parameter，分别清理目标与当前query，保留其他参数和现有编码，当前query覆盖目标同名值，支持片段和可信代理子路径。关闭语言query时不删除参数。不保存偏好、不创建响应、不硬编码首页。
- WelcomeController::store_locale 只表达请求体保存、选择`/`返回目标及303响应；框架locale_preference夹具同时使用204和JSON响应，普通无状态API仍不能借此写Cookie，可信前端的有状态管线沿用既有CSRF和加密机制。
- 三语localization正文同步页面／API示例、清除语义、Cookie安全前提、输入处理及本地返回URL；不写入内部验收流水。

### 验收

Rust1.94.0，隔离源码 `target/tmp/locale-save-20261123/source/`；所有命令经 tests/build_guard.py，项目内TMPDIR及正常并发／增量开启。复用locale-discovery工作流Cargo缓存，协作任务占用时由Cargo锁等待，未抢占或删除其他任务产物。首个等待过程取消自身等待Cargo，不计通过；最终重新执行。最终复制保留mtime可能复用旧测试可执行文件，因此主动touch隔离快照中受影响输入，重编译后确认新增候选配置消费者实际出现在输出，不能仅用文件比较证明新源码已运行。

| 实际命令／范围 | 结果与日志 |
| --- | --- |
| cargo +1.94.0 test --locked -p openrust-framework --test locale_preference --test request_input --test urls | 15＋30＋6＝51项通过，0失败／忽略，final-framework.log |
| cargo +1.94.0 test --locked --test welcome | 30项通过，0失败／忽略，final-welcome.log；包含本轮真实POST／CSRF／303／后续请求和并行候选配置消费者 |
| rustfmt +1.94.0 --edition 2024 --check，六处变动Rust文件 | 通过，final-fmt.log |
| cargo +1.94.0 clippy --locked -p openrust-framework --test locale_preference --test request_input --test urls -- -D warnings | 通过，final-clippy-framework.log |
| cargo +1.94.0 clippy --locked -p openrust --test welcome -- -D warnings | 通过，final-clippy-welcome.log |
| node docs/tests/check_content.mjs、npm --prefix docs run build | 通过，docs-final-content.log／docs-final-build.log |
| node docs/tests/check_browser.mjs、node docs/tests/check_scope.mjs | 根与/docs/双前缀、三语、桌面手机12组合及6组全导航通过，docs-final-browser.log／docs-final-scope.log |

日志均在target/tmp/locale-save-20261123/。初次负例因默认TrimStrings已将纯空白转为空值而失败；测试fixture排除locale裁剪后覆盖原始非法标签，保留生产预处理语义并补文档，没有删除断言或跳过测试。文档全导航入口首次误写为不存在的check_scope_browser.mjs而失败，修正为check_scope.mjs实际重跑通过，不把失败尝试计为通过。全仓cargo fmt --all --check曾被其他在途http_client_sink.rs的格式阻塞（fmt.log），未修改该协作文件；本轮六处Rust格式及严格定向Clippy均通过。依赖现有警告仍可见，不据此宣称所有依赖零诊断。

未重新执行完整workspace测试、Windows／Docker／性能或欢迎页浏览器视觉验收；页面结构和样式未改，本轮通过真实HTTP Kernel／TCP消费者验收功能，文档浏览器通过不替代欢迎页浏览器新验收。未改变共享数据库／缓存驱动，不启动无关三库／缓存服务、不操作开发数据。未经授权，未暂存、提交、推送或发布。



补验：`cargo +1.94.0 test --locked -p openrust-framework --features sanctum --test locale_preference` 16项通过，0失败／忽略（final-stateful.log），包含可信来源的有状态API请求体保存204及后续偏好，不可信／无来源保持无状态。基础81项加feature组合16次执行，共97次执行，不将其中重复用例计为97个独立测试。收尾10处相关源码／测试／配置／三语正文与最终验收快照逐字节一致；新增候选配置只作为当前消费者组合回归，不归为本轮新增能力。磁盘最后约32,192,252 KiB可用，高于31,250,000 KiB停线，未主动清空可复用缓存。完整workspace与新欢迎页浏览器视觉验收仍未执行，其他协作源码未纳入本轮通过结论。

### 前端指定返回页面与本地提交（2026-11-23）

用户随后授权本轮本地提交，并要求前端返回参数优先、首页仅作兜底。WelcomeController读取请求体redirect字段，缺失／null／空值用`/`，错误类型返回400；先用urlWithoutLocale校验目标，再调用偏好保存，避免目标错误时先写Cookie。欢迎页通过Blade转义的隐藏字段传当前请求path；应用指定返回页面，框架不读取固定的业务返回字段、不改语言来源优先级。现有语言query清理、编码、其他query保留及可信代理子路径继续复用。新增消费者覆盖自选页面及片段、空／缺省、外部／协议相对／反斜线／相对地址和数组错误不写偏好、query不代替body返回字段。三语示例同步。

追加验收期间磁盘触线，build_guard安全停止本轮等待构建（exit75，不计通过），协作清理释放约100GB后保留日志／源码／Cargo锁inode。本轮改用target/tmp/locale-save-20261123/cargo-target独占目录重新编译，未继续与其他任务共用缓存。实际最终结果在本节收尾补记。

返回页面增量最终验收：独占target冷编译后的`cargo +1.94.0 test --locked --test welcome`31项通过、0失败／忽略（redirect-welcome.log）；两处变动Rust的rustfmt与`cargo +1.94.0 clippy --locked -p openrust --test welcome -- -D warnings`通过（redirect-fmt.log／redirect-clippy.log）。三语最终正文的内容、构建、12组双前缀浏览器及6组全导航全部重跑通过（docs-final-*.log）。上一阶段框架请求体／URL回归51项、有状态feature16项保持其当时验收边界，本次没有新增底层修改。真实欢迎页API流程以HTTP消费者为依据，未将文档浏览器当作欢迎页浏览器视觉验收。提交前核对源码与最终快照及Git范围，仅纳入本轮偏好保存／返回页面相关改动，保留其他协作者已暂存和未暂存的工作，不推送。
