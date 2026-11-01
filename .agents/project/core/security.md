# `openrust::security` 当前事实

> 更新：2026-10-26
> 摘要：`encryption` feature 提供应用进程内 AES-256-GCM-SIV 加密与本地 key ring 轮换；不是 TLS、密码哈希或数据库备份加密的替代品。

- 公开 API：`EncryptionConfig::new(active_key_id, keys)`、`EncryptionServiceProvider` 与 `Encryptor::{encrypt,decrypt,encrypt_with_context,decrypt_with_context}`。根 facade 的 `prelude` 重导出服务和插件。
- 每把 Base64 key 必须解码为 32 bytes；key ID 为 1–64 个小写 ASCII 字母、数字、`_` 或 `-`；总 key 数 1–16，必须含 active key。所有配置错误与 `Debug` 输出均不暴露 key 值。
- 每次加密使用 OS CSPRNG 的 96-bit nonce。`or1.<key_id>.<nonce-b64url-no-pad>.<ciphertext-tag-b64url-no-pad>` envelope 将格式版本、key ID 和最多 1 KiB 的调用者可信 context 绑定至 AAD；明文上限 1 MiB。解密只按 envelope key ID 查找，不试探所有 key。
- active key 用于新密文，历史 key 只解密。轮换通过让所有实例同时加载新旧 key、切换 active key、由应用迁移数据并验证备份，最后手动退休旧 key；框架不热更新或自动重密文。
- `openrust key generate` 将 32-byte OS 随机 key 以标准 Base64 打印到 stdout，不写文件。CLI `openrust new --encryption` 启用 feature、插件和环境变量示例。
- 算法规范为 RFC 8452；所用 `aes-gcm-siv` crate 文档目前声明其实现本身没有完整独立安全审计。版本发布前需要复核审计/维护状态并评估目标平台性能。
- 验收记录见 [testing](../memory/testing.md) 和 [verification](../memory/verification.md)；实现决策见 历史决策 034。
