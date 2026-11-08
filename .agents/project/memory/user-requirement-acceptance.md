# 真人需求验收对账

> 受众：架构协调、总控与独立验收 Agent；核对日期：2026-11-06。
> 时间窗口：北京时间2026-10-31至11-06本次请求；本对话实际检索到11-02至11-06的243条相关真人消息。
> 基线：main已提交5bd13be8，加当前WIP与正式Goal/自动化只读状态。仅审计，没有新跑构建或删库。

## 读取与维护规则

- 62个归并事项是需求验收对账单位，不是框架功能数量，不能计算或替代核心38.3%的功能清单比例。重复进度查询、重复催办合并；普通解释问答不凭空变成编码任务。
- “已完成”只针对列明边界；持续规则的多批执行证据不保证未来自动满足。“部分完成”“明确遗漏”“待提交”均不得在汇报时改成已交付。发送/已安排不作验收依据。
- 原始真人消息、完整ID、时间和去重索引保存在[审计证据目录](../../../tmp/user-requests-audit-20261106/requirement-map.json)；表中U编号对应该目录的source-user-messages.json零基索引。此页不复制完整聊天/执行日志。
- 本页维护需求与验收的对应关系，不建立第二开发队列；实际领取、依赖、owner和停点仍只在[唯一todo](../todo.md)。下面“路由”均指已有任务或规范入口，新发现残留由总控归入相应项。
- 关闭事项前必须核“原始要求→实际源码/配置/根文件→生成模板/唯一示例/调用方→指南/Agent当前事实→匹配验证→提交及获准推送”。删除要求要核Git实际删除，不仅核默认路径不再读取。
- 同一事项发现后继缺陷，保留此前限定交付证据，新增或更新剩余边界，不把旧通过宣称为当前修复通过，也不抹掉真实成果。
- 不把历史日志中的旧口径当当前文档错误；当前README、指南、context/core事实中的过时口径须修正。每次需求变化、验收回执或提交后，更新对应行和实际证据，不以“已发给总控”关闭。

## 重点事项当前结论

1. R08/R11：5bd13be8已完成同Goal正文修订恢复，checkpoint=continue、自动化ACTIVE，并有新的AR-N01 Routing工具证据；391项通过、3项Windows跳过，原生命周期失败与未确认根因保留。不启用Architect守护。
2. R39：根modules_statuses.json及无用旧模板已按真人直接要求git rm，文件已不存在且其他索引条目未变；提交状态以Git为准，旧URL工具整改仍为候选。
3. R41：configuration指南、authentication指南和CLI README的配置默认已统一为api；最终URL保留/api。本次文档校正已应用，提交状态以Git为准。
4. R56：已清旧恢复码/独立MFA调用指引，更新已验profile与配置/队列/推送当前事实；保留真实客户端及完整能力缺口。文档校正已应用，提交状态以Git为准。
5. R12/R13：已有并行产出，但当前各支线持续性证据不足，守护修复仍占主线；不是派发后就算持续并行。
6. R59/R60及本记录：文档与清单已落盘并验证，按本次纯文档范围由总控唯一索引提交；不夹未验生产源码。

## 逐项对账

| ID | 真人交代 | 来源 | 核对结论 | 已落实与证据 | 未闭合边界 | 唯一路由 |
| --- | --- | --- | --- | --- | --- | --- |
| R01 | 汇总项目聊天并核对最近交代 | 11-02/11-06；U001,U018,U188,U242 | 本次对账已完成 | 本次检索本对话窗口内243条真人消息，重复进度查询与催办合并，按最终决定核对源码/Git/证据；[source-user-messages.json](../../../tmp/user-requests-audit-20261106/source-user-messages.json) | 原始消息与证据保存在项目tmp，不是全项目所有对话的全文档案；提交状态以Git为准 | 本记录 |
| R02 | 品牌只强调Laravel，Modules作为默认扩展 | 11-02；U011 | 已完成 | 3eae8df6已统一对外标题，范围仍包含Modules；[README.md](../../../README.md)；[AGENTS.md](../../../AGENTS.md) | 内部历史来源标题保留不等于当前品牌；新文档继续遵守 | ADR-001 |
| R03 | 持续按Laravel完整核心及官方生态对照开发 | 11-04/11-05/11-06；U064,U065,U066,U067,U100,U120,U183,U226 | 部分完成 | 固定来源/矩阵及唯一todo存在，多批子范围交付；核心清单72/188已验；[core-functional-acceptance.md](core-functional-acceptance.md)；[todo.md](../todo.md) | 全部适用核心/官方生态仍未全量交付，不能因目录或单簇通过关闭全部 | R0–R16唯一todo |
| R04 | skill在Codex真实使用并提交 | 11-02；U003,U004,U008,U012,U013 | 历史限定已完成 | 18ccb209/5112e071完成原生/同框合作式接入，有定时投递、接收、自检及业务工具证据；[SKILL.md](../../skills/agent-continuous-development-guard/SKILL.md)；[cooperative-acceptance-complete.json](../../../tmp/guard-desktop-20261102/cooperative-acceptance-complete.json) | 当前新正文修订故障归R08，历史成功不抵扣当前异常 | Desktop守护 |
| R05 | 修复额度资格未知与解析问题 | 11-02/11-04；U005,U052,U053 | 历史限定已完成 | f0aa388b、9ac8a86b、43994d4a、f6ba45ce等已修正式包装/重置精度/暂态恢复；[guard_heartbeat.py](../../skills/agent-continuous-development-guard/scripts/guard_heartbeat.py)；[testing.md](testing.md) | 新故障须按原因分别处理，不把解析异常说成额度耗尽 | Desktop守护 |
| R06 | 关闭删除Architect守护且不重启 | 11-02；U006,U007,U010,U012,U013 | 已完成 | 正式删除回执、当前配置清单与无Goal相互印证；[architect-guard-deleted.json](../../../tmp/guard-desktop-20261102/same-thread-investigation/architect-guard-deleted.json) | 继续保持关闭，不创建本对话Goal | 仅总控守护 |
| R07 | 降低频率、持续优化守护 | 11-04；U059,U060,U061 | 规则已落实，持续执行 | 119339f1默认30分钟，0d3b4fd0记录真实故障驱动迭代；[SKILL.md](../../skills/agent-continuous-development-guard/SKILL.md) | 当前自动化PAUSED不是正常运行；后续按R08解除 | Desktop守护 |
| R08 | 总控同框守护、忙碌时也能续作 | 11-02/11-04；U008,U009,U012,U013,U054,U055,U057 | 本次已修复并实际续作 | 5bd13be8正式接纳同Goal修订、checkpoint continue、自动化ACTIVE及新AR-N01 Routing工具；[恢复回执](../../../tmp/guard-objective-revision-20261106/live-recovery-receipt.json) | 391通过/3Windows跳过；历史生命周期失败及未确认根因保留，不承诺App不可用时准点或硬预算中断 | 现守护正文修订有限簇，最高当前阻断 |
| R09 | 暂停/换号/额度恢复保留原Goal及预算历史 | 11-02/11-03/11-04/11-06；U020,U021,U043,U044,U074,U229 | 历史限定已完成 | 原Goal ID保留；501e4a25实际恢复checkpoint与AR-N01业务工具有证据；[live-recovery-receipt.json](../../../tmp/desktop-budget-rebase-20261106/live-recovery-receipt.json)；[context.md](../context.md) | 当前R08为之后新故障，不沿用早前恢复回执虚报当前正常 | 守护原Goal |
| R10 | 本次取消额度保留线及后续正式预算恢复 | 11-06；U128,U229 | 阶段授权已落实 | fc6340a5落实当前任务完整普通额度覆盖；U229仅要求额度恢复，百分比重建另依总控保存的真人“允许授权”正式证据，501e4a25收口并保留历史/credits=false；[context.md](../context.md)；[full-speed-20261106](../../../tmp/full-speed-20261106)；[authorization-evidence.json](../../../tmp/desktop-budget-rebase-20261106/authorization-evidence.json) | 授权按生命周期，不是永久100%或消费credits许可；不因本审计重放授权 | 预算正式政策 |
| R11 | 总控Goal目标正文纠正 | 11-06；U231,U232,U234 | 正文与本次守护联动已完成 | 新版Goal保持原ID且active，5bd13be8完成受限正文采用及实际业务续作；[回执](../../../tmp/guard-objective-revision-20261106/live-recovery-receipt.json) | 原Goal及预算历史保留，后续修订仍须按真实来源和正式入口核验 | R08 |
| R12 | 持续并行、空闲自动接续 | 11-04/11-05/11-06；U068,U069,U075,U076,U114,U128,U131,U132,U133 | 部分完成 | 多次真实worker/独立审查与namespace候选存在，角色分工已落地；[todo.md](../todo.md)；[plan.md](../plan.md) | 尚不能证明SQLite/Passport/数据库翻译每条均持续编码；需按近期真实工具/产出确认，不靠owner名单 | 总控核心与并行分工 |
| R13 | 核心主线不被扩展/SQLite/守护长期占用 | 11-05/11-06；U113,U114,U118,U121,U126,U131,U132 | 部分完成 | 核心/扩展分工已记录，Query/Model/框架/Modules有实交付；[todo.md](../todo.md)；[plan.md](../plan.md) | 当前总控再次投入守护修复，AR-N01及并行支线最近交付仍受影响，须回实际核心停点 | AR-N01与并行分工 |
| R14 | 独立测试对话接管运行验收 | 11-06；U140,U143 | 已完成 | 独立验收对话已创建，AR-P01/AR-M01多批实测及失败反馈存在；[AR-P01-RESULT.json](../../../tmp/independent-testing-20261106/AR-P01-RESULT.json)；[AR-M01-RESULT.json](../../../tmp/independent-testing-20261106/AR-M01-RESULT.json) | 未来每批继续按快照交验，不算未来所有任务已验 | 独立验收职责 |
| R15 | 同级openrust-dev隔离测试源码/产物 | 11-06；U141,U142 | 已完成 | 实际detached测试worktree和候选overlay/SHA校验存在；[plan.md](../plan.md)；[AR-M01-RESULT.json](../../../tmp/independent-testing-20261106/AR-M01-RESULT.json) | 不能只比较detached HEAD忽略实际候选overlay | 独立验收职责 |
| R16 | 总控与测试直接交接/修复/复验 | 11-06；U145,U186,U187 | 已完成，持续执行 | 真实源码冻结、失败反馈、新快照复验与提交树映射已发生；当前守护亦有真实往返；[AR-M01-RESULT.json](../../../tmp/independent-testing-20261106/AR-M01-RESULT.json)；[V2-TEST-RESULT.json](../../../tmp/guard-objective-revision-20261106/V2-TEST-RESULT.json) | 仍须每簇执行，派发回执不算验收 | 独立验收职责 |
| R17 | 先完整编码配套、有限簇对标后集中验收 | 11-06；U138,U139,U143,U185,U201 | 规则已落实，持续执行 | f63ed257/1a7473ba固定有限簇，Query及拆包已按有限输入收口；[plan.md](../plan.md)；[todo.md](../todo.md) | 不追加无关簇延长冻结；后续发现新缺陷关联整改，不抹已验子范围 | 集中验收规则 |
| R18 | 总控标题改名 | 11-06；U144 | 已完成 | 正式标题OpenRust · 核心开发总控；[plan.md](../plan.md) | — | 对话元数据 |
| R19 | 删除roasted-spider源码及Codex项目记录 | 11-04；U070,U072,U075 | 已完成（本机索引范围） | 正式项目列表无此项、原路径不存在、两份Codex目录/扫描索引计数均0；[deletion-receipt-1793791901383570000.json](../../../tmp/roasted-spider-catalog-repair/deletion-receipt-1793791901383570000.json) | 手机端UI未现场检查，不能声称移动端缓存也已验 | 项目清理 |
| R20 | 清理主/测试构建产物 | 11-06；U146,U147 | 已完成多批，持续执行 | AR-P01约7.85GB、AR-M01约14.24GB真实清理及73文件生成归档退役；[AR-P01-RESULT.json](../../../tmp/independent-testing-20261106/AR-P01-RESULT.json)；[AR-M01-RESULT.json](../../../tmp/independent-testing-20261106/AR-M01-RESULT.json) | 没有本轮全路径新水位，历史回收不能证明此刻总量；活跃/必要缓存不可盲删 | 构建产物与临时资源 |
| R21 | 磁盘和构建产物软硬阈值 | 11-06；U211,U212,U213,U214；11-08 | 以现行规则为准 | 当前规则为构建产物达到 100 GB 或磁盘可用空间低于 5% 时清理本项目全部可重建编译产物，在途构建释放占用后也须补清，复测合格后直接继续。[当前规范](../conventions.md#构建产物与临时资源) |
| R22 | 已验代码及时提交并安全推送 | 11-06；U148,U149,U150 | 已完成多批，持续执行 | 远端main实核501e4a25，与本地HEAD一致，Query/Model/拆包均已有提交；[todo.md](../todo.md)；[testing.md](testing.md) | AR-N01/Passport等未验WIP不能为清空目录而提交；本次文档待交接提交 | Lead唯一索引/推送 |
| R23 | 尽快完成SQLite支持 | 11-02/11-04/11-05/11-06；U017,U026,U028,U036,U046,U121,U132,U201 | 部分完成 | 20cea6c9/f9e831a5/45a10972/c3ba63b4/5c26fd04分范围已验收；[testing.md](testing.md)；[todo.md](../todo.md) | UPDATE/DELETE join/order/limit、Queue/CacheLock、完整Schema及故障/发布矩阵未闭合 | SQLite并行后继 |
| R24 | 数据库最低兼容基线与Laravel一致 | 11-06；U203,U204,U205,U207,U209 | 部分完成；发布矩阵后置 | 目标MariaDB10.3/MySQL5.7/PG10/SQLite3.26/SQLServer2017已记录；SQLite最低版多个限定实测；[database-compatibility-baseline.md](../plans/database-compatibility-baseline.md)；[todo.md](../todo.md) | PG14/MySQL8拒绝阈值、MariaDB/SQLServer及旧SQL差异未闭合；发布前完整矩阵尚未执行 | DB-COMPAT |
| R25 | 日常只用真实MySQL8.4验收 | 11-06；U206,U209,U210 | 已完成 | 政策、runner/CI及最新拆包routine-mysql证据一致；[testing.md](testing.md)；[conventions.md](../conventions.md) | 非MySQL/最低版本后置不等于取消兼容开发或冒称通过 | 数据库运行政策 |
| R26 | 当前Query簇验收收口提交推送 | 11-05/11-06；U121,U126,U201,U210 | 已完成（有限簇） | 960aca43、ed3f608b及beforeQuery/exists各簇已验收提交推送；[testing.md](testing.md)；[todo.md](../todo.md) | 完整Query另保留JSON/FROM/raw/join/写目标等未闭合，不能把有限簇改报全量 | C07后继 |
| R27 | 冷PREPARE取消恢复修复收口 | 11-05；U121 | 已完成（限定边界） | c729fe13真实TLS取消后淘汰重建、回滚/锁释放与生成接线已验；[OPENRUST.md](../../../vendor/launchbadge/sqlx-mysql/OPENRUST.md)；[testing.md](testing.md) | 不保证任意网络阶段取消后复用原物理连接；升级前核上游补丁 | 驱动后继边界 |
| R28 | 统一Hashing验收提交 | 11-06；U126 | 已完成（本批前置） | 0c29bd9a三驱动、认证/验证码/标准UserProvider/生成共用已验；[testing.md](testing.md) | 完整Manager表面与Model hashed cast仍见R29，不扩大本批完成定义 | Hashing前置 |
| R29 | HashManager完整表面及Model剩余能力 | 11-06；U126,U131 | 部分完成 | 06b4d50c属性基础已验，通用Model主线已领取前置；[todo.md](../todo.md)；[db.md](../core/db.md) | getDrivers/forgetDrivers/getContainer/setContainer/getDefaultDriver、save/dirty/关系/日期/完整值载体及hashed cast未全闭合 | Model/Hashing后继 |
| R30 | 清理本地/Docker openrust_*遗留测试库 | 11-05；U122 | 已核当前运行服务 | 本轮只读SQL：当前MySQL8.4、PG18容器前缀库均0，未执行删除；[db-catalog.json](../../../tmp/user-requests-audit-20261106/db-catalog.json) | 不外推全部停止容器/历史宿主服务；未见独立宿主DB进程也不等于扫描所有磁盘目录 | 测试库归属机制 |
| R31 | 异常退出后自动清库，保护业务库 | 11-05；U123 | 部分完成（Unix已交付） | f6002c91正常/取消/失败/SIGKILL持久归属恢复及真实双库清单恢复通过；[testing.md](testing.md)；[test.md](../core/test.md) | 非Unix ACL/真实Windows、只读副本/无应答拓扑仍待验；不合作外部连接/DDL有公开边界 | 网络测试库后继 |
| R32 | vendor补丁源码随仓库直接Cargo运行 | 11-06；U152,U153,U154,U155,U156,U157,U158,U159,U160,U161 | 已完成 | 固定SeaORM/SQLx必要源码+来源/SHA/许可/patch及root/consumer/生成解析图已有验收，不需手工准备脚本；[Cargo.toml](../../../Cargo.toml)；[SOURCES.md](../../../vendor/seaql/sea-orm/SOURCES.md)；[OPENRUST.md](../../../vendor/launchbadge/sqlx-mysql/OPENRUST.md) | 升级/发布重核上游与解析图；不代表已发布crates.io | vendor依赖规则 |
| R33 | vendor按publisher/package组织 | 11-06；U190,U191,U192,U193,U194,U195,U196,U197 | 已完成 | 实际seaql/sea-orm、seaql/sea-orm-migration、launchbadge/sqlx-mysql，无额外仓库层；[Cargo.toml](../../../Cargo.toml)；[extension-package-layout.md](../plans/extension-package-layout.md) | 仓库名方案被后续Laravel/Composer方案替代，不再按旧方案返工 | 包布局 |
| R34 | 核心迁vendor/openrust/framework/src，同仓保留src层 | 11-06；U170,U171,U172,U174,U175,U176,U177,U178,U179,U198,U199,U201 | 已完成 | 44632383独立包和根应用职责/生成/consumer/真实启动已验；[Cargo.toml](../../../vendor/openrust/framework/Cargo.toml)；[testing.md](testing.md) | 早期根src核心或不加src方案已被最终决定替代 | AR-P01 |
| R35 | 根包身份对齐Laravel应用骨架 | 11-06；U216,U217,U218,U219,U220,U221,U222 | 已完成 | 根包openrust、框架openrust-framework，别名openrust::、薄入口与cargo run保留；[Cargo.toml](../../../Cargo.toml)；[testing.md](testing.md) | 旧openrust-app方案已替代，不据旧记录回改 | AR-P01 |
| R36 | Modules作为独立扩展包 | 11-06；U223,U224 | 已完成（拆包范围） | 6c21bab0迁vendor/openrust/modules/src，应用→扩展→框架，无第二内核；[Cargo.toml](../../../vendor/openrust/modules/Cargo.toml)；[testing.md](testing.md) | 全部模块命令/生成发布/发现仍有缺口；不是framework/modules目录 | AR-M01/完整Modules后继 |
| R37 | 命名空间/开发流程最大程度对齐Laravel | 11-06；U225,U226,U227 | 部分完成 | AR-N01出现foundation/container/console/database/routing候选；[todo.md](../todo.md)；[lib.rs](../../../vendor/openrust/framework/src/lib.rs) | 调用方/宏/模板/示例/consumer/文档及适用验收提交尚未完成，HEAD旧路径/兼容不能当新交付 | AR-N01 |
| R38 | 默认模块注册生效、不依赖状态JSON | 11-02；U022,U023 | 默认机制已完成 | edb70a3b默认registered，生成不写JSON，根清单显式注册启用；[mod.rs](../../../modules/mod.rs)；[modules.rs](../../../config/modules.rs)；[testing.md](testing.md) | Rust mod只是编译声明，不等于运行注册；根文件实际删除见R39 | 模块注册 |
| R39 | 根modules_statuses.json及旧模板清除 | 11-02/11-06；U022,U023,U241 | 文件已删除 | 已git rm根文件及旧模板，磁盘/索引均确认不存在，其他索引条目未变；[删除回执](../../../tmp/explicit-omissions-20261106/direct-deletion-receipt.json) | 旧URL runner整改仍是未应用候选；显式file/file-live实现与自有fixture保留，删除提交以Git为准 | 默认宿主清理，Lead唯一索引 |
| R40 | API前缀可配置、默认api、示例api/admin | 11-02/11-06；U024,U181,U182 | 已完成（运行与主指南） | d9267695根/模板及真实TCP/生成六组边界通过；[api.rs](../../../config/api.rs)；[api.rs](../../../tools/cli/templates/config/api.rs)；[routing.md](../../../docs/src/the-basics/routing.md)；[testing.md](testing.md) | 其他文档残留见R41 | RT-A01 |
| R41 | API前缀所有指南/示例同步 | 11-02/11-06；U028,U181,U182,U183 | 文档已校正并静态验证 | 配置默认值已统一为api，实际URI继续为/api；[configuration.md](../../../docs/src/getting-started/configuration.md)；[authentication.md](../../../docs/src/security/authentication.md)；[README.md](../../../tools/cli/README.md) | 本次为文档纠正，不改运行行为；适用链接/字面检查后限定提交 | AR-N01文档与RT-A01残留 |
| R42 | CLI名称、参数、选项及生成行为完整对齐 | 11-02/11-06；U025,U026,U127 | 部分完成 | 680908bd env及多批生成应用有限验收已有；[cli.md](../core/cli.md)；[todo.md](../todo.md)；[testing.md](testing.md) | 仍有generate旧用法，完整make/Artisan命令/交互/选项/生成工作流未完成 | R13/CLI并行 |
| R43 | 固定Laravel13.x章节/阅读顺序及可运行示例 | 11-02/11-06；U027,U028,U183 | 部分完成 | 导航/首批mdBook及各簇示例/生成有验证，不只有规划；[SUMMARY.md](../../../docs/src/SUMMARY.md)；[todo.md](../todo.md) | 完整章节和全部示例未闭合；旧默认/过时状态/恢复码文字仍有遗漏 | 文档随功能闭环 |
| R44 | 默认cargo run邮件启动和脱敏诊断修复 | 11-02；U029,U030 | 已完成 | 85f6e9a9缺省/production/缓存三种真实启动HTTP及正常退出通过，未改development绕过；[testing.md](testing.md)；[mail.md](../core/mail.md) | 完整MailManager/具名mailer和外部投递不在本启动bug范围 | 启动修复 |
| R45 | Docker模拟S3线上环境验收 | 11-02；U034 | 部分完成 | 真实MinIO上传下载/版本/延迟CopyObject/删除竞争已有，不只是mock；[testing.md](testing.md)；[storage.md](../core/storage.md) | 真实ACL启用/禁用bucket、迟到发布不确定性、throw=false等尚未完整闭合，不称真实AWS验收 | Storage/ST-V后继 |
| R46 | 固定Fortify来源、按官方机制实现TOTP | 11-04；U045,U046,U048 | 已完成基线与部分机制 | d8205280固定Fortify1.41，54f4a087同体系组合验收；[laravel-rebuild-baseline.md](../plans/laravel-rebuild-baseline.md)；[testing.md](testing.md) | 完整Fortify公开行为仍不全；Google真实客户端见R52 | Fortify后继 |
| R47 | 官方机制默认、API+Bearer JWT显式可选 | 11-05；U087,U088,U089,U090,U102,U103,U104,U105,U106,U107,U108,U109,U110 | 已完成（本批组合） | 54f4a087默认Web/显式jwt-api、配置/Provider/生成与无Cookie组合已有验收；[fortify.rs](../../../config/fortify.rs)；[authentication_profile.rs](../../../app/providers/authentication_profile.rs)；[testing.md](testing.md) | 早期一律JWT默认/全中心禁Cookie被最终决定替代；完整UUID/字符串JWT持久链不在已验边界 | 认证最终契约 |
| R48 | 删除旧MFA独立体系和旧数据兼容 | 11-05；U078,U080,U082,U083,U084 | 已完成（生产体系） | 54f4a087删除旧feature/类型/表/旧profile和接管兼容，保留有效安全回归；[testing.md](testing.md)；[auth.md](../core/auth.md) | 历史日志与MFA术语保留；旧文档未清归R56，不能据旧文字误认生产双轨 | Fortify单体系 |
| R49 | 其他旧兼容代码全仓清理 | 11-05；U085,U086 | 部分完成 | 其他四类已登记唯一todo，MFA之外未全清；[todo.md](../todo.md)；[lib.rs](../../../vendor/openrust/framework/src/lib.rs)；[validation.rs](../../../vendor/openrust/framework/src/validation.rs)；[queue.rs](../../../vendor/openrust/framework/src/queue.rs)；[main.rs](../../../tools/cli/src/main.rs) | data_exchange别名、Rule length/range、QueueDriver run_next_with_state兼容默认忽略AppState、CLI旧vec迁移回填仍存在 | OLD-API/VALIDATION/QUEUE/GENERATOR |
| R50 | Session派生JWT/PAT和OIDC末检二次验证安全 | 11-05；U078,U080,U111 | 已完成（本批修复） | 54f4a087真实父Session/Redis回滚、Keycloak/OIDC/独立API及双库组合已验；[testing.md](testing.md) | 不承诺跨Redis/DB分布式原子性、所有未来OAuth入口或全部标识链无缺口 | 认证安全后继 |
| R51 | TOTP按需开启及MFA/2FA术语统一 | 11-05；U093,U094,U095,U096,U097,U098,U099 | 已完成（当前策略） | 未绑定可密码登录、待确认不启用、确认后挑战，用户可关闭；同一Fortify三字段；[auth.md](../core/auth.md)；[testing.md](testing.md) | 不把MFA术语扩大成已实现短信/邮件等多种新因子 | Fortify用户选择 |
| R52 | Google Authenticator真实扫码/导入互操作 | 11-04；U046,U048 | 部分完成 | TOTP/otpauth/窗口/恢复流程及实库组合存在；[testing.md](testing.md)；[todo.md](../todo.md) | 真实Google客户端扫码或导入、产码完成流程尚未验；不能自家算法互验替代 | Google真实客户端外部验收 |
| R53 | Passport自建OAuth授权服务器 | 11-05/11-06；U091,U092,U121,U126,U127 | 部分完成 | 固定版本及协议/PKCE/Client/Repository候选有产出；[todo.md](../todo.md)；[oidc-authorization-server.md](../plans/oidc-authorization-server.md) | Client/code/token/schema完整持久签发链未交付；不把RP/Keycloak或普通JWT当AS | L090 Passport并行 |
| R54 | OIDC Provider | 11-05/11-06；U091,U092,U121,U126 | 部分完成 | OP方案及前置已登记；[todo.md](../todo.md)；[oidc-authorization-server.md](../plans/oidc-authorization-server.md) | Provider发现/授权签发/身份声明/JWKS等完整OP尚未交付，不以外部OIDC客户端抵扣 | OIDC OP后继 |
| R55 | 双应用SSO及统一退出 | 11-05/11-06；U091,U092,U121,U126 | 部分完成 | 双RP/退出分期和官方默认/显式JWT边界已记录；[todo.md](../todo.md)；[oidc-authorization-server.md](../plans/oidc-authorization-server.md) | 自建中心→双业务应用→退出完整真实链路未验收交付 | 双RP SSO后继 |
| R56 | 删除旧认证说明、配置/示例/README同步 | 11-02/11-05；U028,U110,U111 | 文档已校正并静态验证 | 已修正旧MFA/恢复码说明及54f4a087验收状态；[当前认证事实](../core/auth.md)；[认证指南](../../../docs/src/security/authentication.md)；[CLI指南](../../../tools/cli/README.md)；[上下文](../context.md) | Google真实客户端、UUID/字符串JWT持久链及完整Fortify继续开放；本次文档提交以Git为准 | 认证文档与AR-N01说明 |
| R57 | 框架错误统一国际化、缺省语言统一 | 11-06；U162,U163,U164 | 已完成（本批） | 1dff3029同一I18n渲染JSON/HTML/envelope、显式locale与默认语言及脱敏稳定code已验；[testing.md](testing.md)；[i18n.md](../core/i18n.md) | 完整Laravel Loader/namespace/plural另有缺口，不把ICU已有资产冒称全量 | I18N-E01 |
| R58 | 数据库翻译来源、简洁统一且可扩展 | 11-06；U165,U166,U167,U168,U169 | 部分完成 | 来源叶/测试代码和统一Loader/快照原则已准备；[todo.md](../todo.md)；[localization-sources.md](../plans/localization-sources.md) | 父Loader、迁移、Provider、配置、唯一示例、生成/文档完整接线及验收未完成 | I18N-S01/translation-database并行 |
| R59 | 核心逐项百分比、扩展单列、更新记录 | 11-06；U235,U236,U237 | 记录与复算已完成 | 188项/72已验/86部分/30待=38.3%，71核心章映射，独审和复算通过；[core-functional-acceptance.md](core-functional-acceptance.md)；[summarize_core_acceptance.py](../scripts/summarize_core_acceptance.py)；[progress.md](../progress.md) | 后续每簇升级有证据的对应行，不以章节覆盖或工时推算完成率；本次提交以Git为准 | 核心验收清单v1 |
| R60 | 优先成熟Rust包集成/必要二开，避免造轮子 | 11-06；U238 | 原则已记录 | 已写复用/选型/许可安全/固定依赖及Laravel机制验收原则并发Lead；[core-functional-acceptance.md](core-functional-acceptance.md) | 后续Spec逐项执行选型审查；本次记录提交以Git为准，不据已有依赖证明后续审查已全部完成 | 成熟Rust包复用原则 |
| R61 | 包名/目录名参考成熟项目并维持统一映射 | 11-06；U239,U240 | 已核对，无待改指令 | 已查Tokio/Axum/SQLx/SeaORM/Rocket/AWS及Composer实际配置，当前目录组织+包前缀方案可行；[extension-package-layout.md](../plans/extension-package-layout.md)；[Cargo.toml](../../../Cargo.toml) | 用户未要求新一轮重命名，不因问答擅改目录 | 包布局规则 |
| R62 | 具名Guard核心Clippy修复、验收并提交 | 11-04/11-05；U076,U077 | 已完成（限定簇） | 5ad34b9f已完成R7具名Guard核心适用门禁、真实双库/最低SQLite/生成及独审；[testing.md](testing.md)；[auth.md](../core/auth.md) | 完整Auth/Fortify后继不由该核心簇关闭；当前守护故障不是这批Clippy未修 | R7具名认证核心 |

## 后续决定已替代的中间口径

| 原口径 | 最终生效要求 |
| --- | --- |
| Architect顶层定时监护 | 已明确删除且不重启，仅总控原线程合作式守护 |
| 默认5分钟定时触发 | 默认30分钟；当前PAUSED问题仍按R08处理 |
| 根默认JWT／认证中心一律禁Cookie | 固定官方机制默认；显式JWT profile全程无Session/Cookie |
| 临时复用旧auth-mfa及旧数据 | 旧独立体系/旧数据不兼容，不做接管/迁移/重绑fallback |
| 根src承载框架／框架包不带src／根名openrust-app | 根应用openrust；独立vendor/openrust/framework/src，框架包openrust-framework，保持openrust:: |
| vendor组织/仓库/子crate多层布局 | 最终Laravel/Composer式publisher/package；不按旧GitHub仓库层级返工 |
| Modules塞入framework/modules | 独立vendor/openrust/modules/src，应用→扩展→框架 |
| 每次日常多后端或受影响最低SQLite运行 | 日常仅真实MySQL8.4；全部承诺后端/最低版本运行发布前验证 |
| 6% 磁盘软线或旧 50 GB / 10% | 11-06 采用 80 GB / 100 GB 与 8% / 5%。11-08 曾采用 40 GB / 50 GB 与 5% / 3%，随后更新为现行 100 GB / 100 GB 与 5% / 5% |
| 本地只提交不push | 已验有限簇按最新持续授权安全推送并核远端；不force/tag/release |
| 全部长期使用100%预算 | 完整普通额度只按具体真实授权生命周期；最新基线/20%及历史以正式回执为准，credits=false |

## 验收证据边界

- 本次正式Goal记录确认同原ID、active及新版正文；“正文已换”与“守护联动可继续”是两件事，前者完成不代表后者完成。
- 当前运行MySQL8.4及PG18容器的openrust_前缀库均0，证据为[只读目录核对](../../../tmp/user-requests-audit-20261106/db-catalog.json)；未执行删除，不推定所有停止容器或历史服务都已清理。
- roasted-spider本机源码与Codex项目索引已清，未现场核手机端缓存。target已有真实回收证据，本次未重遍所有路径计算当前合计；这两项是证据边界，不能虚报已验证。
- 三域原始审查与字段来源见[操作审计](../../../tmp/user-requests-audit-20261106/operations-audit.json)、[数据/布局审计](../../../tmp/user-requests-audit-20261106/data-layout-audit.json)、[认证/服务审计](../../../tmp/user-requests-audit-20261106/auth-services-audit.json)。原始审查中的OP16为较早快照，最新核心清单完成状态以R59为准。
- 发布前多数据库/最低版本后置是用户批准的排期，不算今天漏跑；但相关兼容实现缺口仍必须保留，不能删减范围。
