# Artisan init（基础流程已验收，完整扩展范围仍在开发）

2026-11-19，按最新用户授权以 `artisan init` 替代旧入口，不保留旧命令或别名。单项安装命令继续保留。计划见 [初始化](../../plan.md#artisan-init)；旧只读命令的证据仅保留为 [历史记录](install-full.md)。本批未暂存、未提交、未推送。

## 实现范围

- 终端向导读取既有应用、数据库、缓存、Session、队列和日志邮件／短信设置，隐藏密码，确认后保存。无交互使用项目 .env，缺失时按 .env.example 创建。`--dry-run` 不连接、写文件、构建或迁移。
- SQLite、MySQL／MariaDB、PostgreSQL 连接预检和 Redis 超时检查；Redis 复用现有 RedisManager，包括 URL、凭据和具名默认／缓存库规则。当前配置向导仅提供基础初始化 profile，非完整生态安装清单。
- 复用单项安装服务生成 API／Sanctum、广播频道和 gRPC 注册；迁移发布日期及 VendorTagPublished 事件沿用原机制。已有文件正常保留，force 仅按已有 API 安装器契约覆盖路由，其他安装器不盲目接受该选项。
- 共用源码锁防并发；统一构建后清除配置缓存，缺密钥时调用 key:generate，已有密钥使用同一个加密解析器校验而不重新生成；随后调用原 migrate。生产环境必须显式确认或 force；子进程失败返回非零，保留脚手架供重试，不重置数据库或历史迁移。
- Windows 使用独立 target/artisan-rebuild，避免替换正在运行的 EXE，也避免临时目录在新子进程启动前被提前回收。此路径尚未实机验收。
- 修复向导回车将未配置的 Redis 用户名／密码写为空字符串、继而错误发送认证的问题。未设置与空字符串保持区分，预检走同一个 Redis 实现。
- 中文 APP_NAME 的 HTTP 验收发现 Session Store 人为增加 ASCII 限制。固定 Framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 Illuminate/Session/Store.php setName 仅保存名称；本批去掉此额外校验，Cookie 名称编码／校验仍交原 Cookie 层。未改变配置中的 snake 命名或数据格式。增加 Unicode、空字符串及其他字符串名称回归，真实中文应用名 HTTP 通过。

## 第一批真实验收（历史）

macOS ARM64，Rust 1.94.0；每日数据库按规范使用 tests/mysql_acceptance.py 的独立 MySQL 8.4.13 tmpfs 容器，Redis 使用独立 redis:8 镜像 ID、随机端口及 /data tmpfs；所有测试 schema 和凭据均为临时批次所有，不使用现有 mysql／postgres／redis 数据。

- `cargo +1.94.0 test --locked --test init --test startup`：6 项新测试＋28 项启动／安装回归通过。覆盖只读、帮助、无交互连库失败不写配置、生产不授权拒绝、无效已有密钥不轮换、取消旧入口，以及共享锁冲突和释放后重试。
- `python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --locked -p openrust-framework --test session_http`：17 项通过。
- `python3 -B tests/mysql_acceptance.py -- python3 -B tests/init_acceptance.py`：最终日志 `/private/tmp/openrust-init-e2e-unicode.log`。独立干净源码快照的 MySQL＋Redis 初始化、真实 PTY 向导及中文应用名变更、保留已有凭据／密钥、重复执行不新增迁移、非法表前缀导致迁移失败返回非零且保留脚手架、修正后运行、真实 Redis queue:work 空队列退出通过。随后直接 cargo run；/up 与首页 200，自定义 /api/admin/user 返回 JSON 401，SIGINT 后进程退出。
- 7 份本批实现文件 SHA 与实际验收源码一致，清单位于 `/private/tmp/openrust-init-acceptance-final/candidate-sha256.json`。
- 本批包范围严格 Clippy：`cargo +1.94.0 clippy --locked -p openrust-framework -p openrust --all-targets -- -D warnings` 通过。完整 workspace 尝试被并行 Passport client_lookup.rs 的两处 insert(Vec) 类型错误阻断，记录 `/private/tmp/openrust-init-clippy-latest.log`；未修改协作者测试，不能声称整工作区通过。
- `cargo +1.94.0 fmt --all --check` 和差异空白检查通过。三语 VitePress 构建、12 项根路径／docs 子目录、桌面／手机浏览器检查通过；命令文档迁至 init，不保留旧公开命令。
- 第一轮临时目录重名、Redis 空凭据和中文名称导致首页 500 的失败日志均保留；修复后的结果与早期失败分开，不用旧快照冒充最终验收。

日志位于 `/private/tmp/openrust-init-*.log`，最终终端／重试／Worker／失败恢复／HTTP 详细记录在 `/private/tmp/openrust-init-acceptance-final/`。原始脚本验证和资源回收另见下方补充。

## 第一批结束时的待办（后续进展见下方）

- Passport／OAuth2、Reverb 服务端及其他第一代选定扩展的完整安装清单和选择引导，继续等对应共享安装器就绪后接入同一个 init；不能新增第二个完整安装入口，也不能将 profile 成功描述为所有扩展已安装。
- 当前向导只接受日志邮件／短信配置，外部供应商配置／凭据引导及相应验证尚未接入。已有其他驱动会明确拒绝并保留配置，不偷偷改成 log。
- macOS 下 MySQL／Redis已实测；SQLite、MariaDB、PostgreSQL 初始化端到端、Windows CMD／PowerShell、Linux、生成项目模板尚待各自验收；已有其他功能的证据不等于 init 已实测。
- 初始化仅支持项目 .env，显式 --env 当前明确拒绝；更复杂的自定义连接／安装 profile 需继续核定。配置合并保留已有值；跨版本完整升级清单和任意自定义结构的集成仍需后续补齐。
- 此记录不更新原核心66单元为新的完整1:1结论，不解锁v0.1.0发布；仍交用户最终验收，未授权不推送、打tag、发布或部署生产。

## 脚本与清理

原始 `RUSTUP_TOOLCHAIN=1.94.0 ./artisan init --dry-run` 通过，未改当前项目配置。确认无占用后直接删除本批三个独立构建目录、浏览器依赖、站点 dist 和快照编译视图，按 du 回收 9.379 GB；本项目卷 df 实际增加 8.631 GB，清理后可用 76.734 GB（71.464 GiB）。保留全部源码快照、历史失败和成功验收记录；临时 MySQL／Redis 容器已按夹具回收，现有 mysql／postgres／redis 容器未删除，无卷删除或共享缓存清理操作。Docker 运行时为本次验收启动，保持运行以免影响现有服务。未暂存、未提交、未推送。


## 第二批：Reverb、SMTP、短信配置及环境选择

2026-11-19 继续实现。以 `artisan init` 为唯一统一入口，不暂存、提交或推送。

### 新增实现

- 安装 Reverb Cargo feature，监听器仍由 `REVERB_ENABLED` 控制。启用时补齐缺失的本机监听设置及随机应用 ID／Key／Secret，重复执行保留已有凭据；向导可配置监听端口、广播连接和服务端地址。
- SMTP 引导复用原 `SmtpTransport` 的连接检查，沿用 URL、认证、TLS 配置解析；本地协议检查不发送邮件。不将 SMTP 连通等同于真实供应商投递。
- 引导 log、阿里云、腾讯云、Twilio、Vonage、短信宝及云片所需的凭据；默认 log。外部短信驱动缺凭据时在修改配置前失败；不发送测试短信，不声称已验证签名、模板或手机收信。
- `--env=acceptance` 读取和更新 `.env.acceptance`，将环境选择传给重建后执行的 `config:clear`、`key:generate` 和 `migrate`。不修改默认 `.env`；只读计划展示实际选定文件。生产迁移继续使用原确认／`--force` 机制。
- 测试夹具以原子序号补充时间戳，修正并行／低精度计时下的临时目录冲突；PTY 验收过滤终端控制序列，并即时保留失败进度。

### 实际验证

- macOS ARM64，Rust 1.94.0：`cargo test --locked --test init --test startup` 共 **8＋28** 项通过。独立 MySQL 8.4.13、Redis 和本地 SMTP 端点的初始化 E2E 通过，Reverb 返回 WebSocket **101** 和 `pusher:connection_established`；SMTP 连接检查成功且未发出 MAIL／RCPT／DATA。日志 `/private/tmp/openrust-init2-e2e-api.log`。该 macOS E2E 发生在追加指定环境文件的端到端断言之前，环境选择的完整路径由以下 Ubuntu 批次覆盖。
- 原生 Ubuntu **x86_64**，MySQL **8.4.13**、Redis：真实 PTY 初始化和中文 APP_NAME、重复 `--force` 不更换应用／Reverb 密钥或重复迁移、故障迁移非零退出并保留脚手架、恢复后启动、Redis `queue:work --stop-when-empty`、Reverb 握手和本地 SMTP 预检全部通过。另实测 `.env.acceptance` 生产环境 `--force`，只为选定环境生成新 Key，默认 `.env` 字节未变。
- 默认 `/api/user` 和自定义 `/api/admin/user` 均实测匿名 **401**、Bearer Token **200**；Token 是测试库写入的 SHA-256 令牌夹具，证明安装后的认证接线，不冒充本批验证了令牌签发接口。
- Ubuntu 服务器未装宿主 Rust：本批专用 Cargo 包装器在原生 Linux Rust 1.94.0 容器中编译，直接在 Ubuntu 宿主执行生成的 ELF；测试中的 `cargo run` 使用同一包装器构建后执行。固定 Git 依赖网络获取失败后，从本机复制同一提交的缓存到本批独立目录，离线构建，未改依赖版本或清理共享缓存。最初构建目标目录传递失败属于验收包装器问题，修正后完整重跑通过；旧失败日志保留。
- 本批包范围 `cargo +1.94.0 clippy --locked -p openrust-framework -p openrust --all-targets -- -D warnings` 通过，日志 `/private/tmp/openrust-init2-clippy-resolved.log`。修正了环境参数迭代的 Clippy 提示，使用 `next_back()`，不改变参数选择行为。完整 workspace 的协作者 Passport 测试阻断见第一批记录，未擅自修改。
- 最新三语 VitePress 构建和 `/`、`/docs/` 桌面／手机共 **12** 项浏览器检查通过。日志 `/private/tmp/openrust-init2-docs-final.log`、`/private/tmp/openrust-init2-browser-final.log`。

Ubuntu 原始证据保留于服务器 `/tmp/openrust-init-native-20261119/`，本机副本 `/private/tmp/openrust-init2-native-evidence/`。macOS 各步骤记录保留在 `/private/tmp/openrust-init2-acceptance/`。

### 当前待办

- Passport／OAuth2 统一安装仍依赖其共享安装器；当前 Passport 只有密钥命令等部分能力，不能将 API／Sanctum 验收当成 Passport 验收。完整扩展选择清单和引导尚未全部闭合。
- 真正 SMTP 服务商的 TLS／投递和短信供应商、模板及手机收信待测试账号；本批不消耗外部供应商额度。
- Windows 原生构建、重建迁移及 HTTP 已通过，真实终端交互向导、补充 Windows Clippy 的范围及清理结果见下方。SQLite、MariaDB、PostgreSQL 初始化 E2E 及生成项目模板仍未纳入本批，留对应发布验收。
- 维持用户最终验收后发布的边界，不调整核心完成统计，不推送或打发布标签。


### Windows 原生路径修复与回归

Windows 11 ARM64（Parallels 虚拟机）、Rust 1.94.0 原生 MSVC 目标。开始时虚拟机处于挂起状态；本批暂时启动验收，收尾后恢复挂起。未使用 Windows Docker Desktop，也不把本机数据库容器算成 Windows Docker 验收。

- 构建最初缺少 `clang`；核实既有 Visual Studio BuildTools 的 `VC/Tools/Llvm/ARM64/bin` 后只为本批子进程补 PATH，没有安装额外工具链或修改全局设置。原失败日志保留。
- 实际启动回归发现欢迎页和公共文件目录共 3 项 HTTP 500。诊断确认 `ApplicationBuilder::create` 的规范化目录在 Windows 带 `\\?\` 前缀；`Paths` 的手动拼接在该前缀后保留 `framework/views` 等正斜线后缀，Win32 不再替换分隔符，返回错误 123。只在 Windows 分支将后缀换为系统分隔符；不修改根路径、公开方法名、配置键或遍历片段的原有语义。Unix 编译分支不变。这是 Windows 运行时路径约束的适配。
- 增加规范化路径下实际创建、写入和读取文件的 Windows 回归。既有存储配置测试中的路径按 dotenv 语法加单引号，避免将 Windows 反斜线当作转义；不放宽应用的 dotenv 解析规则。
- `cargo test --offline --locked --test init --test startup --test storage_paths` 最终 **8＋28＋5** 项通过。原 3 项失败均恢复；PowerShell 的 `artisan.bat init --dry-run` 和 CMD 的 `artisan.bat help init` 通过，原始日志 `/private/tmp/openrust-init2-windows-final-fixed.log`。
- 独立 MySQL 的完整重建／迁移实测及清理结果继续在下方记录，不能用只读入口替代迁移验收。


### Windows 完整重建、迁移与 HTTP

- `python3 -B tests/mysql_acceptance.py -- python3 -B /private/tmp/openrust-init2-windows-e2e.py`：在本批 Windows 干净源码骨架中用 PowerShell 执行 `artisan.bat init --no-interaction`，实际独立目录重建、生成缺失 APP_KEY、执行 MySQL 8.4.13 迁移成功。再用 CMD 执行 `artisan.bat init --force --no-interaction`，断言密钥不变、迁移文件数量不变，退出码 0。日志 `/private/tmp/openrust-init2-windows-e2e.log` 和 `/private/tmp/openrust-init2-windows-evidence/initialization.log`。
- Windows 无 Docker 后端。本批由 macOS 既有 Docker 启动独立 MySQL 8.4 tmpfs 夹具，只经 Parallels 私有网卡 `10.211.55.2` 的本批 TCP 转发供指定虚拟机访问；未绑定公网接口、未使用现有数据库、没有新增数据库卷。转发和容器随脚本收尾。
- 首次远程启动使用过长编码参数而未进入 guest，改为共享脚本文件启动。首轮完整重建后的失败则是验收夹具把 `.env.example` 与覆盖项直接拼接，产生两个 APP_KEY；原 key:generate 正确拒绝重复定义。修正夹具去重，恢复本批干净源码骨架后重跑通过，没有放宽应用密钥命令或修改主仓库配置。
- `python3 -B tests/mysql_acceptance.py -- python3 -B /private/tmp/openrust-init2-windows-http.py`：在新独立数据库运行已生成应用的原迁移，写入同类测试令牌夹具后直接启动 Windows 原生 EXE。默认 `/api/user` 和自定义 `/api/admin/user` 均得到匿名 401、Bearer 200；`/up`、首页 200，中文 APP_NAME 正确显示。PowerShell `artisan.bat list --raw` 和 CMD `artisan.bat help list` 通过。日志 `/private/tmp/openrust-init2-windows-http.log`、`/private/tmp/openrust-init2-windows-evidence/http.log`。首轮 HTTP 脚本变量与 PowerShell 内建变量冲突的失败记录保留，改用专属变量后重跑全部 HTTP 断言通过。
- HTTP 验收通过后按专属进程 PID 结束服务；此次结束使用 Windows 进程树终止，不将其描述成原生 Ctrl+C 优雅关闭验收。ConPTY／真实终端下的交互向导和隐藏密码尚未纳入本批 Windows 路径；macOS／Ubuntu 的真实 PTY 已通过。

### 本批收尾（继续记录）

- 本机 Mac 构建缓存、浏览器临时依赖及站点 dist 已直接删除，实测清理时可用空间增加 **5,411,672,064 字节（5.412 GB）**，当时可用 **75,847,970,816 字节**。记录 `/private/tmp/openrust-init2-local-cleanup.json`。同时有其他开发／虚拟机写入，最终空间另以收尾时读数为准，不把目录 du 与全盘 df 的差值混为一谈。
- Ubuntu 本批无活动构建进程后，删除本批 target、registry、Git 依赖缓存；核对镜像 ID 和使用者后删除本批新增且无人使用的 redis:8 镜像。实际可用增加 **5,129,039,872 字节（5.129 GB）**，剩余 **108,318,351,360 字节（108.318 GB）**。原有 Rust／MySQL 镜像和其他任务资源未清理。记录 `/private/tmp/openrust-init2-native-evidence/cleanup.json`，服务器源码及原始记录保留。
- Windows 生产源码检查和最终产物清理结果待以下补记；未暂存、未提交、未推送。


### Windows 清理与检查边界

- Windows 的补充 `cargo clippy --offline --locked -p openrust-framework -p openrust --lib --bins -- -D warnings` 尚未完成。macOS 磁盘临近 50 GB 停线阈值时主动取消本批独占的编译器，命令以 101 退出；日志 `/private/tmp/openrust-init2-windows-clippy.log`。这不是 Clippy 已通过，也不是已定位代码 lint 错误。前述原生编译、41 项测试和真实迁移／HTTP 成功结论独立保留；资源充足后补该检查。未继续启动新构建。
- 确认本批无活动应用／编译器后，直接删除 Windows `C:/Windows/Temp/openrust-init2-20261119/target` 和 `source/target`，以及本批 Blade 编译缓存，未进回收站。源代码和记录保留，用户 Cargo／Rustup 共享缓存未清理。文件逻辑总长 **8,174,822,074 字节**；guest C 盘实际可用从 **179,952,209,920** 增至 **187,439,161,344 字节**，增加 **7,486,951,424 字节（7.487 GB）**。这不等于 Mac 物理磁盘释放量；虚拟磁盘回收和并行写入需以宿主 df 为准。日志 `/private/tmp/openrust-init2-windows-cleanup.log`，guest 保留 `cleanup.json`。
- 本批 TCP 转发及 MySQL tmpfs 容器均已退出；Docker 仍只有原来的 mysql、postgres、redis 三个服务，数据卷列表前后相同。本批没创建持久数据卷，没有删除任何卷。macOS 的临时共享启动脚本和重复诊断副本已直接删除，原始验收脚本及失败／成功记录仍在 `/private/tmp/` 保留。
- 最终格式检查和差异空白检查通过。本批未暂存、未提交、未推送；Passport 在途改动及其他协作者代码保持原样。Windows 虚拟机按开始前状态恢复挂起，最终状态及宿主空间以收尾读数补记。

最终复查 Windows 11 为 `suspended`。宿主项目卷可用 **53,688,471,552 字节（53.688 GB／50.001 GiB）**；临近 50 GB 停线，后续先复查空间，不启动新的构建批次。本次索引保持为空。


## 第三批：扩展选择与原生验收收尾（2026-11-20）

继续原对话未完成的初始化任务；保留原有 WIP，不暂存、提交或推送。工作开始时项目卷可用 72,689,778,688 字节，超过 50 GB 停线。证据统一保存在 `/private/tmp/openrust-init3-20261119/`，日期目录沿用本次启动时间，不代表全部检查发生于 11 月 19 日。

### 实现及范围

- 增加 `--extensions=api,broadcasting,reverb,grpc`，交互向导提供同一选择；缺省集合维持原行为。API 复用 Sanctum 安装器，Reverb 自动纳入广播频道依赖。选择仅控制本次补装，不删除未选择的已有路由、特性或配置；重复运行不新增独立持久状态账本。
- 共享路由安装服务按所选扩展执行；计划仅列出本次涉及的路由和资源，`--force` 也只作用于已选择安装器原有的可覆盖资源。已配置 Reverb 但没有选择该扩展时，在修改配置前明确报错。
- `--extensions=all` 检查第一代完整发布范围。Passport、Fortify、Passkeys、Socialite、Precognition、OIDC RP、SSO 的实现或共享安装器尚未完成时，列出对应前置单元并非零退出；不写配置、不加锁、不发布资源、不构建或迁移。选择单个未就绪项、未知项、空项或混用 all 同样在写入前拒绝。
- 初始化是 OpenRust 自有编排扩展，本批不改变单项 Laravel 安装器契约。旧版 v0.x 的框架目录中未找到同职责的 init／install 编排，继续复用当前已验收的安装服务，不复制旧版不适配接口。
- 补全工作区集成检查：邮件、分页测试的数据显式转换为已有 `ViewValue`；视图 Gate 测试按实际原生参数类型下转型；原生值 HTTP 测试向应用工厂传入拥有型路径。仅修改既有测试的类型适配，保留断言及其他 ViewData 在途实现。

### 已执行检查

- 独立源码快照，最低 Rust 1.94.0：`cargo test --offline --locked --test init --test startup --test storage_paths`，macOS ARM64 **11＋28＋4** 项通过。覆盖选择性计划、Reverb 自动广播依赖、全部未就绪项与错误选择的无写入拒绝、已有 Reverb 配置冲突，及已有启动／源码安装回归。日志 `tests.log`。
- `OPENRUST_INIT_TEST_INTERACTIVE=1 ... python3 -B tests/mysql_acceptance.py -- python3 -B tests/init_acceptance.py`：MySQL **8.4.13**，先仅选择 API，断言没有频道和 gRPC 文件；默认再次初始化补齐两者，保留密钥和迁移文件。真实 PTY 选择扩展与中文应用名、生产指定环境文件、迁移失败保留及修复、直接 cargo run、首页／健康检查、默认及自定义 API 前缀的匿名 401 和令牌 200 均通过。日志 `e2e.log` 及 `e2e/`。本批未重复 SMTP／Reverb WebSocket／Redis 集成检查，沿用第二批对应证据，不冒称全部再次运行。
- 整工作区全目标 `cargo +1.94.0 clippy --offline --locked --workspace --all-targets --features pail,passport -- -D warnings` 通过，日志 `clippy-final2.log`。首轮快照漏复制文档，导致 Passport 的 include_str 失败；补齐文档后暴露上述既有 ViewData 测试类型不匹配，修复后通过。所有中间失败日志保留，没有把夹具失败记成产品回归或修改 Passport 功能。
- `cargo test ... -p openrust-framework --test view_finder --test view_native_values`：**5＋5** 项通过，包括真实 HTTP／双应用隔离、Gate 原生参数、非有限值、JSON 错误恢复及固定 PHP 夹具。日志 `view-regressions.log`。
- 官方 MySQL 8.4 tmpfs 入口运行 `pagination_on_mysql_and_http -- --exact`：**1** 项真实 MySQL／HTTP 回归通过，显式设置要求执行的环境变量，没有执行 SQLite 用例；日志 `pagination-mysql.log`。邮件类型适配通过全目标编译检查，本批未重新执行投递和数据库队列测试。
- 三语 VitePress 隔离构建通过；现有 Chromium 1243 上 `/` 和 `/docs/`、三语、桌面／手机 **12** 项浏览器检查通过。日志 `docs-build.log`、`docs-browser.log`；专属 npm 目录只安装 Playwright，不下载或清除共享浏览器缓存。
- Windows 原生最低 Rust 1.94.0 的 `cargo clippy --offline --locked -p openrust-framework -p openrust --lib --bins -- -D warnings` 已通过，日志 `windows-check.log` 中的 dev 完成记录。源码以第二批 Windows 快照加本批初始化变更验收；不是当前所有其他在途模块的 Windows 验收。

Windows 最终回归、ConPTY 结果与本批资源清理继续补记；完整发布范围仍未就绪，不能将所选扩展成功或这些前置检查等同于 Passport／OAuth2 与所有认证扩展已交付。


### Windows 最终结果与证据

- 本批包范围严格 Clippy 在恢复默认骨架后再次通过，最终日志 `windows-clippy-final.log`。原生 `init`／`startup`／`storage_paths` 回归最终 **11＋28＋5＝44** 项通过，日志 `windows-final-tests.log`。首轮复用了第二批已初始化的源码，startup 的默认骨架断言失败；恢复默认 bootstrap／routes 后，保留的旧文件时间戳又导致 Cargo 复用旧 include_str 产物。修正本批快照的只读属性和修改时间并重编后通过；没有修改产品来绕过断言。
- 新增可重跑的 `tests/init_windows_terminal.ps1`。ConPTY 真实 Windows 终端实测配置引导、输入非空随机密码且不回显、连接失败后明确取消、环境文件和 API 路由保留，最终通过；日志 `windows-terminal-final.log` 及 `windows-evidence/terminal.log`。这是实际交互和安全输入／取消验收；成功重建迁移的 Windows 证据沿用第二批，未冒称本批重新执行了交互迁移或 Ctrl+C 关停。
- 最初的 ConPTY 夹具继承远程启动器的管道句柄，随后使用写模式 CONOUT$ 又导致终端库判断为 unattended。用本批小型 Rust 启动器以读写方式打开 CONIN$／CONOUT$ 并传递给被测进程后通过。夹具同时过滤窗口标题／光标控制序列；超时按唯一进程树回收。所有失败、探针和最终日志均保留，不修改全局终端设置或安装工具链。
- Windows 的 3 份当前初始化源码／测试 SHA 与本机候选一致，记录 `windows-source-verified.txt`；本机本批 12 份实现、测试与正文 SHA 保存在 `candidate-sha256.json`，7 份编译 Rust 输入和三语已构建正文逐一与工作区核对一致。

### 本批清理与后续边界

- 确认本批无活动构建／应用后，直接删除专属本机 target、Playwright npm 目录和隔离站点 dist。逻辑文件总长 **5,504,649,369 字节**，宿主 df 实际增加 **5,280,145,408 字节（5.280 GB）**，记录 `local-cleanup.json`。未删除 root target、Cargo／Rustup／浏览器共享缓存或来源快照。
- Windows 专属 target 直接删除；guest C 盘实际增加 **3,841,216,512 字节（3.841 GB）**，逻辑总长 **4,181,057,149 字节**。这不等于宿主物理磁盘释放量。证据和启动脚本已保存到本机专属目录，guest 源码保留；本批共享中转目录的 root 所有文件在复核副本后用限定绝对路径直接清理，未进废纸篓。
- Windows 恢复开始前的 **suspended** 状态；挂起保存会写入宿主磁盘，最终宿主可用 **69,609,062,400 字节（69.609 GB／64.828 GiB）**。记录 `final-resources.json`，不能用清理瞬间读数替代最终读数。
- MySQL tmpfs 容器已由官方夹具回收；现有 mysql、postgres、redis 服务保留。本批未创建持久数据卷、删除数据卷或触及已有数据库。
- 格式、差异空白及 Python 语法检查通过。没有暂存、提交或推送；工作开始时的其他 WIP 保留。完整工作区检查已恢复通过，但不代表其他在途模块完成交付。
- **尚未完成的原目标：** Passport／OAuth2 及第一代其他认证扩展本身与共享安装器仍按既有依赖单元开发；当前完整范围检查会明确拒绝。不能将此批的扩展选择与平台检查当作所有扩展已经安装。下一步须按 Passport-02 至 Passport-09 和 API-F／S／P／O 前置闭合后，逐项接入同一选择清单并运行双认证及生成项目的真实联测；真实邮件／短信投递和其他发布数据库矩阵仍需对应环境，不改变 v0.1.0 发布门禁。


## 本地提交授权（2026-11-20）

用户在侧对话明确要求「提交代码」，覆盖本任务此前不暂存／不提交的限制；本次仅提交已验收的初始化实现、必要配套修复、测试、三语 Artisan 正文及所属计划与记录，不推送。17 份功能源码／测试／正文 SHA 与第三批已验收快照一致，沿用上述验收结果，不将提交动作描述成重新执行所有平台测试。ViewData 原生值、Passport、历史 WIP 等其他任务改动排除。完整扩展集的未就绪状态保持不变。实际提交号及文件清单保存于 `/private/tmp/openrust-init3-20261119/init-commit.json`。

提交前范围内 Rust 格式、Python 语法和暂存差异空白检查再次通过。整仓 `cargo +1.94.0 fmt --all --check` 此时仅报告并行 Passport `tests/client_lookup.rs` 的格式差异；未修改或纳入该文件，第三批历史整仓 Clippy 通过记录仍为当时快照的结果。
