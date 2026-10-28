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
- 未完成原目标默认退避恢复，包括未知临时错误和运行器留下的blocked；blocked枚举本身不是硬阻塞。最近显式update_goal blocked按工具契约代表必须外部条件的语义阻塞，兼容直接调用、原始JS和JSON code包装，观察到后按日志文件、字节位置和时间标识持久化到既有state，防止日志截尾或重启丢失，优先于旧错误恢复。同一原Goal确认active且其本地更新时间晚于该控制调用时，持久记录已解除控制版本；尾部日志重复读取不会重新引入旧blocked，后续新控制仍保留。普通用户输入、task_started和plain stop/start不能解除显式阻塞；最新用户输入/取消、暂停/归档、权限、认证、预算、身份变化继续保护。规则只读最多512KiB日志尾部，不引入模型判断轮询；AI仅实际恢复后判断业务下一步。启动恢复暂停、暂停用户任务及每次中断前，核对本地 goal_id 前后相等及协议公开身份。当前正式协议不暴露 goal_id，使用 threadId、createdAt（秒）和 objective 与本地记录比较；不输出目标正文。协议缺少条件写/CAS，无法完全消除最后复核与写入之间由另一客户端替换目标的竞态，身份不明即拒绝操作。
- 状态中`guard_alive`和兼容字段`active`表示锁、监督进程与心跳确认守护存活，不等于开发推进；`task_running`仅在最近检查确认业务主控running时为真。`reason`说明等待原因，`next_recovery_at`给出最早恢复复核时间，`next_check_at`仍为本地业务控制检查时间。额度自然窗口已知时保存`quota_check_at`至最晚耗尽窗口reset后30秒，未知用量或资格每300秒重查；等待跨worker重启保留，但每120秒继续核对完成、暂停、权限、认证和身份，主动运行主控优先于旧额度期限。额度到期也必须重新确认ordinaryUsageAllowed及窗口可用，不能据到期直接派发。
- 完成事件缺少 completed_at 时，仅使用原始事件中带时区的 ISO timestamp；无可信时间进入待处理，不把每轮当前时间误作完成时间。Windows没有现成通知实现时记录 notification_status=unavailable 和 notification_delivered=false；不安装通知组件，不承诺送达。macOS工具失败则记录 failed。

## 普通问答与审批

无人值守优先由AI在提问前处理信息充分的普通开发决策；弹窗适配仅处理原已授权目标内的普通选择题。首次观察原始待答问卷后至少等待120秒，常规监测仍为120秒，故不是从界面显示时精确倒数。新问卷最多调用一次轻量分类；用户先答、停止/暂停或请求/Goal/线程版本变化会撤销。没有推荐标签也可由该次AI给出推荐；无把握不猜。

2026-10-29按本机正式接口、安装代码及隔离线程实际核实：

- CLI `inspect --json`与`permit ls --json`只输出待答摘要，缺原始问题。`guard_paseo.cjs`通过已安装Paseo的正式`connectToDaemon`/`fetchAgent`只读取得原始`pendingPermissions`，核对agent UUID、provider、cwd、session/thread及activeTurn/最后用户消息；复用现有登录，不读取或改写认证配置。提交也复用同一正式客户端的`respondToPermissionAndWait`，桥接内最后再次fetch并深比较完整pending列表、原请求与绑定；桥接当前只验证macOS应用安装路径，其他环境保留摘要并等待。
- 同步`request_user_input`在原始快照中为`kind: question`、带`metadata.threadId/turnId/itemId/questions`；`requestId=permission-<itemId>`。`respondToPermission`只按`updatedInput.answers[header]`字符串映射回Codex的`answers[id].answers`，缺答案会退回首项。因此适配器强制每题完整且唯一header映射、仅选择现存单项或固定自行决策指引，不使用裸allow或`--all`。CLI的allow会在精确ID消失后继续前缀匹配，因此生产适配直接调用相同官方SDK的精确ID函数，避免另一个审批请求误命中。同/异步问卷最多3题，每题0–5项；无选项普通自由题仅填写固定的授权内自行决策指引；拒绝secret/multiSelect、歧义及超长输入。
- 异步`request_user_input_async`没有独立thread元数据，须用正式`thread/items/list`最近20项中的原始`agentMessage`（delivery=async、相同item ID与完整questions）交叉确认。Paseo按`Question N`读取答案，随后steer当前turn或发送follow-up；active turn变化会报错。找不到原始item保守拒绝，不全量扫描历史。
- 已安装Paseo同步/异步处理代码的隔离原生测试实际核实答案映射、pending移除及重复响应拒绝。另用独立临时测试线程实际验证CLI答案结构及SDK精确ID提交：异步问答→一次受限推荐→正式答案提交→收到所选标签、pending清空；SDK重复提交明确被拒绝，结束后归档。未向真实开发总控发送测试消息或问题；同步真实弹窗仍以已安装处理器测试和正式schema为证据。
- `guard_questions.py`先做协议/结构筛选，再由一次无工具AI语义判断是否为目标内、可逆且信息充分的普通开发选择。命令审批、秘密/认证/凭据、付款/额度、发布/外部通信、删除/破坏操作、账号/系统设置、必要用户信息、暂停停止或不确定性都拒绝；同一次分类还接收整批已通过结构验证的待答问卷；其中任一题属于硬边界则整批拒绝。批次指纹在锁内再次核对，只有已精确确认由守护回答的题移出批次可复用剩余题的分类。题面与选项仅作不可信数据，分类结果严格限制为boolean、confidence、awaiting_question和选项索引；-1只映射固定句“请AI依据已授权目标与现有证据自行决策并继续”，不接收模型生成的自由答案。没有推荐项也可由本次AI推荐；有适合推荐项时优先选推荐项。普通局部测试修复中引用暂停、三次失败、认证或邮件等词，不做机械关键词拒绝；真实用户暂停、敏感审批和必要事实缺失仍拒绝。语言分类不是形式化证明，无法穷举所有表达；无法确认普通类别时等待。
- 分类使用已安装Codex CLI `0.159.3`、现有`gpt-6-sol`、low推理。每次在自动清理的临时目录生成该已有模型的受限catalog：direct工具模式、无patch、无实验工具；关闭shell、apps、浏览器、多代理等，忽略用户/仓库配置与技能注入。`model_instructions_file`只放分类规则，输入仅原Goal、问卷与有界阻塞/最近用户上下文，最多16KiB、输出最多4KiB、截止45秒。隔离本机HTTP服务捕获实际请求，验证`input.additional_tools.tools=[]`且无其他工具、无仓库路径/聊天/技能；该测试不访问商业模型。CLI版本升级后默认拒绝，须重新验收后更新版本。
- 分类和提交前核对`ordinaryUsageAllowed`与自然额度窗口；未知/不足等待，不购买、切换账户或模型。分类意图先落盘，即使异常、进程被杀或plain stop/start也不重跑该ID的分类；提交意图同样先落盘，结果未知不重发。每个原Goal最多保留64个请求ID的防重记录，超限等待人工，不自动清除未知意图。
- 若Goal blocked或存在模型blocked控制，本次同一分类还须根据Goal、当前阻塞说明及最近用户上下文明确确认“只等待本普通问答”，否则不处理。题面本身不能证明不存在外部条件。精确答案已确认成功后，持控制锁重新核对原Goal完整版本、正式blocked状态、控制版本与用户/线程绑定及pending清空，才记录恢复意图并用`thread/goal/set`恢复active；用户paused/取消、新block、目标变化、答案或恢复结果不明均不重发，不修改GoalDB或注入继续消息。普通答题与恢复active不等于开发推进；ledger另记录后续新turn/tools证据。
- 原始请求摘要、Goal版本、rollout控制版本和线程/turn绑定都参与指纹；用户先答、混合新审批、取消、暂停、目标/请求变化撤销旧答案。请求ID来自不可变item ID，同一ID换版本视为异常并永久撤销，新的请求ID才重新观察，避免重启或替换题面绕过一次分类/防重。无需把后台计时当作新的模型轮次。plain stop/start沿用既有Goal与问答/恢复意图，不重置一次分类、重答或恢复未知保护。
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
