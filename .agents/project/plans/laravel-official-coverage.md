# Laravel 官方生态覆盖与规划缺口

> 状态：当前覆盖校准；不新增平行任务队列，不表示能力已实现。
> 范围：固定Laravel文档导航、102章/749节矩阵、现总计划和官方组织独立工具；不把组织所有仓库当作框架功能。

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
| Pulse L095、Telescope L101 | 采集/存储/过滤/采样/脱敏及资源开销；管理视图与未启动可选模块边界分别记录，底层不因UI未开工而消失 |
| Pail/Tinker（L029）、Prompts/Dusk/Sail/Envoy | 已有文档条目，但Rust对应命令、开发流程、交互/浏览器/部署及语言约束须专项Spec；不直接运行PHP工具 |

Head、Fortify/Passkeys、Sanctum、Socialite、Reverb、Vite、WorkOS相关starter-kit体验和Slack/SMS通知也已有章节/能力入口，不应误报为完全未规划。官方独立包源码/版本和前端配套往往尚未冻结，这是细化缺口。旧“后续按需求”措辞不能自行豁免已固定章节；按原UI/托管产品边界区分底层对应与未来应用。

## 官方独立工具尚无明确对应的部分

本轮按官方组织仓库、README及项目语义核对；文字搜索无包名只作为线索，已排除echo/stream/head等同名shell/协议词，不能以匹配数量作完成度。

| 官方工具 | 当前缺口 | 处理建议与边界 |
| --- | --- | --- |
| [Wayfinder](https://github.com/laravel/wayfinder) | 没有明确路由/控制器到TypeScript调用定义的对应contract | 在R3路由元数据→R9/R13前端与生成器职责评估；公开元数据和Rust类型映射先Spec，不把普通OpenAPI清单等同功能完成 |
| [Laravel Stream](https://github.com/laravel/stream) | 有SSE/AI流规划，但未明确官方React/Vue/Svelte hooks的接入对应 | 与AI SDK流输出/R9前端适配联合Spec，优先既定Vue使用体验，核对实际协议及取消/错误/重连；不启动后台业务，不与正文加密适配重复造轮子 |
| [Laravel LSP](https://github.com/laravel/lsp)、[Surveyor](https://github.com/laravel/surveyor) | 尚无官方开发工具的适用性分类记录 | PHP/Laravel源码分析与编辑器能力不可原样套Rust；先比较rust-analyzer/Cargo/Clippy及公开框架元数据可覆盖的行为，再记录必要差异与剩余缺口，不另造通用Rust LSP或解析器 |
| [Maestro](https://github.com/laravel/maestro) | 未单列适用性说明 | 上游用于协调维护starter-kit仓库，不直接等同开发者运行时功能；按现单仓/唯一示例约束评估维护流程，不因此创建多仓库或一组新starter应用 |

以上新增发现先进入R0/R13/R16分类和具体Spec，尚未取得与AI SDK相同的独立实施排期；不从“官方组织有仓库”推定全部立即开工。Echo、vite-plugin、passkeys-server、通知渠道等源码包还应在已存在章节中补明确来源/兼容矩阵，而不是重复建立同功能新计划。

## 防止再次漏落实

- 官方能力保持三层证据：固定章节/源码→职责与Spec→唯一todo可领取功能簇。只登记名称或稳定ID不算“规划已完善”，文档项状态与代码验收分别报告。
- 按功能簇补齐泛化矩阵的API/default/错误/测试与真实服务前置；官方可选包按实际前置在阶段二分批实施，最终证据汇入R14，不反向等所有R14完成产生循环。
- 官方AI SDK是主要对标，Prism为第三方补充；Laravel官方MCP包定义框架体验，rmcp只作为协议组件。不能拿第三方包替代官方语义，也不能把SDK升级擅自变更内核固定基线。
- 发布与跟进按既定冻结来源/差异流程盘点新增官方工具。闭源商业运营、托管服务、已归档工具、PHP运行环境及未授权UI/产品逐项说明边界，不机械扩成V1硬任务。

来源：[Laravel 13.x文档](https://laravel.com/docs/13.x)、[固定导航源码](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/documentation.md)、[当前导航源码](https://github.com/laravel/docs/blob/13.x/documentation.md)、[Laravel官方仓库目录](https://github.com/orgs/laravel/repositories)。当前导航/仓库状态会变化；本审计没有验证全部独立仓库的成熟度、固定版本、所有正文差异或产品可行性，不据此宣称官方生态已全部规划完善。
