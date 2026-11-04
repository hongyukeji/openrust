# Codex Desktop 合作式守护

Desktop默认直接在业务总控对话中运行合作式守护：通过已验证的App自带CLI队列投递定时控制检查，总控在工作边界确认消息、获取新鲜额度并检查原Goal后继续业务。App原生heartbeat只作真正空闲时的补充。不依赖Architect或另一监护对话，不新增业务Goal。Paseo和已有共享app-server仍使用原独立后端；不直接连接内部管道、不修改插件或签名、不接管活动writer、不安装系统服务。

宿主会延后忙碌线程的heartbeat，skill无法配置忙时插入或强制排队。因此本模式不承诺长时间工具阻塞、推理、App关闭或额度硬限期间的准点检查和强制中断。需要独立实时监护时不能把本模式当作替代；用户明确要求忙时定时抢占时，应说明宿主限制，不能用主动checkpoint冒充原生触发。

## 总控队列定时器

原Goal自动续轮可能在毫秒内开始下一轮，而宿主按较长周期扫描空闲线程；缩短原生自动化的周期不能保证捕获这个窗口。队列模式使用`guard_cooperative.py`：只调用已有CLI的`queue --thread <原总控> --message <控制请求>`，不传`--resume`、`--remote`或模型参数。启动探测要求真实help把`--resume`列为可选的排队后恢复动作；旧版CLI若不能证明默认只入队，则拒绝使用，不安装或升级软件。正式入队回执也不证明原writer已消费，实际接收仍须验证。

本路径沿用已有`native-heartbeat.json`及其总控专属自动化绑定；新任务先按下节初始化原Goal预算并正式创建/绑定同框原生自动化。由可信`CODEX_THREAD_ID`等于目标的总控执行以下命令，其他线程不得覆盖环境冒充总控：

```bash
python3 -B .agents/skills/agent-continuous-development-guard/scripts/guard_cooperative.py --thread <总控UUID> start --codex <App自带codex的绝对路径>
python3 -B .agents/skills/agent-continuous-development-guard/scripts/guard_cooperative.py --thread <总控UUID> status
python3 -B .agents/skills/agent-continuous-development-guard/scripts/guard_cooperative.py --thread <总控UUID> stop
```

默认每300秒，验收可显式`--interval 60`。同任务只有一个本地Python worker，不是另一个Agent或业务writer。常规检查不调用模型；只有原Goal仍active、绑定及原预算政策未变，且最近checkpoint允许继续时才投递控制请求。缓存资格最长为周期加30秒，不给旧额度换时间、不把缓存当新的开发许可；过期或未知则等待，连续一小时退出。长阻塞可能延迟处理，也不承诺唤醒真正idle的线程。

发送意图先原子落盘；最多一条待确认心跳，超时、异常或回执不匹配为unknown，不重发。严格核验CLI返回的queue UUID和原thread UUID。总控收到后先执行消息内的`ack --heartbeat-id <id>`，核当前run、原Goal、停止状态及唯一pending；成功只证明接收，再用正式`get_usage_limits`保存新鲜快照并执行checkpoint，continue才推进原todo。真实验收分别保存定时器run/心跳ID、CLI队列回执、同线程ack、新鲜正式额度/checkpoint及后续业务工具，source明确为`codex_cli_queue`，不能写成App原生heartbeat或仅凭队列消失称恢复成功。

status以worker锁及本地last_tick取证，不把状态字符串当存活。stop只停止本任务worker，停止后旧消息ack拒绝。CLI无本模式已验证的删除入口，尚未消费或发送未知时保留`cleanup_pending`，不宣称精确清队列；不写内部数据库。显式重新start只有在旧worker已停止、严格只读查询证实自有queue ID已不在原线程队列后才可解除pending；消失只记不在队列，不记已经执行。未知ID/schema或队列仍存在继续拒绝重启，保护真人和他人的队列项。

## 原生空闲补充的启用与复用

1. 从可信`CODEX_THREAD_ID`、用户指定的准确线程和正式`read_thread`核实项目、主线程与原Goal。只有明确持续执行授权才启用；已有Goal必须沿用，不为测试创建另一业务Goal。跨对话监护须在本次用户授权的同一任务范围内。
2. 检索本任务`native-heartbeat.json`及官方自动化回执，复用原ID和账本。通过正式`get_usage_limits`取得当前额度，将本次工具原始JSON保存为新鲜envelope（source固定codex_app.get_usage_limits、thread为总控、observed_at为本次实际采集时刻、payload为原始JSON），120秒内使用，不能给旧数据换时间。首次才运行`guard_heartbeat.py --thread <id> init --usage-file <file>`；已有账本只check/checkpoint，不重新init或重算预算。helper只读Codex数据库核对身份/原Goal，保存默认80%及逐账号预算，不读取认证秘密、不改Goal或全局设置。
3. 用正式`automation_update`创建或更新同一ID，`kind=heartbeat`、`destination=thread`、`targetThreadId`直接指向业务总控，ACTIVE、正常五分钟一次。沿用其模型与上下文，不创建另一个线程或cron。首次回执ID用helper的`bind --automation-id <id>`登记；bind仅确认绑定，不授予开发资格。迁移旧独立监护方案时先让总控实际接收合作式检查职责，再更新同一automation，不删除原Goal或预算历史。
4. 验收可暂用一分钟周期。分别记录总控主动checkpoint和真实原生heartbeat，不能混算：总控须在本线程执行正式额度工具与checkpoint；真正heartbeat则须见宿主定时输入、总控内正式检查及后续业务工具。自然工作边界正常结束一轮，不暂停Goal、不制造中断、不空转等时间。persistent Goal连续续轮仍可能延后heartbeat，未见真实触发就保留该项待验。验证后恢复同ID五分钟，并核对目标、原Goal及预算基线。

若用户要求删除旧对话的自动化并在总控新建专属自动化，先通过正式工具删除旧ID并保存回执，再正式创建替代ID，核其targetThreadId为原总控且ACTIVE；不能复活用户已删除的监护。已有账本用`guard_heartbeat.py --thread <原总控> rebind --previous-automation-id <已删除旧ID> --automation-id <正式新ID>`显式迁移，不能删账本或重新init。helper只比较旧绑定、检查跨任务排他并记录迁移历史，不自行验证官方删除/创建回执；调用Agent须单独取证并复核Goal、账号历史、baseline与reserve完全保留。普通bind仍拒绝替换ID；rebind及重复执行不授予继续开发资格，仍需新鲜checkpoint。读取本地调度记录时须核对当前运行App实际使用的数据库；开发版/正式版索引可能并存，旧索引不能作为当前调度状态。

## 每次触发的固定职责

### 异常诊断与自动续作

守护的目标是恢复原开发停点。`wait` 不是“只记录后永远不处理”，应结合结构化 `recovery` 字段行动：

| 情况 | 总控的恢复职责 |
| --- | --- |
| 正式工具暂态报错、快照过期、索引暂忙 | 本轮不启动业务，不写永久绑定失败；最多三次有界重新取证，随后回原五分钟检查，不新增轮询进程 |
| 包装解析或预算字段验证失败 | 保留原始回执与账本，核固定诊断、时间精度、账号/窗口身份；修复守护自身缺陷并运行隔离回归，取得新鲜正式额度后重验 |
| 真实额度不足或保留线触发 | 等正式资格恢复，不消费未授权 credits、不重算预算，按已有一小时资源回收政策执行 |
| Goal 已 blocked | 明确通知正式恢复动作和具体原因，不能静默无限 DONT_NOTIFY；工具无恢复接口时等待用户在原 Goal 继续，不写内部数据库 |
| 真人暂停、审批、绑定/目标真正改变 | 保持停止或等待，不能从时间经过、心跳或额度上涨推导授权 |

Goal 仍 active 时，短暂工具故障、可自行修复的脚本错误和普通编码/测试失败不应仅因重试次数被设置为 blocked。可恢复条件消失后，在同一原Goal及预算门禁 continue 后直接继续唯一todo，不再等待Architect消息。符合系统 blocked 判定的真实外部阻塞仍按宿主规则处理，禁止为了持续状态虚报工作。

总控可在本任务授权内修复守护本身的可定位缺陷，无需把已结束的历史 skill owner 当作永久审批；先核当前是否有真实在途 writer，互斥交接，保护业务 WIP，skill-only 运行适用隔离回归、链接/AST/diff检查，必要独审及限定提交。不能因此改全局配置、私有宿主协议或扩大额度。发送 unknown 不重发，已证明未发送的暂态失败与未知发送分开处理。

恢复证明分层记录：正式工具可用、checkpoint continue、后续真实业务工具。只有最后一项发生才可声称开发恢复。若仅剩用户操作，应通知一次具体动作并保持低频检查；有语义变化再通知，不能持续刷屏。

额度envelope的`payload`保留正式工具原始返回值，不手工提取或拼接百分比。helper同时兼容直接额度对象、MCP的单一`content[].text` JSON和`structuredContent`；两种表示同时出现须一致，工具报错、重复JSON键、多份文本或混合/冲突结果均拒绝。未知时查看结构化输出的`budget.error`：`usage_snapshot_invalid`表示来源/绑定/新鲜度问题，`usage_tool_error`表示正式工具报错，`usage_tool_result_invalid`表示包装/内容歧义，`usage_budget_validation_failed`表示额度字段或预算验证失败。只记录固定诊断码，不输出工具错误正文。修复解析后用新鲜正式快照复核，沿用原账本，不用重新init、清账本或重算基线绕过未知。

总控开始/恢复工作、进入新的耗时阶段、收取在途结果后以及提交前，核对原Goal及最新真人控制。实际工作边界距上次正式检查已过五分钟时，先取得新鲜额度，再执行`guard_heartbeat.py --thread <本线程UUID> checkpoint --usage-file <file>`。不为了计时打断在途工具，也不添加持续轮询进程。checkpoint要求可信环境`CODEX_THREAD_ID`等于目标，错线程返回`checkpoint_caller_mismatch`且不更新账本；其门禁与check一致，输出`last_check`记录实际调用线程、模式、时间和结果。只有active原Goal与额度检查continue才继续原唯一todo；暂停/停止、目标变化、真实审批或必要事实缺失优先，不从定时提示推导新授权。

原生触发提示须写明这是已有总控原Goal的定时检查，不是新的真人授权。总控收到后用正式`get_usage_limits`保存新鲜envelope并执行同一checkpoint，不向自己发续作、不读自己状态为idle再恢复。continue后直接按唯一todo收取在途结果并推进已授权业务；业务工具出现才说明实际推进。wait保留账本与停点，不执行新的业务；stop/finish通过正式自动化工具暂停/删除本ID并核回执，仅清理本任务自动化。原Goal状态不由heartbeat恢复；interrupted、未知failed或真人控制来源不明时不得推断恢复许可。每次检查分别记录触发来源、原Goal/automation/本线程、官方额度回执、helper及后续业务工具，不把主动检查写成自动触发。不得修改别人的自动化、替换Goal、登录换号、购买或兑换额度。

## 资源与验收边界

原生路径复用App调度，不新增监督/worker/PID。实际触发与继续开发仍消耗模型额度；调度登记不等于任务正在运行。App必须可运行，系统睡眠、App关闭、认证/额度硬限可能阻止触发，不能承诺离线恢复或准点执行。额度文件是受信执行Agent从正式工具传入的数据，envelope不能密码学自证来源；真实来源由同一线程的正式工具回执单独取证，helper只校验绑定、新鲜度与数值。helper的额度等待一小时终止规则只在检查实际执行时生效，不能假称模型无法运行期间也有本地Python计时器。

当前原生路径不提供独立的精确turn强制中断，也不代答真实审批；预算检查按实际触发和工作边界执行，不是账户硬上限。用户明确停止时由总控停止工作并正式暂停/删除本自动化。若需要无人值守的本地强制中断/无模型额度监测，须使用已经验证的共享app-server/Paseo后端，不能把合作式守护包装成同等控制能力。

官方工具的真实schema与回执为执行依据；不得直接改`~/.codex/automations`或内部数据库来模拟创建、触发、停止。对当前自动化状态、最近触发和真实工具活动分别取证，不以纯文档或隔离测试冒充真实使用。
