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
- 未完成原目标默认退避恢复，包括未知临时错误和运行器留下的blocked；blocked枚举本身不是硬阻塞。最近显式update_goal blocked按工具契约代表必须外部条件的语义阻塞，兼容直接调用、原始JS和JSON code包装，观察到后持久化到既有state防止日志截尾或重启丢失，优先于旧错误恢复；最新用户输入/取消、暂停/归档、权限、认证、预算、身份变化继续保护。规则只读最多512KiB日志尾部，不引入模型判断轮询；AI仅实际恢复后判断业务下一步。启动恢复暂停、暂停用户任务及每次中断前，核对本地 goal_id 前后相等及协议公开身份。当前正式协议不暴露 goal_id，使用 threadId、createdAt（秒）和 objective 与本地记录比较；不输出目标正文。协议缺少条件写/CAS，无法完全消除最后复核与写入之间由另一客户端替换目标的竞态，身份不明即拒绝操作。
- 完成事件缺少 completed_at 时，仅使用原始事件中带时区的 ISO timestamp；无可信时间进入待处理，不把每轮当前时间误作完成时间。Windows没有现成通知实现时记录 notification_status=unavailable 和 notification_delivered=false；不安装通知组件，不承诺送达。macOS工具失败则记录 failed。

## 验证矩阵与限制

| 范围 | 当前证据 |
| --- | --- |
| macOS / POSIX | 本机隔离假 CLI 与真实进程回归：单实例、并发停止优先、监督重启、完成清理、孤儿后代回收、通过真实launch_tool启动后SIGKILL worker、顽固工具后代清理、超时/正常退出及无关进程存活；不向真实开发线程发测试消息 |
| 跨平台边界 | 真管道超时/UTF-8、跨进程锁互斥及释放、npm bin 安全解析、缺依赖无运行状态写入 |
| Windows 原生 | 已提供专属 job 后代清理、绑定失败屏障、PID 身份测试；当前 macOS 会跳过，尚未在原生 Windows 执行，不能称双平台验收通过 |
| Windows完整Paseo/Codex接入 | 仍需已有 Windows 环境执行原生测试并核实已安装客户端、路径/锁与 start/stop 全链路；不为验收创建环境或修改 CI 跳过验证 |

运行 `python -B <skill>/scripts/test_guard.py -v`（按现有 Python 命令调整）。测试使用临时目录和假客户端；Windows 的 start/stop/finish/停止竞态测试通过临时 Python 假CLI入口运行，不需 Node 或修改系统。Windows 专属 Job 测试只在原生 Windows 执行，POSIX进程组/SIGKILL场景在Windows跳过。POSIX清理范围为这些工具的专属进程组；主动setsid脱离该组的daemon和bootstrap自身被SIGKILL不在自动回收保证内，不扫描/终止外部daemon。宿主重启、监督进程被杀、无登录会话或客户端协议变更不能承诺无人工恢复；不改变用户自动启动设置。

依据：[Python subprocess](https://docs.python.org/3/library/subprocess.html)、[Microsoft Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)、[SetInformationJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-setinformationjobobject)。

轻量参考：[frankbria/ralph-claude-code](https://github.com/frankbria/ralph-claude-code) 的退避与会话续接；[oh-my-claudecode 的 Ralph skill](https://github.com/Yeachan-Heo/oh-my-claudecode/tree/main/skills/ralph) 的实际验收要求。
仅借鉴行为原则，沿用已有 Goal；不移植编排、不安装依赖、不另建 PRD 或任务系统，不与原生接续竞争。
