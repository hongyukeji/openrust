# 01-04 Windows 原生系统日志与整单元闭合

以固定 Laravel 的 syslog 渠道和 PHP 8.5.0 Windows 实现为参照。Windows 使用 `RegisterEventSourceW` 与 `ReportEventW` 写入 Application 事件日志。应用名作为 Provider，进程标识和格式化消息分开保存，类别取 syslog 级别。和 PHP 一样，Alert、Info、其他级别分别映射至 Error、Information、Warning。应用级 Provider 名取代 PHP 运行时版本名，使用原始事件插入字符串查询正文。Windows 校验 facility，系统事件结构不保存该字段。Rust 应用级渠道及错误回退沿用 v0.x 的工厂结构。固定 Goravel 用于比对编译时应用装配。 [固定 Laravel LogManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Log/LogManager.php#L399) · [PHP Windows syslog](https://github.com/php/php-src/blob/php-8.5.0/win32/wsyslog.c) · [Windows Event Log API](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-reporteventw)

Windows 回归发现，追加专用句柄无法在 Windows 上加独占文件锁。修改后，启用 `locking` 的 Windows 文件以可写方式打开，加锁后定位文件末尾，再写入并释放锁。修复前两项写文件回归失败，修复后同一原生目标全部通过。Unix 保持追加式文件句柄。

隔离基线 `a3e3632be`，同一 Parallels Windows 11 ARM64 虚拟机、Rust/Cargo 1.94.0、HostARM64 MSVC 14.44.35207 和 ARM64 clang-cl。验证六个候选文件的 SHA-256 与宿主源码一致。AWS-LC 的 ARM64 汇编使用 clang-cl。锁定离线 `cargo test --target aarch64-pc-windows-msvc -p openrust-framework --test logging` 通过 14 项。真实双应用 TCP HTTP 和 Artisan 子进程都进入 Windows Application 事件日志。通过 PowerShell 查询事件原始字段验证 Unicode、级别及类别、过滤、格式化、PID、应用隔离、过长插入字符串的紧急回退和同一渠道恢复。原生 `main.exe` 的 PE Machine 是 `0xAA64`，真实 `main.exe artisan inspire` 通过。

Windows Clippy 的首次严格运行在既有 `queue/worker_signals.rs` 的 `clippy::never_loop` 停止。该文件未改，验收命令仅放宽此项后通过其余警告的严格检查。macOS arm64 的 18 项日志、Context 和管理器属性回归、所有本批源码格式、定向严格 Clippy 及实际 Artisan 通过。三语 Unix 示例分别运行，Windows PowerShell 路径经真实事件测试覆盖。VitePress 构建及 24 项浏览器通用、Unix 与 Windows 章节检查通过。

01-04 的轮转、保留上限、临时渠道、栈顺序和工厂错误见[轮转和工厂验收](README.md)。[格式化器、processor 与 tap](customization.md)、[配置处理器与缓冲](handler-channels.md)、[Unix syslog](syslog.md)及[JSON 异常上下文](json-exceptions.md)已各自验收。Windows x64 兼容层复验归发布平台矩阵。 [英文](../../../../docs/en/the-basics/logging.md#syslog-channel) · [简体中文](../../../../docs/zh_CN/the-basics/logging.md#syslog-channel) · [繁體中文](../../../../docs/zh_TW/the-basics/logging.md#syslog-channel)
