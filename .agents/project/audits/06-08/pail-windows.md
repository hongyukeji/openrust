# Pail Windows 原生终端与受管进程

## 基线与范围

2026-11-20，承接 V05；以 `376d10884` 干净归档验收，仅叠加本批 Pail 测试和验收脚本。同期 Passport `ef430b465` 与后续在途工作不纳入该 Windows 快照结论。固定 Pail v1.2.5 `fdb73f5eacf03db576c710d5a00101ba185f2254` 的命令／打印／捕获契约沿用 [Pail 命令](pail-command.md) 和 [默认开发进程](pail-dev.md)。复查 `v0.x` 仍无 Pail；复用当前原生 Reader、关停信号、Windows 作业与已验收的 ConPTY 安全句柄启动方式。

Windows 11 ARM64 Build 26200.9974，Rust 1.94.0、MSVC 14.44.35207。来宾开始为 suspended，临时恢复后在 NTFS 独立目录构建。ARM64 原生与 x64 仿真进程分别记录，不将 macOS／Linux 或 ARM64 的结果冒充另一目标。

## 改动

- `tests/pail_windows_terminal.ps1`：真实 ConPTY 经读写 CONIN$／CONOUT$ 句柄启动应用，分别运行默认颜色、no-ansi、quiet、silent 与正常超时。前四种由伪终端输入 ETX 触发键盘 Ctrl+C；验证 UTF-8／筛选、退出码、会话删除、输出抑制和控制台状态。ConPTY 会拆分／重排 SGR，颜色断言按背景色语义核对。输入模式必须恢复，输出模式除既有 VT processing 能力位外保持不变。
- Pail 命令夹具在 Windows 也执行关闭 stdout 管道的错误／清理路径。自制分段写入、截断和无效 JSON 生产者与真实 File::log 一样先获得独占锁，再定位／写入；Windows 不能对仅 FILE_APPEND_DATA 的句柄加此锁，所以使用 write 权限和持锁 seek，沿用生产实现。
- `tests/pail_auth_http.py` 支持 Windows EXE、独立控制台和真实 Ctrl+C，显式按 UTF-8 读取日志，并将 urllib.parse 导入移到模块，修复不提供可选 MySQL URL 时的局部绑定错误。
- `tests/artisan_pail_dev.py` 复用 Windows 控制台信号工具，新增 Toolhelp 进程快照；Windows 执行 JSON 监督模式，Linux 保留原 Tabs／Stream 路径。实际检查默认 server／queue／logs、HTTP 报告、Pail 无子进程、完整进程树退出与端口释放。
- 三语日志页同步 Windows 二进制命令与受管关停行为；浏览器契约检查三语两挂载路径的 Windows 示例。

没有修改 Pail 生产协议或应用装配，未引入 PHP／tail／外部终端依赖。stdout 同步满管道背压和崩溃无法运行析构的原有边界保留。

## 已执行验收

- ARM64：最低 Rust 锁定离线 `-p openrust-pail --test command --test files --test options` 通过（10 个测试函数及独立命令 harness）；末十行、分段 UTF-8、截断、轮换、程序化取消及 Output 元数据恢复、多会话／双应用、真实 HTTP、过滤、正常／负／零超时、无效 JSON 和关闭 stdout 后删除会话通过。
- ARM64：五种 ConPTY 模式、键盘 Ctrl+C、控制台模式、独立 Artisan inspire 通过；根 HTTP 异常→等待匿名认证→Pail、补视图恢复 200、Ctrl+C 退出 0、端口释放和超时删除会话通过。
- ARM64：官方 `tests/mysql_acceptance.py` 确认 MySQL 8.4.13；只经 Parallels 私有网卡将本批 tmpfs 实例转发给指定来宾。真实 dev JSON 三进程与 HTTP 500→logs、队列启动、Ctrl+C／作业进程树回收、会话删除、端口释放及随后独立 Artisan 通过。不是 Worker 全驱动重验。批次标签 `0d9c25a44379179f005939c97880335a` 容器已复核删除，未改数据库卷。
- x64：在同一 Windows 11 ARM64 的 x64 仿真环境，锁定离线 x86_64-pc-windows-msvc 构建、相同 10 个测试函数及命令 harness、五种 ConPTY、根 HTTP／恢复／Ctrl+C／会话清理全部通过。第二次官方 MySQL 8.4.13 夹具下，相同 dev JSON、无 Pail 子进程、完整关停和独立 Artisan 通过；标签 `be9bf7b872f18e1361a28407e2edf4b2` 容器已复核删除。PE 头分别为 ARM64 `0xaa64` 和 x64 `0x8664`。这是两个二进制目标的实际 Windows 验收，不是原生 x64 硬件矩阵。
- ARM64：`cargo clippy --offline --locked --workspace --all-targets --features pail,passport -- -D warnings` 通过。
- 主机：Rust 1.94 格式、Python AST、差异空白检查、三语站点构建、12 项通用和 6 项内容浏览器检查通过。

独立测试写入器的未加锁／append-only 错误、一次仍占用的测试 EXE 链接失败、ConPTY SGR 字节断言和可选 MySQL URL 未设置时的 urllib 导入失败日志全部保留；修正后以最终源码复验。不为这些夹具问题更改生产文件锁、信号或颜色语义。

## 剩余环境与资源

Windows Docker Desktop／CLI／服务仍不存在；`wsl --status` 返回 50，固件虚拟化与 SLAT 均为 false。V10 需要另一台具备真实 Linux 容器后端的 Windows 目标，本批 Windows 原生验收不能替代它。

证据保存在 `/private/tmp/openrust-v05-20261120/`，最终 9 份候选输入 SHA 与本地独立副本一致，11 份 Windows 源码／基线 SHA 全部匹配；[摘要](pail-windows-sha256.json)记录候选与两个目标。Linux 分支未重新执行，未改其原有行为；全量最终主线与新增 Passport 依赖留 V17 整合。

确认没有活动的本批进程后，直接删除 Windows 两套专属 target，保留源代码、日志、Rustup／Cargo 共享缓存（包括本批安装的 x64 标准库）。来宾逻辑产物 7,494,363,263 字节，C 盘实际释放 6,796,599,296 字节；这不等于宿主释放量。本机浏览器依赖／站点 dist 与重复共享中转已直接清理，root 所有的中转文件在逐份核对证据副本后以限定绝对路径清理；全部数据库卷前后清单相同。Windows 恢复 suspended。宿主 Available 从清理前 65,021,164 KiB 到最终 67,091,532 KiB（68.702 GB／63.983 GiB），实际 df 增量 2,070,368 KiB（2.120 GB），含同期活动影响。

V05 的原生 ARM64 与 x64 仿真进程范围闭合。Windows Docker Desktop 目标、原生 x64 硬件及其余发布范围继续按各自门禁验收；不得将这些 Pail 结果外推为所有 Windows 部署、所有数据库或整个框架完成。本批只核定下列 17 文件本地提交，不推送，不执行发布操作：

- 四个根测试／脚本、Pail 命令测试、三语日志页和现有内容浏览器检查。
- 本审计及 SHA、06-08 README／命令／dev 的接续说明，现有核心账本、current 和 progress。
