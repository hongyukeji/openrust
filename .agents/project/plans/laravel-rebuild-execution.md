# 全面重构执行任务

> 当前代码与目标：源码仍有 `app/example/` 迁移 WIP；最终业务位置为 `modules/<模块名>/`，唯一示例为 `modules/example/`。目录和模块能力尚未完成验收，按 [ADR-074](../decisions/adr-074-laravel-modules-directory.md) 推进，不能将目标路径当作已实现。

> 受众：执行本次重构的 AI Agent
> 更新：2026-10-28
> 用途：用户明确要求的可复制执行任务说明；不保存聊天恢复日志。范围唯一来源是全面重构计划，状态唯一来源是 todo.md。

请在 OpenRust 仓库执行 `.agents/project/plans/laravel-rebuild.md`，按 ADR-070 落实正式定位 Laravel + laravel-modules = OpenRust：固定基线的功能与可观察语义逐项 Rust 1:1 实现。

## 已获授权

- 产品目标是固定Laravel框架与laravel-modules模块化能力的Rust 1:1实现，并长期跟进；逐项覆盖功能、可观察语义/默认值/错误安全边界、register→boot和扩展/CLI/生成/测试/部署；默认保持其目录、概念、命名、默认值、工作流和扩展方式。必要 Rust 适配与版本差异依总计划[长期上游跟进](laravel-rebuild.md#长期上游跟进)记录和验收，不自行降低为仅部分概念相似的框架。
- Laravel固定文档/源码/测试是行为验收标准，laravel-modules独立来源但同内核，Goravel只作实现参考；不是目录或命名相似即完成。必要Rust差异与用户约定按总计划分类，不以困难将适用核心标不适用。Laravel 是能力、概念、默认流程和开发体验的固定标准，Goravel 是跨语言落地参照。允许目录、架构、公共 API、默认值和实现推倒重来，无旧接口或历史数据兼容义务。
- 已有优秀能力必须按 Laravel 对应机制实现和使用；保留价值及有效行为，旧 API/独立体系可重写。R0 记录上游扩展机制、Rust 公开入口、生产调用链与回归证据；统一返回格式通过 Response / API Resource 与异常渲染扩展提供便捷接入和定制，R3 定稿默认值。不以改名、移目录或套 Provider 冒充融合，不因上游无相同内置能力而删除，也不强制包装所有协议响应或改成 HTTP 200。
- Rust 类型、所有权、trait、async、Cargo 的便利性必须保留。不得用动态字典、全局可变单例、隐式阻塞 I/O 或 PHP 解释器模拟来追求外形一致。
- 旧 ADR/规范中与新方向冲突的约束按 ADR-070 替代；当前代码是已实现事实，不能用“旧规定不允许”拒绝本次重构。普通设计决策自主完成并记录，不重复询问是否允许重写。
- 已有 WIP、他人工作、用户数据与共享服务必须保护；发布、推送、tag、凭据和项目外写操作仍遵守授权边界。

## 开始和推进

1. 读取 AGENTS.md、`.agents/project/README.md`、context.md、todo.md、plan.md、ADR-070 和全面重构计划；核对 Git HEAD/status、worktree、进程/其他代理、已完成提交和最近验收。
2. 根据todo.md领取当前可执行子簇；已完成基线不重做。模块包正式矩阵尚待逐项细化，不把Laravel原749行改成模块包已盘点。基线盘点须固定 Laravel/Goravel 版本和源码 commit，逐章建立能力/子行为/现有资产/差异/验收矩阵；随后直接进入 R1，不把完成计划当作完成重构。
3. 当前先R1-S结构门禁/限定提交，再R2基础Container/Provider/配置/TestApp，再R2-M模块最小闭环/目录迁移，再R1-E真实CRUD/当前能力示例贯通；结构不等于R1整体已完成，GET信息响应不是CRUD。R2基础不等待完整R1-E，R1-E不等待所有未来框架能力，防止循环。其后按R0–R16依赖连续推进；一次一个主功能簇，内部可分派不冲突的子任务。每簇执行 Spec → Implement → Verify → Commit，写明公共 contract、生命周期、事务、错误与资源边界。
4. 根采用 Laravel 应用职责：app/bootstrap/config/database/public/resources/routes/storage/tests；同 package openrust 提供框架 src/lib.rs 与默认 server 薄 src/main.rs，default-members=["."]、default-run="server"。唯一 modules/example 作为普通业务模块由根 app/mod.rs、bootstrap 与 routes 装配，不是独立应用/package/bin，不新增 example feature；保护有效代码和测试后删除原双应用与根 examples。业务集中在 modules/；其余目录均为框架通用结构和代码；示例专有迁移/种子/模板/proto/生成脚本与测试 fixture 留模块内，根仅通用公开挂载，不因示例保留 build.rs/build 依赖或样例特判。CLI 模板仍提供用户应用生成功能，只在 /tmp 验证。按 ADR-074 重新验收，目录迁移不能替代框架等价证明。
5. 完成交付矩阵的全部适用机制：内核/Provider/Container/Facade、HTTP/FormRequest/Resource、ORM/关系/Factory、认证授权、缓存/存储、视图/国际化/通信、事件/队列/调度、网络/广播、观测、Artisan 式 CLI、测试与生成器、生产集群、V2 和已纳入的生态可选包。子行为、排期和证据以总计划为准，不私自缩小范围。
6. 已有实现可复用、适配或重写；复用必须通过新 contract 测试。删除失效代码、旧别名和重复内核，迁移全部调用方、模板、示例、CI 与文档，不长期并存两套框架。

## 模型和协作

- 已定 contract 的实现和测试优先派给 `gpt-6.1-sol` 子智能体；普通任务使用适中推理，提供最小必要上下文和明确路径，节约 token。
- 按环境支持使用本地主控做轻量编排；指定模型不可用时如实说明并选成本合适的可用模型。不要仅凭风险词升级到最贵模型。
- 当前及以后直接在本仓库 main 开发和本地提交，不创建额外源码仓库、开发分支或隔离 worktree。同一功能簇内，主控明确划分互不重叠的文件所有权后可并行实现；存在依赖的任务按顺序推进。Cargo manifests/lock、大规模迁移、共享文档与最终提交由集成负责人协调，同一共享文件不得并发写入；构建 target、端口和服务资源预先分配，清理前确认无其他任务占用。worker 不递归派发、不擅改整体 contract、不推送。高风险代码由不同模型或 Provider 独立审查。
- 按当前工具的异步任务规则汇报启动和完成；不虚报后台任务已运行，不把计划中的验证写成已执行。

每项闭环必须是固定上游源码/测试→Rust公开API/配置→等价行为测试→modules/example实际代码→文档→适用真实服务证据；用户约定、增强、必要Rust差异与未闭合缺口独立记录。无法等价不声称严格1:1，商业/闭源运营、AI产品、真实付款和对外动作不因本定位扩大。

## 必须实际验证

- 每簇开工、审查和提交前执行总计划[纠偏规则](laravel-rebuild.md#执行偏离的纠正)：追踪实际生产入口、默认值、示例与生成器，以可区分旧、新行为的证据验收。发现偏离先纠正再继续依赖该设计的开发；未就绪项进入唯一 todo，不能只改名包装旧实现后报告目标完成。
- 每个代码簇执行适用 fmt、workspace clippy/test/doc、cargo deny、feature/MSRV 门禁；失败先修复。生成项目执行 check/test 和真实启动/HTTP 流程。
- PostgreSQL 18/MySQL 8.4 LTS 双库；SQLite 单机支持按阶段纳入。真实复制验收延迟/追平/强制主库/故障恢复，不能用两个独立数据库替代。
- 真实 Redis、S3、SMTP、HTTP/WebSocket/SSE/gRPC、Collector；各数据库分别验收负载均衡、两个 API、多 worker/scheduler 和共享状态/文件/广播/恢复。外部 SMS/支付等仅用授权目标和凭据。
- 根目录普通 `cargo run` 和真实 HTTP 启动实际成功；额外 --bin/--package 或命令别名不能替代。
- 验证单机部署、完整集群、资源上限、性能、干净环境黄金路径与 Laravel 使用者任务对照；没有真人走查时注明 Agent 证据。

## 持续交付

用户明确要求持续完成全面重构时，总控先检查本线程持久 Goal；没有目标则创建，与目标一致且 active 则沿用，不自行设置未授权的预算。目标覆盖当前获准的全部适用阶段及真实验收，不能缩成单个子簇或派发任务。Goal 的实际状态以工具结果为准，不以提示词或口头承诺代替。

子代理完成后，总控必须收取结果、核验变更与验证证据、协调限定提交并领取后继任务；等待时使用任务等待/收件机制，不以“已派发”结束整个目标。按需独立推进非冲突工作，禁止重复派发同一写入范围。未达整体验收时保持目标未完成，跨轮次接续依赖原生 Goal；不另建正常接续定时器或重复发送“继续”的循环。避免为了查询同一在途任务或重复汇报而频繁开启短轮次。

### 整计划执行守护

由AI结合上下文理解持续执行意图（包括“持续开发”）并自动调用 [continuous-development-guard](../../skills/continuous-development-guard/SKILL.md)，按可信环境登记本任务原Goal与Paseo/Codex线程；普通任务不启用，子代理不重复登记。入口为 `python3 -B .agents/skills/continuous-development-guard/scripts/guard.py start`，`status` 检查运行事实，`stop --stop-task` 响应用户明确停止。

- 原生Goal负责正常接续；独立监督进程持单实例锁，监控恢复worker并在异常退出后重启。worker默认每120秒只读核对实际状态，并限制日志尾部读取量，正常运行不打断；异常或整目标active且idle超过120秒时退避恢复原任务，不另建开发者/写入者。
- 临时失败按60/180/600秒加抖动恢复，三次后转30分钟冷却，持续监测而非退出；只在新一轮有工具调用且正常结束后重置失败计数。发送前持久化意图，结果未确认先观察原线程，不立即重发。
- 额度通过正式 `account/rateLimits/read` 检查，等待自然重置后复核；不消费额外credits、兑换reset、切账户或擅改模型。认证/权限、预算、真正外部blocked与目标身份变化进入待处理并本机通知，保留监测；外部缺口继续独立项的责任在总控。
- 暂停、取消和用户停止优先。停止先持久禁止唤醒，再暂停Goal/中断主控；只回收本任务监督/worker和其专属协议/CLI进程树及PID文件（POSIX进程组，Windows自有Job Object），不碰开发Agent、构建或基础服务。整Goal完成后Agent自动调用finish及时清理，周期检查亦能自动清理；完成真实性由计划矩阵与实际验收负责，脚本不能独立证明语义等价。
- 项目 `.agents/project/guards/registry.json` 为运行看板，配置/状态/精简日志在忽略的 `.agents/project/guards/<agent>/`；保留空锁文件inode避免并发start双实例，退出时释放实际锁。明确新授权的start可清除旧停止标记，不自动扩大目标范围。无完整身份时拒绝猜测最新会话。
- 使用标准库与已安装的Python 3.10+/Paseo/Codex，复用现有环境，不自动安装软件、改系统或认证。macOS/POSIX使用文件锁与进程组，Windows使用字节锁、创建时间身份和先绑定再放行的Job Object；不通过全局进程名或裸PID杀进程。平台验收范围与Windows原生未验证项见[按需参考](../../skills/continuous-development-guard/references/platforms.md)。正常监测无模型调用，watch.log有容量上限；不安装系统自启动服务。worker退出可自恢复，但电脑重启、监督进程自身被杀或客户端协议变化仍需重新启动/修复；本机通知不可用时保留通知失败状态和日志，不声称已通知送达。恢复入队不能当作模型恢复或任务完成。

压缩或恢复后重新核对 Git、todo、在途任务与 Goal。用户暂停、额度或工具限制按真实状态处理并报告；外部阻塞保留证据缺口并继续独立项。不能为保持运行虚报完成、删除范围或越过授权边界，也不承诺忽略暂停及运行限制的无限执行。

每簇同步 todo/plan/progress、领域 ADR/core、用户文档、生成器和验收记录，只本地提交本簇变更；不 push、不发布、不打 tag。前台及时汇报有证据的进展，普通问题和设计分歧自行解决，不在分析阶段反复停下。

外部条件缺失时记录缺口并继续所有不依赖项，不虚报完成；遇到工具/授权硬边界如实报告。最终以总计划矩阵、真实验收、独立审查和交付证据判断完成，不能仅以“已重构”“已编译”“所有计划已更新”结束任务。
