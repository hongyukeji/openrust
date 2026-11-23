# 18语资源与消费者验收

日期：2026-11-23；首批基线 `74ef6493b`。当前目录布局与追加验收以文末“框架默认值与应用覆盖”为准，前文保留当时的首次交付证据。范围是运行时18语资源与交付链，开发者文档仍为英语／简体中文／繁体中文；不计为全框架正式发布或母语编辑审校完成。

## 来源与实现

- 固定 Laravel v13.40.0 的 FileLoader、Translator、消息选择器及 `lang:publish` 机制。资源参考 Laravel-Lang/lang `3fc62be68ac6d462f6a679944526fbddf5fced09`，MIT原文随应用资源、框架模板和发行声明保留。原始来源与适配见 `lang/standard-resources.source.json`，本批资源输出摘要及专有译文来源见 [资源来源清单](localization-18-source.json)。
- 旧版 `openrust-v0.x` 的语言资源和消费者在计划阶段已核对；旧结构／旧标签未迁回。本批复用主线 FileLoader、Lang、Blade、偏好保存及任务语言作用域，不引入运行时在线翻译、PHP／Composer或新的语言管理器。
- 18语为 en、zh-Hans、zh-Hant、ja、ko、fr、de、es、pt、it、ru、ar、hi、id、vi、th、tr、nl。每语8组资源，另含14项通知原句；标准资源72份、原句18份与许可共91个发布模板。应用资源是维护源，回归逐字节校验发布模板，禁止手工双份漂移。
- 社区差异按真实替换参数适配：日语2项、德语3项、阿语2项补 attribute；韩语／土耳其语调整 values；越南语 contains 移除消费者未提供的 values；移除 custom 示例占位词，保留空 custom／attributes。大小写替换遵循既有 :attribute／:Attribute／:ATTRIBUTE 契约，不机械将合法首字母大写降为小写。
- 欢迎页只对文档链接作有限集合映射，其余语言链接英语文档；当前App语言、回退及响应头不改变。语言菜单复用自动发现与现有滚动布局，版本用 bdi 保持混合方向。现有默认语言、偏好Cookie、URL和字段不迁移。
- `lang:publish` 复用原覆盖规则，按相对文件名逐个创建父目录并原子发布，支持18语分组、原句及许可。公开语言目录接线属于并行任务；保留其 `app.langPath("")` 调用及四个必要核心前置，不将该任务未验收的其他入口归成本批成果。

## 实际技术验收

隔离源码 `target/tmp/localization-18/source/`，独占Cargo缓存 `target/tmp/localization-18/cargo-target/`，TMPDIR为该工作流的项目内tmp；Rust1.94.0，保留正常debug与增量编译。证据日志均在 `target/tmp/localization-18-locales/`。不将跳过或仅存在资源文件算作消费者验收。

| 范围 | 实际结果与日志 |
| --- | --- |
| 资源结构／许可 | 4项Python测试通过；逐语检查精确键、重复键、叶子类型、变量、母语名、RTL、14项原句和模板逐字节一致。resources-final.log |
| App／HTTP／翻译与偏好 | 根localization 13项、welcome 34项、新资源消费者3项通过；覆盖语言优先级、回调、地区／扩展标签、配置缓存、加密偏好与CSRF、并发隔离、缺词回退及链接映射。rust-final.log |
| 标准资源消费者 | 18语精确Lang加载及实际required验证消息；18语本地捕获邮件的主题、正文、问候和版权；作用域恢复。数字格式全部18语，复数0／1／2／3／5／11／21／1.5，阿语6分支、俄语3分支。rust-final.log |
| Blade／选择器／语言标签 | 7＋2＋12项框架回归通过；保留转义、替换、缓存、组件、标签及既有复数机制。translation-regression.log |
| 实际Worker | 1项真实Artisan queue:work回归通过；18语邮件及通知、55个作业全部消费、failed_jobs=0、18条数据库通知、37条捕获记录，后续默认英语与App语言恢复。使用既有隔离SQLite夹具，不代表本轮重做MySQL驱动或跨平台验收。queue-locales.log |
| 实际CLI与发布 | 18语实际帮助及错误输出通过；根lang及旧resources/lang两种目录均验证默认不覆盖、force、existing不新增、用户其他文件保护、90份JSON及许可。cli-final.log；另以PTY完成18语×2种生成器的实际名称／视图提示与源码生成，cli-generators-final.log |
| 实际页面 | 18语×桌面／手机×明暗共72组通过；真实HTTP、Content-Language、HTML lang、文档链接、全部母语名称、偏好保存与删除、自动模式、无横向溢出和JS异常。桌面键盘Enter及手机tap操作。browser-final.log／browser-final/browser-results.json |
| 视觉核对 | 查看阿语RTL、德语长文案、印地语与泰语手机截图；版本方向、换行和字体显示可读。此检查不代表母语文意审校。 |
| 编译质量 | 隔离全工作区fmt与全targets严格Clippy通过。fmt-final.log／clippy-final.log |
| 三语文档 | 正文检查、站点构建、根／docs子路径三语桌面／手机12组合及全部导航页面通过；最终正文新增能力表后再构建并复验全导航。docs-content-final.log、docs-build-final.log、docs-browser.log、docs-scope-final.log |

## 分发与交接

独立目录实际二进制不带源码树或PATH，携带lang／resources／public即可消费18语；真实CLI发布到空目录并升级既有自定义资源已验。Dockerfile原本整体复制lang，无须加固定语言分支。当前隔离源码已通过正式docker/Dockerfile构建Linux ARM64 release镜像，镜像摘要 `sha256:ca03dedbfce4e266c6b777798a8bc046fb1690a38d801ccbd7efa5ebde6d0b58`。实际容器18语HTTP／文档链接通过，另在容器可写storage下的空应用根由编译发布器重建90份JSON和许可并逐字节核对通过，未改变镜像只读应用资源的所有权；日志docker-build-final.log／docker-final.log及docker/images.json。旧镜像不支持新增模板表达式的失败已保留docker.log，不计通过。此次为实际原生ARM64镜像验收，不代表x64或其他平台复验。

升级时一并部署语言树和模板，重建配置／视图缓存、重启常驻Worker与使用自动发现的常驻应用；显式候选由应用自行扩展。不改数据库持久化标识，不需要数据迁移。

语言资源、消息复数、数字格式与Faker地区数据分开：18语十进制格式化已验；数字拼写和各Faker格式器的地区支持不因资源扩展自动扩大。通用fr不会静默改成fr-FR，pt不会改成pt-BR；保留既有中文文字标签的提供者映射。外部邮件供应商、所有部署平台和母语编辑审校未验，不计通过。专有console／schema文案使用机器辅助初稿并人工修正协议、占位符及意大利语漏译；所有18语的最终母语质量仍待独立审校，不声称已取得人工母语认证。

最终265项资源、模板、相关Rust源码和四个并行核心前置与验收快照逐字节一致；摘要见target/tmp/localization-18-locales/accepted-inputs.json。编译与验收进程均已退出，测试容器已停止；保留热缓存及日志，未操作原开发容器或数据卷。

本批未暂存、未提交、未推送；保留并行语言路径与模型字段工作，最终是否提交及其组合范围单独核对。


## 框架默认值与应用覆盖

用户后续要求默认译文迁入框架、应用按键覆盖、星号启用全部可用语言；随后确认应用lang保留三语欢迎页、配置入口读取与解析分离，并删除框架语言目录中的许可和来源JSON。当前实现基于已提交语言路径契约b12f08102，保留其他协作者的模型字段及其他改动，不改共享索引。

### 正式布局与职责

- 唯一框架默认集合为vendor/openrust/framework/src/translation/lang/：18语×8分组与18份原句，共162份JSON。Provider将这些静态资源编入产物，默认加载不依赖运行时框架源码或发布命令。默认应用保留lang/en/welcome.json、lang/zh-Hans/welcome.json、lang/zh-Hant/welcome.json供直接定制，其他应用词典按需创建。
- 先加载框架默认值，再递归合并应用同语言分组。原句JSON顺序为addJsonPath来源、框架默认值、应用及后续addPath；原句按顶层键替换。这与固定Laravel FileLoader和TranslationServiceProvider的来源顺序一致。应用缺键保留同语言默认值，仍缺失才按既有fallback规则查找；损坏的覆盖JSON报错，不静默忽略。
- 直接FileLoader::new仍只加载指定文件；默认框架来源由Provider内部装配，不破坏独立加载器或包命名空间的行为。候选发现合并框架集合与应用资源，仍在HTTP引导时执行一次，不逐请求扫描。翻译组沿用既有进程内缓存，覆盖修改后重启常驻进程。
- config/app.rs使用supported_locales(get(env, "APP_SUPPORTED_LOCALES", "*")?)。默认"*"启用全部可用来源；null／(null)保留自动发现，逗号列表限制候选，空值生成空数组关闭识别。解析函数只接收值，不读环境；直接JSON配置同样支持"*"、数组和null。星号不是数组成员，也不代表无资源的全部BCP47标签都能翻译。
- 已删除用户指定的translation/lang/LICENSE-Laravel-Lang与translation/lang/standard-resources.source.json。完整MIT原文移至translation/stubs/LICENSE-Laravel-Lang，THIRD_PARTY_NOTICES.md保留完整原文；固定来源、源摘要及适配合并到本主题localization-18-source.json的standard_source字段。发布器共用同一默认集合，按需发布162份JSON和许可，不保留第二份词典副本。

### 追加验收

日志仍在target/tmp/localization-18-locales/，隔离构建保留正常增量及debug配置。触及32GB停线后，守护器已停止构建并持Cargo锁清理项目编译产物，释放75,945,283,584字节，保留源码、索引、日志和数据；随后从同一源码快照重新构建。

- 最新根配置3项、本地化13项、资源4项、欢迎页34项通过：bundled-final-tests.log。新增覆盖单键、嵌套分组保留、原句覆盖与addJsonPath顺序、缺组及非法JSON；18语默认加载、邮件渲染、数字／复数继续通过。
- 框架13项语言发现／候选测试、2项选择器、7项Blade回归通过：bundled-framework-tests.log。自动发现现在包含内置18语，使用非内置sv验证应用新增来源的启动快照与多应用隔离，另验星号及显式候选优先；保留非法类型／符号链接／非目录失败路径。
- 实际独立HTTP：仅应用键覆盖、无对应应用文件时使用默认词条、sv新增语言、星号／null／列表／关闭通过：bundled-http-revision.log。欢迎页下游自身的Content-Language保留，停用协商不会错误删除它。
- 18语实际CLI帮助／错误、36次PTY邮件及通知生成器提示／源码生成、两种应用语言目录的163项发布与覆盖保护通过：bundled-cli-revision.log。15项既有Artisan回归通过：bundled-artisan-regression.log；旧“德语缺译”夹具改用无内置资源的sv，继续实际验收配置的fr回退。
- 默认应用三语欢迎页及框架默认词条组合的72组浏览器语言／桌面手机／明暗／键盘触控回归，见bundled-browser-revision.log；文档同步说明内置来源、三语欢迎页、覆盖粒度、默认星号和许可位置。
- 迁移涉及的Migrations／Activitylog／Passport测试夹具改为读取框架schema资源，不再依赖根lang/en/schema.json。严格全工作区检查包含这些编译入口；本轮未重跑无关数据库驱动验收，不把检查通过当成三库实机通过。

最终格式与全工作区all-targets严格Clippy通过：bundled-fmt-final.log／bundled-clippy-final.log。三语正文检查、站点重建和根／docs子路径三语全部导航页面通过：bundled-docs-content-final.log／bundled-docs-build-final.log／bundled-docs-scope-final.log。

当前源码按正式Dockerfile构建Linux ARM64 release镜像，摘要sha256:f55bca90ba3778fb4b4af162e609fedd70a29643549da1b85f48dc1236479b04；在容器中显式核对应用没有日语欢迎词典，18语实际HTTP仍通过；空的可写应用根由编译发布器生成162份JSON及许可并逐字节核对通过。日志bundled-docker-build-final.log／bundled-docker-final.log，容器已正常停止，原开发容器与数据卷未操作。

最终183项相关Rust源码、模板、三语应用欢迎页、框架词典及许可与隔离验收输入逐字节一致，见bundled-accepted-inputs.json。用户指定删除的两个路径实际不存在；许可／固定来源仍分别保留在stubs与既有审计JSON。母语编辑审校和外部邮件供应商的既有边界不变。本批不暂存、不提交、不推送；b12f08102由另一任务单独授权提交，不计成本批18语改动已入库。
