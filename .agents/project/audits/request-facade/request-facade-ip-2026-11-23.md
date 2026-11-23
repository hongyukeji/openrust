# Request 门面客户端 IP 验收（2026-11-23）

后续完整门面工作见 [本批复核](README.md)。本记录保留前置 IP 单元的历史验收边界，不代表当前完整门面状态。

## 范围与依据

固定框架基线 v13.40.0 的 `Illuminate/Support/Facades/Request.php` 声明 `ip`、`ips`、`getClientIp`、`getClientIps` 四个静态入口。本次提供对应的 Rust 原生命名入口，沿用已有可信代理解析和请求任务作用域，不引入第二套代理头解析或全局请求缓存。

- 公开入口：`openrust::support::facades::Request::{ip, ips, get_client_ip, get_client_ips}`。
- 返回值：`Result<Option<IpAddr>, BindingResolutionError>` 和 `Result<Vec<IpAddr>, BindingResolutionError>`。请求内缺少传输地址返回 `None`／空列表，请求外返回 `FacadeNotSet`；独立任务不继承请求作用域。
- 请求实例补齐 `get_client_ip`／`get_client_ips` 别名。正常响应和来源校验失败的异常渲染均使用同一请求地址作用域。
- 旧版 `openrust-v0.x` 的 `http/request.rs` 已有 `client_ip`／`client_ips`，通过 `proxy_context` 获取地址；本次复用当前框架已有的 `Request::ips` 和任务作用域，不移植旧代理基础设施。
- 三语用法：`docs/{en,zh-Hans,zh-Hant}/the-basics/requests.md#request-ip-address`。

## 实际验收

当前工作区与并行 N-03 命名改造组合验收；没有回退其他协作者的改动，没有操作索引、提交或推送。

```sh
python3 -B tests/build_guard.py --workflow request-facade-final \
  --target-dir target/tmp/rust-naming/cargo-target -- \
  cargo +1.94.0 test --locked -p openrust-framework --test request_facade
```

结果：11 项通过，0 失败。覆盖 IPv4／IPv6／映射地址、无传输地址、可信代理链、Forwarded、非可信代理伪造头、冲突头 400、无效路径 400 的异常渲染、48 个并发请求及双应用隔离、取消后的清理、独立任务隔离。真实 TCP 测试分别使用非可信和可信 loopback 代理策略，请求结束后停止监听并确认端口关闭。

源码 SHA 清单：`target/tmp/request-facade-20261123/final-source-sha256.json`。运行日志：同目录 `tests-workspace-final.log`。此前隔离基线的 10 项通过和中途失败记录保留，均不替代最终 11 项结果。

最终质量检查通过：`cargo +1.94.0 fmt --all --check`；同一受守卫保护的缓存执行 `cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings`，退出 0（1m 11s）。日志分别为 `fmt-workspace-final.log`、`clippy-workspace-final.log`。检查后复核六份 IP 源码 SHA 未变化；并行协作者的额外消费者结果单独归其命名验收，不在本记录重复计数。

## 保留边界

本次仅闭合客户端 IP 四个入口，不声明整个 Request 门面完整对齐。静态声明清单见 [逐项清单](request-facade-inventory-2026-11-23.json)：其余 188 个静态入口待逐项核对，候选拼写不代表接口存在；通用 Facade 根解析、替换和测试替身也未纳入本批。其他请求操作仍使用注入的请求实例。

本批未运行数据库或外部代理服务器验收；网络验收为本机 TCP 与显式设置的可信代理协议头。无数据库、数据卷或共享缓存改动。构建缓存按项目规则保留增量复用；日志和源码快照保留。
