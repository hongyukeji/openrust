# 欢迎页浏览器语言识别（2026-11-22）

> 下方初版为提交 `443f40af4` 的历史实现；当前方案见本文末尾「统一请求与模板国际化」。

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
