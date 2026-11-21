# SMTP 依赖安全修复

2026-11-20，以 `dee8ae7fb` 为源码基线，隔离源码及独占 target 验收。框架精确锁定 lettre 0.11.24；Cargo.lock 仅修改 lettre 版本与校验和，未升级其他依赖。固定 Rust 1.94.0，macOS ARM64，jobs=3、增量关闭。旧版 v0.x 只有 lettre 0.11 系列依赖与 SMTP feature，无可直接移植的本次修复；沿用官方协议实现，不另写生产 SMTP 编码器。

## 实际验收

- `cargo +1.94.0 test --locked -p openrust-framework --test mail_notifications smtp_security_ -- --nocapture`：4 函数通过。真实回环 TCP 检查空白 EHLO 扩展仍可投递，带引号地址的 RCPT 信封不变；100,000 字节以上无换行回复与 STARTTLS 回复后夹带明文均关闭连接且失败，不继续发送命令或 TLS 握手；直接通过选定 lettre 异步 SMTP 层检查裸 CR／LF 点转义。显示名 CR／LF 返回框架组装错误，无投递。
- 精确运行现有 MIME／应用隔离、sendmail 两模式、HTTP 供应商适配和只读 HTTP 预览共4函数，全部通过；不依赖未配置环境导致的跳过，不执行 SQL，不改动开发 PostgreSQL／Redis。
- 临时回退至 0.11.23 后，点转义、空白 EHLO／引号信封、超长回复三个函数各自实际失败（退出101），随后恢复锁文件与清单并重跑4函数全过。旧版超长回复先触发失败，不能将该轮称作旧版 STARTTLS 独立负例。
- `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked -- -D warnings`、`cargo +1.94.0 fmt --all --check` 通过。
- 三语 VitePress build 和 `check_browser.mjs` 的 `/`、`/docs/`、桌面／移动12组合通过。

## 公告与范围

官方 RustSec 仓库及原文 SHA 在 `references.json`。RUSTSEC-2026-0560～0565 的修复版本均为 lettre 0.11.24；覆盖 STARTTLS 回复注入、裸换行 SMTP smuggling、EHLO 空白 panic、引号本地部分误投、显示名称换行 panic 和回复缓冲无界。生产仍使用原有 TLS／凭据策略，API 不变。没有将本地测试称作外部供应商最终投递。

升级后重跑 `cargo deny --format json --locked --workspace --all-features check advisories`：lettre 六条已消失；命令仍因 libsqlite3-sys、rsa 漏洞及 chrono／chrono-tz／paste／pin-utils 维护公告退出1，未添加 ignore。继续独立修复可升级依赖，维护公告另行列出真实范围，不能宣称整个依赖审计通过。

证据保留于 `/private/tmp/openrust-mail-security-20261120/`：`regressions.log`、`negative-control.log`、`restored.log`、`clippy.log`、`fmt.log`、`docs.log`、`browser.log`、`advisories-after.jsonl`。`source-sha256.json` 对应实际验收输入。闲置专属 target 已直接回收，源码／日志／共享缓存与全部数据卷保留，实际空间见 `cleanup.json`。仅本地提交，不推送。


## 2026-11-21 系统与私有CA信任

本次在lettre0.11.24不变的前提下，启用已有rustls-native-certs feature，让SMTP使用系统/进程配置的可信根并继续验证证书链与hostname。隔离Mailpit1.27.10实际验证STARTTLS/SMTPS、535/550、错误CA/hostname、禁止明文降级及独立TLS1.2/1.3握手共12场景；断言成功MIME进入SQLite收件箱、失败零投递。4项历史SMTP安全函数也复验通过。三语说明、严格检查及最新公告结果见[完整模拟记录](../08-04/README.md#2026-11-21-真实验收缺口盘点与模拟环境复验)。真实SMTP最终收件人和云邮件供应商仍待目标验收，不能据本地Mailpit称已生产投递。
