# 双包应用的目标平台依赖许可核对

使用 `d1677c1b0` 的根清单及锁定依赖、Rust 1.94.0 和 cargo-deny 0.20.2，在 macOS ARM64 检查正式应用只启用 `webhook-client,webhook-server` 时的依赖图。对每个目标分别执行 Cargo 的普通及构建依赖树和 cargo-deny 的许可清单，排除开发依赖及其他可选扩展。

| 目标 | 普通依赖图条目 | 普通与构建依赖图条目 | 无许可标签的条目 |
| --- | ---: | ---: | ---: |
| `aarch64-apple-darwin` | 502 | 518 | 0 |
| `x86_64-unknown-linux-gnu` | 502 | 518 | 0 |
| `aarch64-unknown-linux-gnu` | 501 | 517 | 0 |
| `x86_64-pc-windows-msvc` | 514 | 530 | 0 |
| `aarch64-pc-windows-msvc` | 513 | 529 | 0 |

按 crate 版本去重并包含根包。五个图均包括锁定的 `standardwebhooks 1.3.0`（MIT）和 `hmac-sha256 1.1.15`（ISC）。`tests/webhooks_licenses.py` 同时核对两个依赖的 checksum 与两包附带的许可原文。全应用还包含 MPL-2.0、CDLA-Permissive-2.0 和多许可证 native 组件等声明，不能只用两个包的 MIT 文件作为应用制品的清单。

```bash
cargo +1.94.0 tree --locked --offline --no-default-features \
  --features webhook-client,webhook-server --target aarch64-apple-darwin \
  --edges normal,build --prefix none --format '{p}|{l}'
cargo +1.94.0 deny --locked --offline --exclude-dev --no-default-features \
  --features webhook-client,webhook-server --target aarch64-apple-darwin \
  list --format json --layout crate
```

原始输出在 `target/tmp/webhook-artifact-license-review/`。此批验证声明图与随源码保存的新增依赖许可。正式制品仍需核对实际链接和打包成分、附带原文与声明，并按发布平台及选用功能复跑。Linux 和 Windows 的依赖解析没有执行目标机器上的编译、HTTP 或 Worker。

<a id="docker-runtime-licenses"></a>

## Docker 运行镜像

沿用 v0.x 的 `6d1a81924:Dockerfile` 及现行 [08-04 多阶段镜像](../08-04/docker.md) 的构建目录交付方式。构建阶段以同一目标、锁文件和功能参数执行 `cargo tree --edges normal`，从实际应用依赖图选择 Webhook 包。默认构建无 Webhook 声明目录。仅通过 `openrust-content-moderation/webhooks` 引入时附带接收包的文件。两包启用时，在 `/app/licenses/<包名>/` 分别附带声明与 Spatie、Standard Webhooks、HMAC 的许可原文。

本机以 Rust 1.94.0 复查六种锁定依赖图，包括默认、单接收、单发送、双包、仅内容审核和内容审核间接引入接收包。镜像检查从非特权只读容器读取实际文件，并与源码逐字节比较。

`d729d6b84` 的干净归档叠加本批 Dockerfile，在本机 Docker Desktop 原生 ARM64 构建完整 Release 镜像。双包镜像的 10 个许可文件均与源码逐字节一致，包括根项目许可证和声明。运行 UID/GID 为 10001，动态链接项包括 Debian 的 PCRE2、libgcc 和 glibc。运行镜像携带三者的版权文件，已读取并记录摘要。

独立构建内容审核间接引入接收包的原生 Release 镜像。其 6 个许可文件逐字节一致，发送包目录缺席。实际 Artisan 命令表包含 `moderation:recover`。两个 Webhook Provider 未直接安装，因此命令表没有它们的命令。

默认 Release 镜像保留根项目的 2 个许可文件。镜像未附带 Webhook 声明目录，Artisan 包发现成功且清单未列出两个 Webhook 包。

使用只读根文件系统和独立 SQLite 存储执行 HTTP 与 Artisan。缓存前 `/up` 返回 200，包发现识别双包，37 项迁移完成且重跑无待执行项。配置、路由和包清单缓存均写入挂载目录。接收入站积压为空，缺省应用的出站恢复拒绝缺失的 `delivery.policy`，独立 Artisan Worker 检查空数据库队列后退出。重启后 `/up`、`/` 和 `/api` 均返回 200，SIGTERM 退出码为 0。[带业务 Job 的两端验收](native-platforms.md)另有真实投递及 Worker 证据。

早期的双任务编译期间 Docker 引擎停止响应，取消本批两个构建请求后恢复。同一源码和 Release 配置使用 `APP_BUILD_JOBS=1` 完成。构建、许可清单和运行证据保存在 `target/tmp/webhook-runtime-image/`。其他架构及发布渠道仍按实际制品复核。

```bash
python3 -B tests/webhooks_licenses.py --image openrust:webhook-licenses-arm64 \
  --package webhook-client --package webhook-server
python3 -B tests/webhooks_licenses.py --image openrust:webhook-licenses-transitive-arm64 \
  --package webhook-client
python3 -B tests/webhooks_licenses.py --image openrust:webhook-licenses-default-arm64
```
