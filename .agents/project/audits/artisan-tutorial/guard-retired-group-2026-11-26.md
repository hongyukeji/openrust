# 已退出进程组的停止收尾（2026-11-26）

本轮时间辅助入口门禁触及32GB，`stop` 已等待主进程退出后，末尾 `os.killpg(..., SIGTERM)` 返回 EPERM。原失败日志 `target/tmp/artisan-tutorial-20261125/time-gates.log` 保留，不计严格检查通过。此前仅诊断的历史记录不改写；原失败没有当场的组成员快照，不能追认全部历史 EPERM 都来自同一状态。

本机 Darwin 27.2.0 已独立连续4次复现：创建本批独占进程组，子进程退出但暂不回收，`ps` 确认组内只有 Z 状态，再发送 SIGTERM，4次均由真实系统调用返回 PermissionError/errno1。进程最终均 wait 回收。公开 [XNU killpg1](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_sig.c) 先过滤僵尸，找不到可投递成员时可返回 EPERM；[官方系统调用说明](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/killpg.2.html) 作为权限语义参考。网上 main 是机制解释，并非本机精确内核构建声明；本机实测为验收依据，无复制第三方源码。

修复仅在 PermissionError 分支检查同一进程组的实际状态：没有存活成员时不再把退役组当成停止失败；存在任何非僵尸成员、进程表读取失败或格式无法核实时，原权限错误继续传播。没有 blanket catch、提权、忽略存活构建或改变锁／清理授权；成功与 ESRCH 路径不增加进程表扫描。

实际17项守卫回归全部通过，包含真实macOS反例、存活成员不得抑制、检查失败不得抑制、受控尾部竞态后仍执行停线清理，以及原有锁／归属／缓存／退出码。6个正式守卫CLI取消（SIGINT／SIGTERM各3组）退出130／143，stderr为空，同组无存活父子。真实零存活组复现与受控竞态注入分别记录，未把注入当作系统复现。证据：`target/tmp/artisan-tutorial-20261125/guard-stop/{retired-group-before.json,retired-group-tests.log,retired-group-cli-results.json}`，各CLI原日志同目录。

本批不修改框架运行行为、配置、公开契约或产品教程，无需重复三语产品页面／Rust构建。仅工具源码、测试及内部记录；原生工具回归用项目内临时目录，不删除真实编译缓存；6组子进程均回收，继续复用现有缓存。时间功能仍在独立快照继续最终门禁，不夹带本提交。仅本地提交，不推送。
