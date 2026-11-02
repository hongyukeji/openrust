# Codex Desktop 原生守护

Desktop默认通过正式App工具的heartbeat自动化唤醒独立原生监护线程，由它检查并按需接续原业务线程。Python守护保留为Paseo和已有共享app-server的后端；Desktop没有共享socket不再构成启用原生守护的前置条件。不直接连接内部管道、不修改插件或签名、不接管活动writer、不安装系统服务。

## 启用与复用

1. 从可信`CODEX_THREAD_ID`、用户指定的准确线程和正式`read_thread`核实项目、主线程与原Goal。只有明确持续执行授权才启用；已有Goal必须沿用，不为测试创建另一业务Goal。跨对话监护须在本次用户授权的同一任务范围内。
2. 复用同任务获授权、无业务Goal且可保持空闲的顶层监护对话，用可信UUID和正式read_thread核对项目/归属。当前App禁止向multi-agent子智能体直接输入，因此子代理不可作为heartbeat目标，不得绕过该限制。没有可复用顶层对话时，须用户明确允许新建任务后才能用create_thread，不为普通编码再次索要授权。监护对话不创建业务Goal，不承担第二总控职责。业务persistent Goal会自动续轮，同线程heartbeat可一直busy而无法触发，故不绑定到业务线程，也不通过暂停原Goal腾空。若当前会话即选定监护对话，完成设置后应正常返回，让原生调度在下一轮实际接管；不能在本轮while等待自身heartbeat。检索本任务`native-heartbeat.json`及官方自动化列表/回执，避免重复登记。通过正式`get_usage_limits`取得当前额度，将本次工具原始JSON保存为新鲜envelope（source固定codex_app.get_usage_limits、thread为目标、observed_at为本次实际采集时刻、payload为原始JSON），120秒内使用，不能给旧数据换时间；暂存任务文件后运行`guard_heartbeat.py --thread <id> init --usage-file <file>`；helper只读Codex数据库核对身份/原Goal，独立保存默认80%及逐账号预算，不读取认证秘密、不改Goal或全局设置。
3. 用正式`automation_update`创建`kind=heartbeat`、`destination=thread`、准确的监护线程`targetThreadId`和ACTIVE状态，正常五分钟一次；沿用当前线程的模型与上下文，不创建独立cron任务。回执ID用helper的`bind --automation-id <id>`登记（bind的continue仅确认绑定，开发许可必须来自check），后续更新/暂停/删除仅操作该ID。
4. 验收可暂用一分钟周期；创建回执只证明配置。须见监护线程收到该自动化提示、实际执行针对原业务Goal的helper并输出continue，以及后续正式read_thread核对业务状态的真实工具调用；业务本来working只证明自动监护，不能冒称故障恢复；再正式更新为五分钟，保存同一ID/原Goal与预算未重算的证据。

## 每次触发的固定职责

额度envelope的`payload`保留正式工具原始返回值，不手工提取或拼接百分比。helper同时兼容直接额度对象、MCP的单一`content[].text` JSON和`structuredContent`；两种表示同时出现须一致，工具报错、重复JSON键、多份文本或混合/冲突结果均拒绝。未知时查看结构化输出的`budget.error`：`usage_snapshot_invalid`表示来源/绑定/新鲜度问题，`usage_tool_error`表示正式工具报错，`usage_tool_result_invalid`表示包装/内容歧义，`usage_budget_validation_failed`表示额度字段或预算验证失败。只记录固定诊断码，不输出工具错误正文。修复解析后用新鲜正式快照复核，沿用原账本，不用重新init、清账本或重算基线绕过未知。

触发提示须写明它是无业务Goal的监护检查，不是新的真人授权。监护收到后先检查业务Goal，不对自己调用创建Goal或承担业务。先核对原Goal和最后真人控制；遇暂停/停止、目标更换、真正权限审批或必要外部信息不得借本提示恢复。只收取既有在途结果，不重复派发或启动另一个总控。

用正式`get_usage_limits`刷新额度，立即保存同一envelope后执行helper的`check --usage-file <file>`。没有可用正式快照时不继续新业务：记录未知等待而非猜测额度。`continue`且原业务Goal、最近真人控制均再次核实后，先正式read_thread读取业务线程。业务active/等待输入或审批/未知时不发消息、不代答；只写本轮检查回执并正常final，避免监护自身常驻active。只有业务idle、最近turn completed、无真人控制/审批边界，且发送前再次核Goal和turn身份一致，才通过正式send_message_to_thread提示按原唯一todo停点接续。发送意图先落盘，未知不重发；见新的业务工具后才能称恢复。interrupted及未知failed不自动恢复。业务新的耗时阶段前也要再次查预算；普通编码/测试失败自主诊断修复，不降低门禁。`wait`保留账本和原任务，不把超时当授权。`stop`或`finish`必须通过正式自动化工具暂停/删除本ID，并核回执，再在本地记录处理结果；不能仅写一句“已停止”。不得修改别人的自动化、替换Goal、登录换号、购买或兑换额度。

## 资源与验收边界

原生路径复用App调度，不新增监督/worker/PID。实际触发与继续开发仍消耗模型额度；调度登记不等于任务正在运行。App必须可运行，系统睡眠、App关闭、认证/额度硬限可能阻止触发，不能承诺离线恢复或准点执行。额度文件是受信执行Agent从正式工具传入的数据，envelope不能密码学自证来源；真实来源由同一线程的正式工具回执单独取证，helper只校验绑定、新鲜度与数值。helper的额度等待一小时终止规则只在检查实际执行时生效，不能假称模型无法运行期间也有本地Python计时器。

当前原生路径不提供独立的精确turn强制中断，也不代答真实审批；预算检查按实际触发和工作边界执行，不是账户硬上限。用户明确停止时由原业务线程停止工作并正式暂停/删除本自动化；监护线程正常结束，不再调度，也不为自己保留业务Goal。若需要无人值守的本地强制中断/无模型额度监测，须使用已经验证的共享app-server/Paseo后端，不能把原生heartbeat包装成同等控制能力。

官方工具的真实schema与回执为执行依据；不得直接改`~/.codex/automations`或内部数据库来模拟创建、触发、停止。对当前自动化状态、最近触发和真实工具活动分别取证，不以纯文档或隔离测试冒充真实使用。
