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
