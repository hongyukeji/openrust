# 环境文件编辑验收

日期：2026-11-24。范围为明文环境文件编辑，以及 `init`、`key:generate` 的共用写入层；不改变进程环境优先级、运行时配置或缓存刷新机制。

## 选型与复用

- 固定框架基线 [13.40.0 / KeyGenerateCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/KeyGenerateCommand.php)：保留密钥命令的生产确认、显式文件选择、严格原始 APP_KEY 匹配及运行时密钥更新。上游没有通用的 `.env` 写入门面，`config::EnvironmentFile` 是独立扩展，不给不可变 `Environment` 增加写入职责，也不将 `Config::set` 改成落盘。
- [Laravel-Dotenv-Editor](https://github.com/JackieDo/Laravel-Dotenv-Editor)、[EnvEditor](https://github.com/GeoSot/EnvEditor)：参考独立编辑会话、批量保存、格式保留与文件编辑／配置缓存分离；不移植 PHP 运行依赖。
- [adotenv 1.5.4](https://docs.rs/adotenv/1.5.4/adotenv/)（MIT OR Apache-2.0，MSRV 1.81）仅用于语法结构与字节跨度定位，关闭默认 feature 并固定版本。不把较新发布的库描述为长期成熟组件；以本项目兼容性测试约束。继续使用现有 dotenvy 0.15.7 作为读取语义及最终写入校验，不替换应用加载器。
- 实测发现 adotenv 默认序列化对中文空白、反斜线及美元引用的输出不能直接交给现有加载器。因此不采用其序列化器，而是使用小型、受测试约束的 dotenvy 字面字符串编码器，显式区分 `set` 与 `set_expression`。不使用自行实现的整份 dotenv 解析器。
- 核对旧版 `openrust-v0.x` 的 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`：`src/console/command/key_file.rs` 已有阶段写入、原文比较、严格密钥匹配用例，但没有完整的通用编辑器，且存储层不满足当前 Windows ACL 与多进程锁契约。保留当前主线已有的严格匹配逻辑，借鉴旧版失败不覆盖的回归边界，不整体搬回旧实现。
- Windows 替换依据 [ReplaceFileW 官方契约](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew)：先关闭临时文件句柄；保留 ACL。错误 1176／1177 可能已经改变路径状态，保留恢复文件并返回 `ReplacementUncertain`，不把它当作普通未写入错误重试。

## 实现与消费者

- `EnvironmentFile::for_application(&app)` 读取应用实际绑定的 `env.file`；显式 `open`／不覆盖 `create` 处理独立文件。
- `get`／`get_raw`／`contains`／`all`、字面值／表达式设置、原子批量写入、删除及独立 `EnvironmentEditor` 会话。
- 注释、顺序、export、空白、原引号、BOM、LF／CRLF、多行值及无末尾换行的未修改部分按原字节保留。相同值不更新 mtime；失败的批量修改不污染会话或文件。
- 同目录临时文件、稳定 `<文件名>.lock`、先锁后读、乐观冲突检测；显式解锁避免 macOS fork/exec 窗口继承描述符导致已完成操作短暂占锁。
- Unix 保留权限及 uid/gid，新文件 0600；密钥命令继续收紧到 0600。Windows 采用目标 ACL。拒绝只读目标、目标符号链接／重解析点，限制 UTF-8 正文 8 MiB。错误及编辑器 Debug 不包含值。
- `init` 只更新确有变化的值，保留未修改表达式；仍将缺失密钥交给原 `key:generate` 生成。应用加载器显式支持 UTF-8 BOM。
- 三语配置文档同步说明 API、缓存与常驻进程刷新、锁文件与恢复边界。

## 实际验收

隔离源码：`target/tmp/environment-file/source/`。原始输出统一保留于 `target/tmp/environment-file/`，不提交源码副本、Cargo 缓存或原始日志。

| 验收 | 实际结果 |
| --- | --- |
| macOS 框架测试 `cargo test -p openrust-framework --test environment_file` | 17 项通过（其中 2 项为子进程辅助入口）；含 196 组字符组合、两个真实写入进程、强制终止持锁进程后恢复、符号链接拒绝 |
| 根应用 `cargo test -p openrust --test environment_file --test encryption --test init --test environment_lifecycle --test configuration_cache --test configuration --test environment_encryption` | 共 28 项通过（含额外的 3 项环境文件加解密回归）；包含实际 Artisan 密钥生成、选定环境文件、配置缓存及 HTTP 缓存消费者；新增应用快照与下一次引导隔离验收 |
| 实际 `artisan init --extensions=grpc --without-migration --no-interaction` | SQLite 独立文件连接通过，完成密钥生成、应用重建与 config:clear；未修改的 MAIL_FROM_NAME 引用逐字保留。此项有意不执行迁移，不声称全套安装或数据库矩阵验收 |
| Linux | `rust:1.94.0-bookworm` ARM64 容器，同一份核心源码与测试 17 项通过；tmpfs 数据／构建，无数据库或数据卷 |
| Windows 11 ARM64 原生 NTFS | Rust 1.94.0 MSVC，同一份核心源码 17 项通过，含独占锁、进程中断、并发写入及受保护 ACL 保留；不是 Linux 模拟，也不是整套 Windows 应用部署验收 |
| 严格 Clippy | `cargo clippy -p openrust-framework --lib --test environment_file -- -D warnings` 通过 |
| 文档 | `npm --prefix docs run build` 通过，包含三语内容／命令入口检查及 VitePress 构建；现有 bundle 大小提示仍存在 |
| 格式 | 定向 rustfmt 与 git diff --check 通过 |

便携夹具由 `python3 -B tests/environment_file_portable.py --prepare-only` 直接复制交付源码及同一测试文件到项目内，不伪造文件系统或锁实现；可在各系统以 `cargo +1.94.0 test --manifest-path <夹具>/Cargo.toml` 复验。Windows 本次通过锁定依赖的离线 vendor 副本克服索引连接阻塞，vendor 与临时 Cargo 清单均不提交。

根应用实际初始化复验入口：在未初始化的独立源码快照上运行：

```sh
python3 -B tests/build_guard.py --workflow environment-file -- \
  python3 -B tests/environment_file_acceptance.py \
  --source target/tmp/environment-file/source \
  --binary target/tmp/environment-file/cargo-target/debug/main
```

## 明确边界

- 文件写入不自动刷新配置缓存、现有 Environment 或常驻进程；显式重新缓存并重启。临时文件使用已被项目环境文件忽略规则覆盖的 `.env.edit-` 前缀，避免异常退出遗留明文被意外加入 Git。加密环境文件由已有 env:encrypt／env:decrypt 管理，不向明文编辑器交付密文。
- 锁对同一协议的写入者有效，不能承诺与不遵循锁的任意外部编辑器完全事务隔离；保存前会检查原文，不能将最后比较与替换间的非协作竞争描述为已消除。
- `DurabilityUncertain` 和 Windows `ReplacementUncertain` 的真实磁盘故障未通过断电／磁盘故障注入重现；实现按系统契约报告不确定状态，后者保留恢复文件。网络文件系统不在本次实测范围。
- 保留已有其他协作者的缓存、维护模式、通知等改动；提交仅包含本主题的实现、依赖、测试、三语文档和本记录，不推送。

## 资源处理

收尾时触及 32 GB 停线，最后一次框架构建被守卫安全中断。其他构建退出后项目守卫完成持锁清理：事件记录的可用空间从 31,588,491,264 字节增加到 102,876,880,896 字节，净释放约 71.29 GB（66.39 GiB），保留锁文件原 inode、源码、验收日志、依赖下载及数据库。此数字是同一次项目级清理的合计，不全归于本功能。

清理后重新执行定向验收；三系统核心源码测试均包含最后新增的超限／非 UTF-8 拒绝测试。Linux 容器使用 `--rm` 与 tmpfs，结束后已不存在本主题容器或数据卷。Windows 一次性 NTFS 构建缓存经 Cargo clean 回收约 50.5 MiB（客体可用空间实测增加 51,572,736 字节），虚拟机恢复任务开始前的暂停状态，保留测试源码、离线依赖及日志。正常后续验收缓存按项目规范复用。

收尾磁盘复查：可用约 68.91 GB；后续并行构建会继续改变此数值。验收缓存正常复用，未删除共享缓存、数据卷或发布文件。

## 补充：定位、空行与独立文档编辑（2026-11-24）

在首批 `8e881a75f` 基础上增加位置选项，不改变原有 `set` 默认行为。复查 [dotenv-manager 的 before/after 接口](https://github.com/markwalet/dotenv-manager) 与前述编辑器的空行／原文编辑设计；前者已归档，仅参考接口表达，不作为运行依赖，也不移植其实现。

- `EnvironmentWriteOptions` 独立表达 `before`／`after`／文件首尾、两侧空行数量和显式移动策略。`blank_lines(1)` 表示前后各一行空行，0 表示不加额外空行；未指定侧保留原有间距，显式数量规范化相邻空白行。
- 已有键默认只原位更新值，空行设置仍生效；`move_existing(true)` 才重新定位。缺失／重复锚点、自定位和超限间距失败时均不改变文件或会话。已有键不移动时不解析插入锚点。
- 声明按解析跨度定位，支持多行值，移动保留 export 和行尾注释；独立注释不推测归属、不跟随移动。锚点前表示声明前，不包含其上方独立注释。
- 位置已满足时直接整理原位置的间距，避免重复移动留下自己的空行。保留 LF／CRLF、BOM 和未改变的末尾换行。另修复删除 BOM 后第一条声明时可能同时移除 BOM 的边界。
- 公开 `EnvironmentDocument`，提供内存字符串读取、修改、定位、批量操作和删除；不依赖 Application、容器、文件路径或持久化方式。文件编辑会话直接复用它，不维护另一套编辑逻辑。二开可用自己的存储及并发策略持久化 content；原文件层继续提供文件锁、冲突检测和原子保存。引用读取继续遵循既有 dotenvy 语义，含进程环境回退；不宣称完全脱离进程环境的确定性解析。
- 文件、会话、内存三类入口都实际调用验收。默认选项与旧 set 的行为对比一致；新增表达式定位、显式移动、缺失与重复锚点、错误原子性、BOM、多行、注释及间距规范化回归。

实际结果：macOS 框架测试与便携源码、Linux ARM64 容器各 25 项通过，Windows 11 ARM64 NTFS 26 项通过（包含 2 个子进程辅助入口）；覆盖额外 144 组位置／空行数量／LF 与 CRLF／末尾换行组合及原有 196 组字符往返。Windows 保留受保护 ACL 的回归继续通过。

根应用定向 19 项通过：`--test environment_file --test encryption --test init --test configuration_cache`；新增实际 Artisan 读取指定环境文件中定位插入的 MAIL_PORT=2527，配置值正确。框架 `--lib --test environment_file` 严格 Clippy、定向 rustfmt、diff 检查以及三语文档构建通过。原始日志使用 `target/tmp/environment-file/position-*.log`；Windows 日志为同目录 windows.log。

本轮无新依赖。Linux 使用既有镜像、离线锁定依赖及 tmpfs，容器结束即清理；Windows 临时编译缓存验收后经 Cargo clean 清理并恢复虚拟机暂停，保留源码、依赖与日志。文件系统／断电边界沿用前文；正常增量缓存按现行规范保留。

Windows 最终并发复验发现 ReplaceFileW 路径切换窗口可能使同时打开文件的进程短暂得到 NotFound。读取层现仅在此错误且稳定锁文件存在时等待协作写入者，取得同一锁后重读，最长 100 ms，超时返回 Busy。正常读取不创建锁文件、不增加只读文件的写权限要求；真正缺失且无活动写入的文件立即报错。增加人为持锁切换目标路径的确定性回归，验证读取超时返回 Busy、恢复目标后可继续读取；修复后 Windows 完整 26 项及额外 10 轮真实双进程写入均通过，未把原生瞬态读取错误简单加入忽略列表。

本轮最后一次 Windows Cargo clean 回收 505 个文件、约 55.5 MiB；客体可用空间实测增加 56,418,304 字节。原始验收记录保留，未触及项目 32 GB 停线。
