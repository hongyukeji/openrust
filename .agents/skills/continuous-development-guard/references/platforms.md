# 平台与运行边界

仅在首次接入环境、启动失败、修改脚本或复核验收范围时读取。正常轮次不重复加载。

## 环境复用与启动

复用当前项目、现有 Python 3.10+、Paseo、Codex、已有登录状态及可信主线程。Windows 可使用已有 `python` 或 `py -3`，macOS 通常使用 `python3`；先用已有命令检查版本，不安装软件、全局包、系统服务或修改 PATH、认证、付款设置。缺少工具或原生 Goal 协议时启动明确失败，不创建运行状态。未知工具 shim 不经 shell 猜测执行。

Windows 的 `.cmd` / `.bat` 只通过同目录已有 npm package 的唯一 `bin` 声明及已有 Node 启动，参数不经 cmd 二次展开。非标准 shim 需开发者提供已安装原生程序或可识别入口。Paseo 任务记录只按可信 UUID 查找，核对项目路径、provider、session/thread 全部绑定，拒绝歧义，不猜测路径 slug。

## 生命周期机制

- POSIX：`flock` 单实例与控制锁，worker继承单实例锁。每次工具调用增加一个临时bootstrap和一条生命管道；只有worker持写端，worker被SIGKILL后EOF仍可触发bootstrap回收独立工具组，调用超时/正常退出也清理其顽固后代，不杀worker。bootstrap用系统libc的`waitid(WNOWAIT)`保留未回收组长，清理完成后才wait，避免按复用PID误杀；不扫描进程表猜测所有权。该进程只在工具调用期间存在，不新增常驻服务或模型轮询。
- Windows：`msvcrt` 字节锁不跨进程转交。start 持控制锁，释放临时单实例锁后启动监督进程；监督进程自己获锁、完成 worker 绑定并发布 PID＋创建时间，start 才释放控制锁。停止先落禁用标记，与 start/发送串行。
- Windows 监督进程独占非继承 Job Object handle，启用 `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`。worker 在原始 stdin 单字节屏障前不执行任务逻辑；先绑定 job 再放行，绑定失败直接回收未放行进程。CLI/协议使用相同屏障的标准库 bootstrap，单独 job 保证每次调用退出/超时后回收后代。不用 PID `taskkill` 或全局进程名清理，主开发 Agent 与构建服务不加入这些 job。
- Windows 后台子进程不打开控制台；判活比较内核创建时间，不仅比较 PID。协议用有界 reader queue 和截止时间，避免 Windows pipe 不支持 `select`。JSON/状态/协议显式 UTF-8。
- 单次读取最多512KiB rollout，未变化复用解析。watch.log 超过1MiB时保留末512KiB；每秒监督只检查本地子进程、禁用标记与日志大小，业务检查仍为120秒，不启动模型轮询。保留任务审计和空锁，清理仅本任务进程/PID。
- 确定工具未启动的 FileNotFound 可清除发送意图后冷却重试；退出码、网络错误或返回不明不能证明未入队，同一意图保留并等待新轮次，避免重复开发。同一Goal的plain stop/start保留attempts、退避和未知发送意图，不通过重启绕过保护；只有换新Goal才重置。认证失败、目标改变或协议限制可能需要开发者处理。
- 未完成原目标默认退避恢复，包括未知临时错误和运行器留下的blocked；blocked枚举本身不是硬阻塞。最近显式update_goal blocked按工具契约代表必须外部条件的语义阻塞，兼容直接调用、原始JS和JSON code包装，观察到后按日志文件、字节位置和时间标识持久化到既有state，防止日志截尾或重启丢失，优先于旧错误恢复。同一原Goal确认active且其本地更新时间晚于该控制调用时，持久记录已解除控制版本；尾部日志重复读取不会重新引入旧blocked，后续新控制仍保留。普通用户输入、task_started和plain stop/start不能解除显式阻塞；最新用户输入/取消、暂停/归档、权限、认证、预算未知、任务身份变化继续保护。规则只读最多512KiB日志尾部，不引入模型判断轮询；AI仅实际恢复后判断业务下一步。启动恢复暂停、暂停用户任务及每次中断前，核对本地 goal_id 前后相等及协议公开身份。当前正式协议不暴露 goal_id，使用 threadId、createdAt（秒）和 objective 与本地记录比较；不输出目标正文。协议缺少条件写/CAS，无法完全消除最后复核与写入之间由另一客户端替换目标的竞态，身份不明即拒绝操作。
- 状态中`guard_alive`和兼容字段`active`表示锁、监督进程与心跳确认守护存活，不等于开发推进；`task_running`仅在最近检查确认业务主控running时为真。`reason`说明等待原因，`next_recovery_at`给出最早恢复复核时间，`next_check_at`仍为本地业务控制检查时间。额度自然窗口已知时保存`quota_check_at`至最晚耗尽窗口reset后30秒，未知用量或资格每300秒重查；等待跨worker重启保留，但每120秒继续核对完成、暂停、权限、认证和身份，主动运行主控优先于旧额度期限。额度到期也必须按当前任务政策正式复核资格，不能据到期直接派发。
- 完成事件缺少 completed_at 时，仅使用原始事件中带时区的 ISO timestamp；无可信时间进入待处理，不把每轮当前时间误作完成时间。Windows没有现成通知实现时记录 notification_status=unavailable 和 notification_delivered=false；不安装通知组件，不承诺送达。macOS工具失败则记录 failed。

## 普通问答与审批

正式答复可能写入event_msg.user_message或response_item用户文本；两者均读取，只有完整文本摘要和长度匹配已提交答复才允许解除本普通问卷的blocked。正文只保留末2000字符用于上下文，不作为完整性证明；旧短记录可补算摘要，已达裁剪长度的旧记录不自动推定。带图片/附件的输入与不同人工消息不匹配，原Goal/控制版本、权限、人工暂停和恢复未知保护仍优先。

无人值守优先由AI在提问前处理信息充分的普通开发决策；弹窗适配仅处理原已授权目标内的普通选择题。首次观察原始待答问卷后至少等待60秒，已发现待答期间每60秒复核，其余常规监测仍为120秒，故不是从界面显示时精确倒数。新问卷最多调用一次轻量分类；用户先答、停止/暂停或请求/Goal/线程版本变化会撤销。没有推荐标签也可由该次AI给出推荐；无把握不猜。

2026-10-29按本机正式接口、安装代码及隔离线程实际核实：

- CLI `inspect --json`与`permit ls --json`只输出待答摘要，缺原始问题。`guard_paseo.cjs`通过已安装Paseo的正式`connectToDaemon`/`fetchAgent`只读取得原始`pendingPermissions`，核对agent UUID、provider、cwd、session/thread及activeTurn/最后用户消息；复用现有登录，不读取或改写认证配置。提交也复用同一正式客户端的`respondToPermissionAndWait`，桥接内最后再次fetch并深比较完整pending列表、原请求与绑定；桥接当前只验证macOS应用安装路径，其他环境保留摘要并等待。
- 同步`request_user_input`在原始快照中为`kind: question`、带`metadata.threadId/turnId/itemId/questions`；`requestId=permission-<itemId>`。`respondToPermission`只按`updatedInput.answers[header]`字符串映射回Codex的`answers[id].answers`，缺答案会退回首项。因此适配器强制每题完整且唯一header映射、仅选择现存单项或固定自行决策指引，不使用裸allow或`--all`。CLI的allow会在精确ID消失后继续前缀匹配，因此生产适配直接调用相同官方SDK的精确ID函数，避免另一个审批请求误命中。同/异步问卷最多3题，每题0–5项；无选项普通自由题仅填写固定的授权内自行决策指引；拒绝secret/multiSelect、歧义及超长输入。
- 异步`request_user_input_async`没有独立thread元数据，优先用正式SDK同会话`asyncQuestions`中未resolution的原始`agentMessage`（delivery=async、精确item ID与完整questions）核实所有权。仅旧客户端缺origin字段（None）时兼容正式`thread/items/list`最近20项；当前SDK返回空或异常origin均保守拒绝，不退回旧历史，不全量扫描。Paseo按`Question N`读取答案，随后steer当前turn或发送follow-up；active turn变化会报错，适配器撤销本次发送并重新核验。
- 已安装Paseo同步/异步处理代码的隔离原生测试实际核实答案映射、pending移除及重复响应拒绝。另用独立临时测试线程实际验证CLI答案结构及SDK精确ID提交：异步问答→一次受限推荐→正式答案提交→收到所选标签、pending清空；SDK重复提交明确被拒绝，结束后归档。未向真实开发总控发送测试消息或问题；同步真实弹窗仍以已安装处理器测试和正式schema为证据。
- `guard_questions.py`先做协议/结构筛选，再由一次无工具AI语义判断是否为目标内、可逆且信息充分的普通开发选择。命令审批、秘密/认证/凭据、付款/额度、发布/外部通信、删除/破坏操作、账号/系统设置、必要用户信息、暂停停止或不确定性都拒绝；同一次分类还接收整批已通过结构验证的待答问卷；各请求独立分类，其他题的风险不污染本题结论；其中任一题仍是硬边界/必需未知事实时整批不提交，以免SDK清空尚需用户输入的题。批次指纹在锁内再次核对，批次变化撤销本次发送；稳定请求在同一语义版本下可复用分类缓存，并重新核实当前整批问题与权限边界。题面与选项仅作不可信数据，分类结果严格限制为boolean、confidence、awaiting_question、reason枚举和选项索引；-1只映射固定句“请AI依据已授权目标与现有证据自行决策并继续”，不接收模型生成的自由答案。没有推荐项也可由本次AI推荐；有适合推荐项时优先选推荐项。普通局部测试修复中引用暂停、三次失败、认证或邮件等词，不做机械关键词拒绝；真实用户暂停、敏感审批和必要事实缺失仍拒绝。语言分类不是形式化证明，无法穷举所有表达；无法确认普通类别时等待。
- 分类使用已安装Codex CLI `0.159.3`、任务指定的现有catalog模型（默认`gpt-6-sol`）、low推理。每次在自动清理的临时目录生成该已有模型的受限catalog：direct工具模式、无patch、无实验工具；关闭shell、apps、浏览器、多代理等，忽略用户/仓库配置与技能注入。`model_instructions_file`只放分类规则，输入仅原Goal、问卷与有界阻塞/最近用户上下文，最多16KiB、输出最多4KiB、截止45秒。隔离本机HTTP服务捕获实际请求，验证`input.additional_tools.tools=[]`且无其他工具、无仓库路径/聊天/技能；该测试不访问商业模型。CLI版本升级后默认拒绝，须重新验收后更新版本。
- 跨会话选择“原会话修复并交接”、保留他人WIP并继续独立项/只读检查哈希，是普通协调策略，不能解释为已联系负责人、承诺时间或断言源码稳定。仅必须填写未知具体事实且固定指引无法替代时拒绝。`reason`枚举和`question_wait_reasons`使拒绝/本地模型失败/超时/批次保护可观察；不落盘模型自由理由、命令stderr或秘密。策略版本更新允许旧拒绝一次复判，仍受每ID四版本上限及发送意图保护。
- 已安装SDK的异步响应接受header→字符串，通过一次steer传入原会话，可能清空所有兄弟待答。因此仅整批均独立高把握普通、原始来源可核实、各自等待满60秒时，允许一次精确ID响应携带确定性JSON汇总：逐项列原始题面及已验证选项/固定指引；题面仅为数据。桥接依正式当前完整pending快照重建并严格比对汇总，拒绝任意附言/伪造答案/同步混批。整批意图先写账本，首题成功为`answered`、其余为`delivered_in_bundle`并绑定首题ID；这是答案上下文已由一次正式响应交付，不是每个权限ID均独立成功。结果未知整批均不重发。多同步请求尚无已核实的原子批次协议，记录`unsupported_batch_delivery`并等待。原生已安装处理器回归证明汇总字符串完整进入prompt；fake桥接覆盖篡改/缺项/任意事实/混批，未向真实总控注入测试问题。新增隔离真实批次会话创建被本机provider-quota-guard的普通额度保留线拒绝，未绕过；因此尚无真实批次消费/逐题采纳证据，不能把本地协议回归称作真实自动答题验收。汇总仅保证受限答案上下文构造与SDK提交，不保证模型采纳；原始题面始终不可信，自然语言标注不构成安全隔离。
- 分类和提交前按同一任务政策核对普通窗口或明确获准的已有credits资格；未知/不足等待，不购买或切换账户；额度不足不会触发模型切换。分类意图按稳定请求哈希与Goal/授权/策略语义版本先落盘，每个版本只调用一次、每ID最多4个版本；进程失败或plain stop/start不重跑同一版本；提交意图同样先落盘，结果未知不重发。每个原Goal最多保留64个请求ID的防重记录，超限等待人工，不自动清除未知意图。
- 若Goal blocked或存在模型blocked控制，本次同一分类还须根据Goal、当前阻塞说明及最近用户上下文明确确认“只等待本普通问答”，否则保留真实阻塞；普通协调策略仍可回答但不恢复Goal。题面本身不能证明不存在外部条件。精确答案已确认成功后，持控制锁重新核对原Goal完整版本、正式blocked状态、控制版本与用户/线程绑定及pending清空，才记录恢复意图并用`thread/goal/set`恢复active；用户paused/取消、新block、目标变化、答案或恢复结果不明均不重发，不修改GoalDB或注入继续消息。普通答题与恢复active不等于开发推进；ledger另记录后续新turn/tools证据。
- 稳定请求哈希、Goal身份/目标/状态、用户授权事件、控制与分类策略参与语义版本；activeTurn、自然开发进度/计量更新时间和批次增减不生成新分类版本。已取消/拒绝的未发送问题在真实语义版本变化后可有界重判；同一版本复用缓存，每ID最多4版本，超限等待人工。activeTurn/批次变化只撤销本次发送，重新核验后可继续处理；sending/unknown/answered或已有send_at的任何旧行永不重发。plain stop/start保留全部版本、发送和恢复未知保护，不把后台计时当作新模型轮次。
- 发送与stop共用控制锁；发送前再次读取Goal、线程绑定、当前原始pending和停止标记，只提交一个完整精确ID。协议没有对Goal版本/请求内容的条件写或CAS，外部客户端仍可能在最终复核与提交间改变目标；请求已答通常由Paseo拒绝，未知结果保留意图不重试。不能声称消除了所有跨客户端竞态。

## 验证矩阵与限制

| 范围 | 当前证据 |
| --- | --- |
| macOS / POSIX | 本机隔离假 CLI 与真实进程回归：单实例、并发停止优先、监督重启、完成清理、孤儿后代回收、通过真实launch_tool启动后SIGKILL worker、顽固工具后代清理、超时/正常退出及无关进程存活；不向真实开发线程发测试消息 |
| 跨平台边界 | 真管道超时/UTF-8、跨进程锁互斥及释放、npm bin 安全解析、缺依赖无运行状态写入 |
| Windows 原生 | 已提供专属 job 后代清理、绑定失败屏障、PID 身份测试；当前 macOS 会跳过，尚未在原生 Windows 执行，不能称双平台验收通过 |
| Windows完整Paseo/Codex接入 | 仍需已有 Windows 环境执行原生测试并核实已安装客户端、路径/锁与 start/stop 全链路；不为验收创建环境或修改 CI 跳过验证 |

运行 `python -B -W error::ResourceWarning <skill>/scripts/test_guard.py -v` 和 `python -B -W error::ResourceWarning <skill>/scripts/test_guard_questions.py -v`（按现有 Python 命令调整）。测试使用临时目录和假客户端；Windows 的 start/stop/finish/停止竞态测试通过临时 Python 假CLI入口运行，不需 Node 或修改系统。Windows 专属 Job 测试只在原生 Windows 执行，POSIX进程组/SIGKILL场景在Windows跳过。POSIX清理范围为这些工具的专属进程组；主动setsid脱离该组的daemon和bootstrap自身被SIGKILL不在自动回收保证内，不扫描/终止外部daemon。宿主重启、监督进程被杀、无登录会话或客户端协议变更不能承诺无人工恢复；不改变用户自动启动设置。

依据：[Python subprocess](https://docs.python.org/3/library/subprocess.html)、[Microsoft Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)、[SetInformationJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-setinformationjobobject)。

轻量参考：[frankbria/ralph-claude-code](https://github.com/frankbria/ralph-claude-code) 的退避与会话续接；[oh-my-claudecode 的 Ralph skill](https://github.com/Yeachan-Heo/oh-my-claudecode/tree/main/skills/ralph) 的实际验收要求。
仅借鉴行为原则，沿用已有 Goal；不移植编排、不安装依赖、不另建 PRD 或任务系统，不与原生接续竞争。

## 当前任务的已有 credits 政策

`start --allow-existing-credits` 仅在用户明确允许当前任务消费已有 credits 后使用；默认关闭，`--no-allow-existing-credits` 撤销，同一 Goal 重启保留。修改已运行守护政策须先普通 `stop` 再带选项 `start`；不使用 `--stop-task`，不停止业务主控。`already_running` 仅表示原实例已存在，不更新政策。运行配置的 `allow_existing_credits` 不改变付款、账户或模型设置。普通窗口与 credits 资格分别检查：官方快照须含有效窗口、明确普通用量布尔状态、`credits.hasCredits=true`、有效有限正余额（Decimal）或 `unlimited=true`、`spendControlReached=false`，且限制类型只接受空值或 `rate_limit_reached`。消费控制、未知或其他硬限制不会被余额覆盖。策略变化使旧 `quota_check_at` 失效，下一次需要额度时立即正式复核；退避与发送/问答幂等不变。

问答分类和答案提交使用同一任务额度政策。`--question-model` 仅保存当前任务的分类模型；使用现有 bundled catalog，缺少确切模型即等待。默认仍为 `gpt-6-sol`，`gpt-6.1-sol` 的无工具/无仓库上下文已由真实 CLI 向本地拒绝服务的请求验证，不消费商业额度。此探测不证明商业账户额度可用。

## 动态80%任务预算

正式 `start` 首次为当前Goal取 `account/rateLimits/read` 快照，保存 `usage_budget.version=1`、`baseline_at`、accountId、rateLimits.limitId和每个primary/secondary窗口的reset/时长绑定。剩余百分比是 `100-usedPercent`，不是usedPercent本身。所有窗口分别记录：如启动剩余50%，任务最多用40个百分点，剩余10%即暂停；启动已耗尽窗口暂不参与，仅在有正式新周期证据且首次观测可用后参与。credits按首次真实余额保存，例如5600的保留线为1120。每项独立使用Decimal计算，任一参与项剩余小于等于初值20%就触发 `usage_budget_reached`。没有任何可用参与项也不得开始消费。普通窗口可用不能绕过参与credits的保留线，credits可用也不能绕过参与百分比窗口的保留线。

同Goal的普通stop/start、worker/宿主中断恢复及模型/问答策略调整均保留已存基准，不能以重启侵蚀保留线。同账号credits任务基准仅明确的新Goal才能新建；百分比窗口只按下述正式周期推进。已有守护首次采用预算政策时取正式启用当时真实值并写baseline_at，绝不伪造历史值。旧窗口原先与全局baseline_at来自同一正式快照，升级仅补这个已知周期观测时间，不重算初值。固定余额CLI/配置不再使用。credits余额高于初始值或hasCredits/unlimited状态变化（即使初始credits未参与）、同账号集合变化、缺失/NaN/非法数值都不能借百分比reset放行；credits基准和reserve始终不变。

### 官方窗口周期与自动恢复

已安装Codex CLI `0.159.3` 的正式 `app-server generate-json-schema --experimental` 中，`GetAccountRateLimitsResponse` 的 `RateLimitWindow` 定义 `usedPercent:int32`（必需）、`resetsAt:int64|null`、`windowDurationMins:int64|null`。协议没有额外不可变cycle ID，因此仅在同account/limit/primary或secondary名称及窗口时长不变、旧resetsAt已到、新resetsAt严格更晚且仍在未来时认定新周期。数值上涨、重启或仅到预计时间都不构成证据；null/未知、不完整及提前变化不建立新基准。每窗口持久保存resets_at、previous_resets_at、cycle_observed_at、baseline_remaining和reserve。

初始remaining=0仅暂不参与。官方新周期已确认但仍100%已用时记awaiting_available，以至少300秒间隔等待首次正式available观测，再据当时真实remaining建立该周期80%预算；未恢复前不能用credits绕过原百分比暂停。多个窗口各自推进，一个reset不能解除另一个未reset或新耗尽窗口的保留线。新周期不改credits初始基准，Provider自动扣百分比或credits，守护不控制或承诺消耗优先级。

每120秒控制轮询先于日志读取正式检查运行中主控。任一保留线触发时先存usage_budget_paused意图，再在控制锁内核实原Goal/绑定后暂停、中断。百分比或credits触线、无pending审批/认证/真实blocked，且能取得带时区的正式lastUserMessageAt及稳定控制快照时，才保存percentage_pause所有权；暂停后的完整Goal版本和用户/turn控制水位也必须核实。守护保留原进程做本地120秒人工控制检查，正式额度从300秒指数退避至最多21600秒复核，官方reset较近时提前，不新建后台或模型轮次。自动暂停状态可跨守护重启继续监测，start本身不直接激活Goal。

同账号须全部致停窗口已推进且建立available基准；用户自行切到另一账号则按该账号独立预算核验。所有窗口与credits保留线和官方资格通过、Goal完整版本/绑定/正式用户水位未变、无人工停止/新审批/硬边界时才自动恢复原Goal并精确发送一次原目标继续。激活/发送意图先持久化；结果未知或网络往返期间的人工暂停不重试、不覆盖。用户暂停、目标/控制变化会撤销自动恢复资格；百分比reset不能清除它们。credits触线保持休眠，只能等待用户自行切至可用账号，不将同账号余额上涨当作重置。协议没有CAS，最后核对与动作之间仍有不可消除的竞态，缺可靠来源或水位只保守等待。实际reset未发生前，模拟回归证明的是控制规则，不能冒充账户已真实重置。

检查间隔和在途请求可能让余额跨过保留线；本守护不能封锁其他线程、子代理或账户其他客户端的消费，不承诺精确锁住账户剩余额度，也不为控制额度而终止无关Agent、切换模型/账户、购买或充值。外部消费及不可观察的充值/计量调整不能被解释为本任务精确账单。


### 多账号与长期休眠

- Spec：不操作登录、认证或付款，只观测用户已切换的正式accountId；按(accountId, limitId)保存账号预算。切回账号沿用历史基准，重启不重算；未知账号额度完整且具备正式资格时才建基准。复用原暂停所有权、控制快照及一次性激活/发送链路，不新增派发系统。
- 默认新守护启用账号恢复；只恢复有完整原Goal和用户/控制水位证据的守护预算暂停。旧版只留usage_budget_paused标记的暂停不能自动推定所有权，升级需依据明确恢复授权核实接管一次；不清问答/发送账本。
- 账号恢复暂停未登记百分比触线窗口时，同账号正式额度恢复可用也可继续，不要求换号或新周期；复用原预算、用户控制与Goal所有权校验，激活/派发前再次查正式额度。已登记触线窗口仍等确认重置，credits保留线、未知额度与未知发送继续阻断。
- 所有账号耗尽或未登录时，不循环调用模型或发送继续。额度查询从5分钟退避到最多6小时，官方reset较近时提前；期间每120秒只查本地Goal、停止标记、绑定与auth.json元数据（不读内容）。登录文件变化会提前正式核验，元数据变化本身不授予资格。多账号无需配置清单，也不扫描或保存其他账号凭据。
- Spec：连续额度等待上限3600秒，按持久化quota_wait_started_at计时。账号切换、失败、worker/普通守护重启不重计；正式恢复派发成功或观察到业务运行后清除本轮计时。到期禁用并以quota_wait_expired正常退出，由监督finally回收监督/worker/PID；不操作业务Goal或代码，保留预算和问答/发送防重账本。用户重新要求继续开发，显式start才为超时记录开启新等待周期，不重置账号预算。
- 每120秒检查到期；在途客户端受既有超时限制，监督最多另需一个5秒轮询回收，不承诺精确到秒。只允许期限内自动恢复；超过一小时不再等待3–5天，也不自动重启。沿用既有进程，不安装launchd、Windows任务或软件。监督轮询5秒，无模型token；宿主关机/监督被杀仍不保证自动启动。
- 当前守护回归84项：81通过、3项原生Windows跳过；问答回归47项通过。覆盖同账号可用额度恢复一次、新账号恢复、切回保留线、双窗口切单窗口、手动控制/发送未知、退避/reset与登录唤醒。本机真实原Goal从守护额度暂停恢复active，正式继续提交后已观察到主控running与新task_started；预算和防重账本保留。隔离测试不冒充真实额度reset或Windows验收。
