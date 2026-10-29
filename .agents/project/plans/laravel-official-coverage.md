# Laravel 官方生态覆盖与规划缺口

> 状态：固定范围全量开发已获授权，按两阶段进入唯一todo；本文件说明范围与验收，不表示能力已实现。
> 范围：固定Laravel文档导航、102章/749节矩阵、现总计划和官方组织独立工具；不把组织所有仓库当作框架功能。

## 全量交付授权与两阶段

固定Laravel核心/官方可选包和正式laravel-modules适用能力必须全量实现；阶段二不是无限期候选池。命名、配置、默认行为、生命周期、错误/安全边界、扩展与CLI/生成/测试/部署体验以固定官方源码和测试为准。Prism等第三方只补充，不改变官方标准。每包独立冻结源码/tag/文档/测试和支持面，不能只固定内核版本就宣称生态基线齐全；未完成项持续保留缺口。

| 阶段 | 全量交付职责 | 不得遗漏 |
| --- | --- | --- |
| 一：核心与模块内核 | 固定Laravel核心机制、通用CLI/生成/测试/部署及laravel-modules本身的发现/注册/启停/依赖/资源/完整命令与生成扩展 | 配置、HTTP、数据库/ORM/迁移、认证授权基础、Session/安全、缓存/存储、事件/队列/调度、邮件通知、视图/i18n等不能挪入可选包以减少核心范围 |
| 二：官方可选包及已授权增强 | AI SDK/MCP/Boost，Cashier/Fortify/Folio/Head/Horizon/Octane/Passport/Pennant/Precognition/Pulse/Reverb/Sanctum/Scout/Socialite/Telescope及官方开发工具；配套前端、额外驱动、自建模型服务与已授权增强按依赖交付 | Pail/Tinker、Dusk/Envoy/Prompts/Sail/Pint/Valet/Homestead等需记录Rust工具/运行时对应；Wayfinder/Stream、Echo/Vite、Passkeys及通知渠道按其职责补齐，不因按需启用删除适用能力 |

矩阵逐条增加交付阶段，保留原ID/来源/子行为/状态；阶段是计划归属，不是完成标记。混合章节分别验核心与包行为，不能一半完成就关闭整条。MongoDB等额外驱动可阶段二交付，PG/MySQL/SQLite等既定核心后端按原前置；所有承诺后端仍各自验完整适用链路。

Horizon/Pulse/Telescope等官方包自带Dashboard、授权和必要页面/资源也在全量范围，通过对应可选包交付，不默认加载/公开；不能用“业务后台暂不开发”省略它们。官方starter-kit和工具的前端配套按固定支持面验收，可按Vue优先分批，但不能以优先级删除其他适用行为。若依/芋道式业务后台、uni-app业务套件及AI模型平台继续暂不开发，本次不启动这些产品。

LSP/Surveyor等的框架感知/元数据/编辑器体验也须对应：先举证rust-analyzer/Cargo/Clippy及公开元数据覆盖，再补框架特有缺口，不默认重造通用语言服务器。Maestro等内部维护工具做功能/适用性分类，不等同运行时接口。PHP执行器、闭源商业平台运营及已明确未启动产品仍按既定边界记录；不能用这些边界豁免公开框架能力或用“Rust惯用”跳过困难功能。无法等价则保留未完成与明确差异，不伪称严格1:1。

依赖顺序：核心/模块基础→阶段一门禁→各官方可选包前置Spec与实现→阶段二整合及R14最终门禁。包的共享机制可阶段一先交，但不代表包全量完成；依赖包的模块资源集成可随阶段二闭环，保留模块ID。现有MQTT/gRPC/动态设置等已授权增强继续按职责组合，不建立平行Provider/配置/队列/路由体系。内嵌AI推理仍是后置条件评估，不因全量SDK而强制部署模型。

所有功能簇按Spec→Implement→Verify→Commit，具备官方源码/测试→RustAPI/config→行为验证→唯一示例→生成项目/文档→真实服务证据；仅改目录/名称、文档列项、编译通过或fake都不算完成。外部账号、服务/硬件或许可缺口明确保留，继续独立项，不自行付费/发布。版本按本批冻结，后续上游差异进入跟进批次，不因浮动master无限扩张本批范围。

## 核查结论

固定文档commit `960eedf0911155ad16326ade49c0ea5d18eda0b8` 与本次读取的13.x导航均为102个唯一章节，三个矩阵覆盖相同102章、合计749个稳定二级节；章节名差集为空。这只证明导航登记覆盖，不证明三级行为完整、后续正文无变化、已有可执行Spec或完成验收。

AI SDK早已在[矩阵三](laravel-rebuild-matrix-3.md) L071.01–17和[固定基线](laravel-rebuild-baseline.md#文档导航外的官方生态)登记，问题是缺少清晰的独立Spec、依赖排期及汇报。现[AI专项](ai-capabilities.md)已由用户明确授权开发，并进入唯一todo；不能再称从未纳入或是否实现仍待R0决定，也不能把当前新增专项说成已完成SDK实现。

“已登记未细化”“完全缺少明确对应”“语言/产品边界待评估”必须分别报告。矩阵中的待真实验收也不自动等于没有旧代码资产；旧资产须按新contract重验，不能凭包名或目录宣称完成。

## 已登记但需收敛可执行Spec的重点

下表不是新增范围；归属已同步矩阵目标/验收入口，稳定ID、固定来源和未验收状态不变。实现时仍按唯一todo依赖领取，不能把本表当第二队列。

| 能力与既有ID | 应收敛的真实职责/缺口 |
| --- | --- |
| AI SDK L071 | 官方SDK源码基线与逐项对应、Provider/Agent/多模态/工具/测试及真实供应商；专项与排期已有，尚未实现 |
| MCP L072 | 官方可选包Server与Client、Tools/Prompts/Resources/Apps、鉴权与双向互操作；不能降为非官方增强或仅Server |
| Boost L073 | 开发MCP工具、文档检索、Guidelines/Skills及开发客户端接入；不是模型产品，仍缺专项contract |
| Cashier Stripe/Paddle L080/L081 | 订阅/账单/支付生命周期、webhook验签重放幂等及sandbox；不是普通支付HTTP接口，非核心可选包 |
| Folio L084 | 文件路由与视图、中间件/参数绑定、命令及生成应用；不是泛化可选包名 |
| Horizon L087 | Redis队列worker/supervisor、自动平衡/通知/监测；不能仅映射遥测或用默认DB队列替代官方对应 |
| Octane L089 | 常驻请求/Container隔离、并发任务/定时器/缓存/重载及资源回收；Tokio或常驻server存在不是等价证明 |
| Passport L090 | OAuth2授权服务器/客户端/授权和撤销边界；不能用外部OIDC登录客户端替代 |
| Pennant L091、Scout L099 | 运行时特性作用域/持久driver；模型索引同步/真实搜索引擎；不能都泛化成遥测 |
| Precognition L093 | HTTP预校验、授权/验证、Vue/JS交互及无业务副作用；不能归为CLI骨架 |
| Pulse L095、Telescope L101 | 采集/存储/过滤/采样/脱敏及资源开销；包自身工具视图属于阶段二，业务后台仍冻结；采集与界面分别验收 |
| Pail（L028）/Tinker（L029）、Prompts/Dusk/Sail/Envoy | 已有文档条目，但Rust对应命令、开发流程、交互/浏览器/部署及语言约束须专项Spec；不直接运行PHP工具 |

Head、Fortify/Passkeys、Sanctum、Socialite、Reverb、Vite、WorkOS相关starter-kit体验和Slack/SMS通知也已有章节/能力入口，不应误报为完全未规划。官方独立包源码/版本和前端配套往往尚未冻结，这是细化缺口。旧“后续按需求”措辞不能自行豁免已固定章节；按原UI/托管产品边界区分底层对应与未来应用。

## 补入全量范围的官方独立工具

本轮按官方组织仓库、README及项目语义核对；文字搜索无包名只作为线索，已排除echo/stream/head等同名shell/协议词，不能以匹配数量作完成度。

| 官方工具 | 当前缺口 | 处理建议与边界 |
| --- | --- | --- |
| [Wayfinder](https://github.com/laravel/wayfinder) | 已纳入，待路由/控制器到TypeScript调用定义的对应Spec | 已纳入阶段二R3元数据→R9/R13前端与生成器对应；公开元数据和Rust类型映射先Spec，不把普通OpenAPI清单等同功能完成 |
| [Laravel Stream](https://github.com/laravel/stream) | 已纳入，待官方React/Vue/Svelte hooks与既有SSE/AI流的对应Spec | 已纳入阶段二，与AI SDK流输出/R9前端适配联合Spec，优先既定Vue使用体验，核对实际协议及取消/错误/重连；不启动后台业务，不与正文加密适配重复造轮子 |
| [Laravel LSP](https://github.com/laravel/lsp)、[Surveyor](https://github.com/laravel/surveyor) | 已纳入，待开发工具逐行为适用性与Rust等价证据 | 已纳入阶段二开发体验对应；PHP/Laravel源码分析与编辑器能力不可原样套Rust；先比较rust-analyzer/Cargo/Clippy及公开框架元数据可覆盖的行为，再记录必要差异与剩余缺口，不另造通用Rust LSP或解析器 |
| [Maestro](https://github.com/laravel/maestro) | 已纳入，待内部维护流程适用性说明 | 已纳入维护流程适用性分类；上游用于协调维护starter-kit仓库，不直接等同开发者运行时功能；按现单仓/唯一示例约束评估维护流程，不因此创建多仓库或一组新starter应用 |

以上适用功能已获全量覆盖授权，进入唯一todo的阶段二职责簇，由对应前置决定实施顺序，不再停留为无期限候选；官方组织内部/归档/商业仓库仍先分类，不推定全部是运行时功能或立即并行开工。Echo、vite-plugin、passkeys-server、通知渠道等源码包还应在已存在章节中补明确来源/兼容矩阵，而不是重复建立同功能新计划。

## 防止再次漏落实

- 官方能力保持三层证据：固定章节/源码→职责与Spec→唯一todo可领取功能簇。只登记名称或稳定ID不算“规划已完善”，文档项状态与代码验收分别报告。
- 按功能簇补齐泛化矩阵的API/default/错误/测试与真实服务前置；官方可选包按实际前置在阶段二分批实施，最终证据汇入R14，不反向等所有R14完成产生循环。
- 官方AI SDK是主要对标，Prism为第三方补充；Laravel官方MCP包定义框架体验，rmcp只作为协议组件。不能拿第三方包替代官方语义，也不能把SDK升级擅自变更内核固定基线。
- 发布与跟进按既定冻结来源/差异流程盘点新增官方工具。闭源商业运营、托管服务、已归档工具、PHP运行环境及未授权业务UI/产品逐项说明边界，不机械扩成V1硬任务。

来源：[Laravel 13.x文档](https://laravel.com/docs/13.x)、[固定导航源码](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/documentation.md)、[当前导航源码](https://github.com/laravel/docs/blob/13.x/documentation.md)、[Laravel官方仓库目录](https://github.com/orgs/laravel/repositories)。当前导航/仓库状态会变化；本审计没有验证全部独立仓库的成熟度、固定版本、所有正文差异或产品可行性，全量纳入开发不等于全部独立包Spec/版本已冻结或能力已实现。
