# 第三方来源

包结构、公开契约和对照测试参考固定 Spatie Webhook client；原始 MIT 许可保留于 `LICENSES/Spatie-MIT.txt`。实现为 Rust，不包含 PHP 运行时。

签名协议复用 crates.io `standardwebhooks =1.3.0`（MIT；发布源码提交 `daee8284cfe9d4e435ec16e3e68a8f30084d6aaf`，Cargo.lock 记录内容校验和）；这不是后续 GitHub 调研快照 `4a8ee40c5bcb6157f856905cc8871040d41a78ce` 的同一份源码。许可见 `LICENSES/StandardWebhooks-MIT.txt`。其传递依赖 hmac-sha256 1.1.15 为 ISC，许可见 `LICENSES/HmacSha256-ISC.txt`。其余传递依赖的身份与版本保持在 Cargo.lock。

官方 Standard Webhooks 固定签名向量用于协议互操作验证；另用 Python 标准库 HMAC 产生原始字节反例向量。未执行 PHP／Composer 测试，不将 Rust 测试计为上游完整测试通过。
