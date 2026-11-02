# laravel-modules 正式对应矩阵

> 正式目标：Laravel + laravel-modules = OpenRust
> 状态：固定来源、导航与首批模块运行行为已取证；其余行为盘点、Rust 实现与验收未完成
> 读取：2026-10-28；本文件不是当前已实现能力清单

## 固定来源与取证边界

- 用户指定仓库：[nWidart/laravel-modules](https://github.com/nWidart/laravel-modules)。冻结源码 tag 为 **v13.2.0**，实际 commit 为 [`59714840708a2aece19e810764473f53227c35fe`](https://github.com/nWidart/laravel-modules/tree/59714840708a2aece19e810764473f53227c35fe)。2026-10-28 通过官方 Git refs 核对：tag object `da07e554995c30d0d8b167a143c4653707ee281a`，peel 得该 commit；不使用浮动 master。
- 官方 [v13 Introduction](https://laravelmodules.com/docs/13/getting-started/introduction) 导航读取于同日，下表逐条登记实际入口；网站内容未有对应文档 commit，不伪造内容冻结。每条细化须再对照已固定源码与测试，文档晚于源码的差异不能悄悄加入本版。
- 官方固定 [composer.json](https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/composer.json) 已核对 package/provider 和 Laravel 13 范围；官方 Git tree API 确认该 commit 的 src/tests 树完整可读。已定向读取下列运行路径和测试；未运行 PHP 测试，不宣称全包行为盘点已闭合。
- 本批已读：`ModuleManifest`、`FileRepository`、`Module`、`Laravel/Module`、`ModulesServiceProvider`、`LaravelModulesServiceProvider`、`Support/ModuleServiceProvider`、`Activators/FileActivator`、`Commands/BaseCommand`；定向读取 `Generators/ModuleGenerator`。对应测试已读 Repository/Module、FileActivator、Enable/DisableCommand、LaravelModulesServiceProvider、Support/ModuleServiceProvider 和 Migrator 的相关断言。生成器全选项、发布器、完整命令族和生态整合仍须后续细化。

## 公共合同与边界

沿用[总计划差异分类](laravel-rebuild.md#逐项对应与差异分类)和[固定基线矩阵规则](laravel-rebuild-baseline.md#对应关系差异分类与证据)：上游源码/测试→Rust API/配置→等价测试→modules/example→文档→适用真实服务，逐项记录语义等价、必要Rust差异、用户约定、授权增强或缺口。

Laravel 核心与此第三方模块包分开标来源，但二者均为正式实现目标，使用唯一 Container/ServiceProvider。模块创建/规范目录、provider注册、配置/路由/资源/迁移/测试内聚、依赖与顺序、启停状态/管理、模块生成器均须按固定上游实际行为细化落地，不能只交付示例目录习惯。

最新目标采用 modules/<module>，见 ADR-001；目录使用 snake_case，并通过 Rust 编译期模块声明接入。Cargo/静态声明只解决编译时可用性，不等于运行时发现或启停：运行时状态只能控制已编译模块的注册与贡献，不能许诺加载未编译Rust源码或PHP式热加载。下文区分已取证的启停/priority 行为和待实现的依赖诊断、停机资源合同；不能将后者冒充上游已有自动依赖传播。

R2交共享Provider/Container、配置注册和模块依赖/贡献基础；R13交完整模块生成、启停/管理、资源发布及开发命令体验，相关领域按依赖提前接入。每阶段可分可验证子簇，未交付保持待办。资源集成页先核对已纳入生态与固定源码，不因导航存在就额外启动商业产品、未授权AI产品或真实付款；源码中存在的适用模块机制也不能因难度标范围外。

## 导航入口与待细化条目

当前只有 **35 个文档导航入口**，不是35个已闭合功能，也不能与Laravel矩阵的749个二级节相加伪称同一粒度完整盘点。稳定 M-ID 留作追溯，后续按章节/源码/测试细分行为与证据；导航行保持入口级状态；下文子行为单独记录本批取证进展，均未据此宣称 Rust 已实现。

| ID | 实际官方入口 | 依赖与实施归属 | 对应/差异/证据 | 交付阶段 | 状态 |
| --- | --- | --- | --- | --- | --- |
| M001 | [Introduction](https://laravelmodules.com/docs/13/getting-started/introduction) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M002 | [Requirements](https://laravelmodules.com/docs/13/getting-started/requirements) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M003 | [ChangeLog](https://laravelmodules.com/docs/13/getting-started/changelog) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M004 | [Upgrade](https://laravelmodules.com/docs/13/getting-started/upgrade) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M005 | [Installation and Setup](https://laravelmodules.com/docs/13/getting-started/installation-and-setup) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M006 | [Lumen](https://laravelmodules.com/docs/13/getting-started/lumen) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M007 | [Questions and Issues](https://laravelmodules.com/docs/13/getting-started/questions-and-issues) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M008 | [Configuration](https://laravelmodules.com/docs/13/basic-usage/configuration) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M009 | [Compiling Assets](https://laravelmodules.com/docs/13/basic-usage/compiling-assets) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M010 | [Creating a Module](https://laravelmodules.com/docs/13/basic-usage/creating-a-module) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M011 | [Custom Namespaces](https://laravelmodules.com/docs/13/basic-usage/custom-namespaces) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M012 | [Helpers](https://laravelmodules.com/docs/13/basic-usage/helpers) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M013 | [Routing](https://laravelmodules.com/docs/13/basic-usage/routing) | R2公开挂载→R3 HTTP→R13生成 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M014 | [Controllers](https://laravelmodules.com/docs/13/basic-usage/controllers) | R2公开挂载→R3 HTTP→R13生成 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M015 | [Views](https://laravelmodules.com/docs/13/basic-usage/views) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M016 | [Service Providers](https://laravelmodules.com/docs/13/basic-usage/service-providers) | R2共享Provider/声明契约；R13生成发现工具（复用R2-M/N） | 显式descriptor/拓扑已有基础；[编译期发现Spec](laravel-rebuild.md#r2r13-provider-编译期发现-spec)复用同清单，不将包发现与模块资源启停混同来源 | 一：模块内核/命令 | 基础子范围已验收；发现生成未实现/未验收 |
| M017 | [Middleware](https://laravelmodules.com/docs/13/basic-usage/middleware) | R2公开挂载→R3 HTTP→R13生成 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M018 | [Artisan Commands](https://laravelmodules.com/docs/13/advanced/artisan-commands) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M019 | [Facade Methods](https://laravelmodules.com/docs/13/advanced/facade-methods) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M020 | [Console Commands](https://laravelmodules.com/docs/13/advanced/module-console-commands) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M021 | [Module Methods](https://laravelmodules.com/docs/13/advanced/module-methods) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M022 | [Module Resources](https://laravelmodules.com/docs/13/advanced/module-resources) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M023 | [Languages and Translations](https://laravelmodules.com/docs/13/advanced/languages) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M024 | [Tests](https://laravelmodules.com/docs/13/advanced/tests) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M025 | [Publishing Modules](https://laravelmodules.com/docs/13/advanced/publishing-modules) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M026 | [Registering Module Events](https://laravelmodules.com/docs/13/advanced/registering-module-events) | R2模块契约；R13完整开发/管理体验 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M027 | [Migrations](https://laravelmodules.com/docs/13/database/migrations) | R2注册契约→R4/R5/R6领域→R13模块命令 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M028 | [Models](https://laravelmodules.com/docs/13/database/models) | R2注册契约→R4/R5/R6领域→R13模块命令 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M029 | [Seeders and Factories](https://laravelmodules.com/docs/13/database/seeders-and-factories) | R2注册契约→R4/R5/R6领域→R13模块命令 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M030 | [Resources](https://laravelmodules.com/docs/13/resources/resources) | 先核对固定源码/已纳入生态依赖；R9/R13及对应生态包职责，不依赖业务后台 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：通用生成；二：生态集成 | 待盘点/待实现/待验收 |
| M031 | [Inertia](https://laravelmodules.com/docs/13/resources/inertia) | 先核对固定源码/已纳入生态依赖；R9/R13及对应生态包职责，不依赖业务后台 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 二：随已纳入生态依赖 | 待盘点/待实现/待验收 |
| M032 | [Custom Module Generator](https://laravelmodules.com/docs/13/resources/custom-module-generator) | 先核对固定源码/已纳入生态依赖；R9/R13及对应生态包职责，不依赖业务后台 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：模块内核/命令 | 待盘点/待实现/待验收 |
| M033 | [Livewire](https://laravelmodules.com/docs/13/resources/livewire) | 先核对固定源码/已纳入生态依赖；R9/R13及对应生态包职责，不依赖业务后台 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 二：随已纳入生态依赖 | 待盘点/待实现/待验收 |
| M034 | [Spatie Laravel Permission](https://laravelmodules.com/docs/13/resources/spatie-laravel-permission) | 先核对固定源码/已纳入生态依赖；R9/R13及对应生态包职责，不依赖业务后台 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 二：随已纳入生态依赖 | 待盘点/待实现/待验收 |
| M035 | [Module Generator](https://laravelmodules.com/docs/13/resources/laravel-module-generator) | 先核对固定源码/已纳入生态依赖；R9/R13及对应生态包职责，不依赖业务后台 | 待逐项源码/测试映射；未据目录或Plugin判完成 | 一：通用生成；二：生态集成 | 待盘点/待实现/待验收 |

原 app/example 已迁至 modules/example，编译描述、文件状态、Provider、模块配置/资源及统一迁移的 R2-M 实现与真实证据见 [当前测试记录](../memory/testing.md#r2-m-模块清单资源与统一迁移验收证据)。基础 GET 仍不等于真实 CRUD；管理写命令、发布流程与其余模块矩阵继续按逐项证据验收，不批量改成完成。

## 首批行为合同：运行发现、Provider 与管理

本批按真实运行路径细分 **30 个 B-ID**，不是完整模块行为总数。状态统一为“源码/指定测试已读，Rust 待实现/待验收”；只有源码推导的边界明确标注。`T` 是拟新增的行为测试名，不是现有通过记录。上游证据索引见下节。

| ID / 导航 | 固定上游行为与证据 | OpenRust 对应与差异边界 | 验收 |
| --- | --- | --- | --- |
| B001 / M005,M021 | Repository 扫描路径内 `module.json`，Manifest 另从扫描路径收集启用模块；不是只读取目录名。[S1][S2] | 已实现配置路径扫描metadata与命名编译绑定的[限定子范围](../memory/testing.md#模块运行发现与编译文件限定验收)；仓储/Provider资源取manifest目录，仅files使用module_directory覆盖。完整glob/非有限JSON/平台边界继续开放；modules小写为用户约定，不执行未编译Rust | T01 |
| B002 / M019,M021 | `find` 先不区分大小写精确匹配，再 Studly 名回退；找不到 `findOrFail` 抛 ModuleNotFoundException。[S2][U1] | 保留身份查找顺序，Rust 路径名与显示名分开；错误含模块身份，不返回空模块 | T02 |
| B003 / M008,M021 | FileActivator 没有状态键则 disabled；只有严格布尔 true 才 enabled。[S3][U2] | 缺失记录不默认启用；构造期实例快照、自身写失败内存语义及顶层数字键已按限定范围验收；实时file-live/锁内合并/严格IO为公开增强。完整边界仍开放，见[实例缓存验收](../memory/testing.md#fileactivator-实例缓存限定验收) | T03 |
| B004 / M008,M021 | 状态文件缺失、空白为无状态；损坏 JSON、非对象及非空列表报错并保留文件；空列表可读为空。[S3][U2] | 顶层PHP trim、空表/list及numeric对象拒绝、构造错误与原字节保护已限定验收；非有限数、未知嵌套形状/PHP字节编码完整等价仍开放，不覆写损坏证据 | T04 |
| B005 / M018,M020 | enable/disable 改 activator；Module 发 enabling→enabled 或 disabling→disabled，setActive 直接更新不等于完整事件操作。[S3][S4][U3] | 管理结果和事件分开验证；失败不得发成功事件。运行状态改变不等于卸载当前 App | T05 |
| B006 / M018,M020 | 多名称先 validateModules 再执行；无参数且非交互失败；`--all` 展开；动作失败汇总非零退出。[S5][U3] | CLI 语法可采用 Rust CLI 表达，但名称校验先于写状态；操作性失败不虚报事务式全回滚 | T06 |
| B007 / M016 | Manifest providers→Laravel ProviderRepository；先 aliases，后 providers，再 include files。`Module::register/boot` 已 deprecated，包不调用。[S1][S4][S6] | 命名编译工厂/真正嵌套注册及ModuleFile已接唯一内核；aliases与完整ProviderRepository eager构造/编译缓存、deferred仍未交付，不复活deprecated Module生命周期，不以子范围关闭T07 | T07 |
| B008 / M016,M021 | Manifest 仅启用模块，按 priority 升序（缺省0），保留各模块 providers 数组顺序。[S1] | 有限JSON默认PHP8.5/precision14的Zend比较驱动、Manifest排序/Repository排序/显示分别实测2068向量；全register后boot，非有限数值与全PHP环境仍缺口 | T08 |
| B009 / M016,M021 | 已读加载路径没有 requires 拓扑、缺失/环校验或启停级联；priority 不等于依赖图。[S1][S2] | 不捏造上游依赖排序。显式 provider 依赖诊断属共享 R2 合同，不能偷偷改变无依赖模块 priority 顺序 | T09 |
| B010 / M019,M021 | Repository 扫描同小写名称后者覆盖；Manifest 独立扫描不提供同样的唯一身份验证。[S1][S2] | 编译注册同名/规范化碰撞选启动报错，列双方来源；这是明确的安全增强差异，不能声称上游本来拒绝 | T10 |
| B011 / M019 | 同 alias 后加载模块获胜；上游 PHP AliasLoader 是类名别名，不是服务 binding 别名。[S1] | Rust import/type alias 处理语法映射；运行服务别名另属 Container。不得把一个替代另一个便宣称全覆盖 | T11 |
| B012 / M008,M016 | 自定义 activator 名缺失、class 不存在启动明确失败；repository singleton 延迟读取路径配置。[S7][U4] | 公开 activator trait/选择配置；未注册 driver 报错，不回退文件/内存。默认App独立执行边界，可显式ModuleExecution共享Manifest/canonical files一次状态；不等同OS进程永久缓存，异步失败封闭差异公开记录 | T12 |
| B013 / M018,M021 | disable 仅写状态，无代码移除/路由撤销逻辑；Manifest/Repository 还存在进程缓存。[S1][S2][S4] | 选定新启动构建启用快照：当前运行 App 不热卸载，新 App 不注册禁用模块；CLI 明示需重启，不许诺 PHP 式热加载 | T13 |
| B014 / M018,M021 | delete 删除状态和模块目录，并发 deleting/deleted；不是数据库 rollback。[S4] | 删除命令作为 R13 独立破坏性操作实现；拒绝越界路径，尊重仓库 WIP/授权；禁用/卸载均不自动删表 | T14 |
| B015 / M010 | generator 校验路径与已存在模块；无 force 拒绝覆盖；先渲染全部 stub 验证，再删除既有目标；生成结束写 active 状态并发 created。[S8] | 生成源代码、声明清单与状态可验证；目录 `modules`、snake_case、无独立应用骨架为用户约定。失败不能报告已完整接入 | T15 |

## 首批行为合同：配置、路由与资源

| ID / 导航 | 固定上游行为与证据 | OpenRust 对应与差异边界 | 验收 |
| --- | --- | --- | --- |
| B016 / M008,M016 | Support provider 在 boot：commands→schedule→translations→config→views→migrations；register 负责子 provider。[S9] | 同一 provider 执行资源装配，不在 register 解析网络服务；子 provider 注册仍须遵循 Laravel 核心完整生命周期 | T16 |
| B017 / M008 | 模块 config 以 `array_replace_recursive(existing,module)` 合并；模块覆盖既有值，仅既有的键保留。不是 Laravel 核心 mergeConfigFrom 默认值方向。[S9][U5] | 配置操作必须区分 `core defaults` 与 `module overlay`；不另建第二 Config。`blog.name=My Blog` 加模块 `Blog` 最终为 Blog | T17 |
| B018 / M008 | config.php 最先，其余递归文件按路径排序；专用 settings 文件覆盖 config.php 中的 settings。[S9][U5] | TOML 同职责映射及排序，不靠文件创建顺序；模块 namespace 包含嵌套路径，消除相邻重复 namespace 段 | T18 |
| B019 / M008 | PHP 递归替换数字索引而非数组整体替换。[S9，语言操作语义推导；未发现本批专用数组测试] | 模块 overlay `[a,b,c]` 加 `[x]` 得 `[x,b,c]`；普通 Figment 来源数组规则单独定义，不能共用原子替换实现 | T19 |
| B020 / M008,M016 | overlay 在对应模块 boot 才发生；前序 boot 已读取的值不会追溯改变。[S9，调用时序推导] | 不把所有 overlay 前移到 FinalizeConfig 冒充等价；Boot 阶段受控写唯一配置仓储，后续读取见新版本，Ready 后冻结 | T20 |
| B021 / M008 | configurationIsCached 时跳过模块合并；发布到按配置 key 派生的命名空间路径，不覆写顶层 settings。[S9][U5] | 缓存产物须含已合并值和有效模块指纹；缓存命中不重复 overlay。缓存失效与 publish 命令后继实现，不可无条件跳过 | T21 |
| B022 / M013,M014,M017 | 模块 route provider/stub 通过 Laravel Router 挂 API/web 组；不是框架扫描 controller 自动发路由。[S10] | 模块拥有 routes/controller/request/resource；公开 provider 贡献唯一 Router；API/web 中间件、prefix/name 语义归 R3，同路由冲突规则依核心合同取证 | T22 |
| B023 / M015,M022 | namespaced views：已发布的应用 views 路径在模块源路径之前，源路径兜底；并注册 component namespace。[S9] | `module::view` 解析顺序可测；覆盖是有意行为，不能将所有同名资源统一判冲突。Rust 模板引擎语法差异单列 | T23 |
| B024 / M023 | Support provider 优先已发布 lang/modules/nameLower，否则模块 lang；均注册 namespace 与 JSON。[S9] | 语言包跟模块注册，保留 override/fallback 次序；disabled 模块不贡献 | T24 |
| B025 / M023,M027 | 包级 migrations 自动发现默认 true，translations 默认 false；provider 自己加载资源与包级自动发现是两条入口。[S7][U4] | 配置开关分开，实现注册去重；禁用模块两入口都不得泄漏。不能把 translations 默认写成 true | T25 |
| B026 / M027 | Migrator getPaths 只取配置路径与模块内部已注册路径/文件，排除相邻 BlogExtra 和根的其他迁移。[U6] | 路径归属用规范化边界判定，不能用字符串前缀 `modules/blog`；保留 migration 来源身份 | T26 |
| B027 / M027 | 迁移支持指定数据库且不改变默认连接；事务取决于 grammar 支持与 migration withinTransaction。[U6] | PG/MySQL 分别验证；MySQL DDL不能伪称通用可回滚，错误不得污染默认连接；禁用不回滚历史迁移 | T27 |
| B028 / M018,M026 | schedule 仅实际 override configureSchedules 时在 app booted 后装配；event hook 只改类名猜测，不添加扫描路径。[S7][S9] | Rust 静态 handler/调度声明映射，不伪造运行时源码扫描；未启用模块无 task/handler 贡献 | T28 |
| B029 / M019,M022 | `asset('module:file')` 需要模块名，缺少冒号抛 InvalidAssetPath；路径映射 public/modules。[S2][U1] | 模块 source 仍在模块内，发布后公开路径与 source 所有权分开；路径遍历安全另行测试，不冒称现源已验证 | T29 |
| B030 / M024 | 同进程 App 隔离、关闭清理和资源冲突不是本批上游模块测试已证明的保证 | 接入单一 R2 Container/Scope/Shutdown；模块 boot 失败逆序清理本次创建资源，禁止 listener Ready 前接流量；不是模块专属第二容器 | T30 |

## 配置与生命周期必须先收敛的接口条件

以下是供实现者直接采用的合同输入，**不是现有 Rust API**；最终签名由共享 R2 合同统一，不能在模块实现中私造另一内核。

1. 编译描述只携带模块身份、priority、有序 provider 构造器、资源来源；启用状态在应用启动读取。普通 Rust module 不具备子 Cargo.toml 自动依赖解析。
2. 一个模块 provider 进入唯一 register/boot 清单；显式清单、模块发现顺序和 priority 排序分别有记录。Provider 依赖诊断不可冒充上游模块依赖管理；未验证的 package 依赖由 Cargo 负责，不是运行时自动 enable。
3. 共享配置仓储提供受控的 module overlay 操作，携带来源、namespace 与版本。阶段应为 Prepare/Register→基础来源校验→有序 Boot（按模块时点 overlay/读取/资源装配）→最终配置/贡献校验→Ready。类型化 section 每次解析对应仓储版本，不能全程缓存旧快照。已解析服务不会因后续 overlay 被秘密重建，这与上游读取时序一致。
4. `BootContext` 的一般业务写配置权限可限制；模块 overlay 由框架公开资源贡献执行器按已声明数据执行。仍须容许其在对应 boot 时点改变后续读取，不能靠一个不可变最终快照抹掉差异。最终 schema 失败启动失败、清理已创建资源，不打开 listener。
5. R2合同初稿曾将所有defaults置于最低层并禁止boot配置变更，与B017/B020冲突；当前修订已区分核心配置操作与模块boot时点overlay。配置实现与对应验收仍未交付，不能把容器基元通过当作模块配置已等价。
6. 根 DB/View 等能力按有效配置启用：未配置跳过，显式错误/缺 driver 启动失败，模块按真正可用服务挂载相关路由；仅有 binding 不证明后端可用。模块 overlay 若改变连接配置，必须明确服务首次解析时点，不能静默沿用旧连接。
7. 运行 enable/disable 只影响后续 App 构建。当前 App 资源由自己的 shutdown 持有；持久状态操作不隐式关闭共享连接、不 rollback、不删除代码。热卸载若后继实现须另立合同与证明，不计本批交付。

## 可执行验收场景

所有测试均为**待实现**的 Arrange→Act→Assert；fixture 使用临时目录、已编译的 A/B 模块和同一公开启动路径，不新增第二长期示例。T01–T30 与 B-ID 一一对应，测试文件名/命令在实际实现时登记，不冒称已有文件。

| 测试 | 最小输入与操作 | 必须断言 |
| --- | --- | --- |
| T01 | 编译 A/B，清单仅启用 A，另放未编译 C 描述 | 仅 A provider 运行；C 启用报未编译身份错误，不能执行任意源码 |
| T02 | BlogPost、字面 blog-post 两身份；查精确、小写、snake/kebab、unknown | 精确优先；回退次序与 S2 相同；unknown 有类型化错误 |
| T03 | 缺文件、空对象、A=false/true、A 为字符串 true | 前三类禁用，仅布尔 true 启用；非布尔兼容读取不当作 true |
| T04 | 空白、[]、[true]、损坏对象、标量，逐一启动 | 合法空状态可启动；非法不启动且原文件字节不变 |
| T05 | 捕获 enable/disable/setActive 事件，并注入写入失败 | 完整操作事件顺序；直接 setActive 不假发前后事件；写失败无成功事件 |
| T06 | 无参数；A+不存在 B；--all；A 动作失败但 B 可执行 | 无参数非零；未知 B 在 A 写入前拒绝；全量展开；非零汇总不掩盖部分结果 |
| T07 | A/B provider 写 register/boot 日志 | 全 register 在首 boot 前；每个一次；不出现第二 Module 生命周期 |
| T08 | priorities -1/0/5，各有两个 provider，重复 priority | 排序及模块内次序确定；同 priority 差异规则独立快照 |
| T09 | 无依赖模块反向声明，另加缺依赖/循环 provider 夹具 | 无依赖仅 priority；显式依赖错误诊断，不自动声称级联启用 |
| T10 | 两描述同身份、BlogPost 与规范化碰撞路径 | 启动失败含双方来源，无部分可用 App；记录该增强差异 |
| T11 | 两模块相同 PHP alias 对照；Rust 显式类型/服务别名夹具 | 记录上游后者胜；Rust 类型别名不假装运行服务重绑 |
| T12 | 两个 App 使用不同清单/目录；activator 名缺失 | 无进程 static 串状态；驱动缺失明确错误，无回退 |
| T13 | App1 启动 A→disable A→构建 App2 | App1 保留原快照，App2 无 A 路由/任务/资源，状态写入不删数据库 |
| T14 | 临时模块 delete；路径越界；禁用含已迁移表模块 | 仅合法目标可删；越界拒绝；禁用不删目录不删表 |
| T15 | 临时生成模块，重复生成，无 force 与失败路径 | 生成/状态/声明一致；无 force 或无效 JSON stub 保留既有字节；生成物编译并通过自身测试 |
| T16 | 模块含子 provider、调度和资源，各记录阶段 | 子 provider register 在 boot 前；资源次序依 S9；调度在 booted |
| T17 | 既有 blog={name:App,per_page:10}，模块 name=Module | name=Module，per_page=10；不能被 env 重新覆盖 module overlay |
| T18 | 先创建 settings 再创建 config；嵌套重复 blog 路径 | config 先执行、settings 后覆盖；key/publish 路径不重复 blog |
| T19 | 既有 items=[a,b,c]，模块 items=[x]；再测嵌套/标量替换 | [x,b,c]；map 递归；标量按模块值替换；与普通来源数组测试分离 |
| T20 | observer-before→模块 overlay→observer-after | before 读 App，after 读 Module；先前已创建实例不追溯变更 |
| T21 | 用已合并缓存启动；更改启用清单；发布 nested config | 命中不二次 overlay；清单变化使缓存失效；不覆写根 settings |
| T22 | A/B 各贡献 API/web 路由，禁用 A 后新启动 | 只有启用模块路由；组 middleware/prefix/name 等按核心 Router 测试 |
| T23 | 模块同名 view 与发布 view、仅源 view、相邻模块同局部名 | 发布优先、源兜底、namespace 隔离；重复不是一律冲突 |
| T24 | 模块 lang 与已发布 lang，另有 JSON 语言项 | 发布 namespace/JSON 优先；缺发布时用模块；禁用无贡献 |
| T25 | 默认开关/显式开关与 provider 自己注册组合 | migrations 默认自动发现；translations 默认不自动发现；重复路径不执行两次 |
| T26 | Blog、BlogExtra、根其他迁移、Blog 内子目录与文件 | 只选择 Blog 所属路径，不能因前缀把 BlogExtra 纳入 |
| T27 | PG/MySQL 指定连接执行成功/失败迁移，事务 opt-out | 不改默认连接；真实 DDL 事务边界据后端验证，不用 SQLite 证据代替 |
| T28 | 无调度 override、有调度、禁用模块，事件 handler 清单 | 无空 schedule 解析；booted 后装配；disabled 无事件/任务 |
| T29 | module:file、缺 module、路径穿越输入 | 正常发布 URL 映射；缺身份失败；越界拒绝并记录安全差异 |
| T30 | A 成功、B boot 失败；App1/App2并行请求/job scope | 失败逆序释放、无监听；资源/请求/job状态不跨 App 或 scope 泄漏 |

### 子簇依赖与并行所有权建议

- **R2基础**：Container/ServiceProvider/config 唯一内核和配置时序先收敛，修正草稿冲突；T07/T17/T19/T20/T30 为模块接入前门槛。核心 scoped 根生命周期、request/job scope 另按 Laravel container 合同验收，不能以模块合同覆盖它。
- **R2-M1**：模块身份/编译清单/priority/状态读取与启动快照（T01–T04、T08–T13）；不依赖完整 CRUD，也不要求 R13 CLI 全完成才能接入。
- **R2-M2**：配置/路由/资源/迁移贡献（T16–T29），依共享内核和相关领域可用 API；同步 modules/example 真实使用代码，缺后端能力保留缺口。
- **R13-M**：状态写入、管理/生成/发布 CLI 和完整生成选项（T05/T06/T14/T15/T21/T29）；先与内核共用公开管理契约，不造命令私有 registry。
- 首批并行写入建议：一人独占核心 Container/Provider/config 与共享清单；一人独占模块 descriptor/repository/activator 和对应测试；一人只读核对上游并维护本矩阵；资源领域适配在公开接口稳定后分别独占。Cargo/lock、共享文档、总集成始终单写。
- 尚未闭合：完整生成器/force/plain/custom namespace/stub 选项、所有命令退出语义/缓存失效、完整发布器、Facade/module helper 全集、事件/模型/工厂及各生态页面。不得将上述30项或35导航作为完整盘点结论。

## 固定源码与测试索引

所有链接固定同一 commit；行号是本次读取定位。测试断言已阅读，未运行上游 PHP 套件，也未执行尚不存在的 Rust T01–T30。

[S1]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/ModuleManifest.php#L56-L146
[S2]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/FileRepository.php
[S3]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/Activators/FileActivator.php#L54-L158
[S4]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/Module.php#L174-L373
[S5]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/Commands/BaseCommand.php#L92-L183
[S6]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/ModulesServiceProvider.php#L26-L35
[S7]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/LaravelModulesServiceProvider.php#L41-L161
[S8]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/Generators/ModuleGenerator.php#L365-L420
[S9]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/Support/ModuleServiceProvider.php#L44-L234
[S10]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/src/Commands/stubs/route-provider.stub
[U1]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/tests/LaravelFileRepositoryTest.php
[U2]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/tests/Activators/FileActivatorTest.php
[U3]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/tests/Commands/Actions/EnableCommandTest.php
[U4]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/tests/LaravelModulesServiceProviderTest.php
[U5]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/tests/Support/ModuleServiceProviderTest.php#L37-L102
[U6]: https://github.com/nWidart/laravel-modules/blob/59714840708a2aece19e810764473f53227c35fe/tests/Migrations/MigratorTest.php#L37-L134
