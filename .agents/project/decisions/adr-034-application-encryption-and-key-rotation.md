# ADR-034：应用数据加密与本地密钥轮换

- 状态：Accepted
- 日期：2026-10-26
- 依据：[v1 通用能力计划](../plans/v1-common-capabilities.md#2-通用加密与密钥轮换)、[ADR-022 JWT 运行时 contract](adr-022-jwt-runtime-contract.md)
- 算法依据：[RFC 8452](https://www.rfc-editor.org/rfc/rfc8452.html)、[RustCrypto aes-gcm-siv](https://docs.rs/aes-gcm-siv/0.12.1/aes_gcm_siv/)

## 决策

1. 在 `encryption` Cargo feature 下提供 `openrust::security::{EncryptionConfig, EncryptionPlugin, Encryptor}`。`EncryptionPlugin` 从 `[security.encryption]` 读取配置并注册 `Encryptor`；不用插件的应用不要求加密配置。该能力仅保护应用主动提交给它的数据，不代替 TLS、密码哈希、JWT 签名或数据库/备份整体加密。
2. 固定 AES-256-GCM-SIV（RFC 8452）作为 v1 AEAD。每次加密通过 OS CSPRNG 生成 96-bit nonce。该算法对 nonce 意外复用比普通 GCM 更有韧性，但不以此取代随机 nonce；所有消息仍使用新随机 nonce。不提供算法/模式/填充选择项，不自研密码算法。
   当前 `aes-gcm-siv` crate 文档声明该 crate 本身尚无完整的独立安全审计；算法标准化不等于实现获得审计认证。发布前需重新评估实现维护与审计状态，并确认目标 CPU/平台性能满足应用需要。
3. `EncryptionConfig` 包含 `active_key_id` 与 `keys: BTreeMap<key_id, base64_secret>`。每个 key 解码后恰为 32 字节；ID 使用 1–64 个 ASCII 字母、数字、`_` 或 `-`。最多配置 16 把钥匙，必须包含 active ID。当前 key 用于所有新密文，其他 key 只用于解密。配置 `Debug` 只显示 ID 与密钥数量，不显示密钥；配置错误不回显任何密钥值。密钥由现有 Figment/`.env`/进程环境层级提供，不设默认密钥。
4. `Encryptor::encrypt` / `decrypt` 接收与返回 bytes；另提供 `encrypt_with_context` / `decrypt_with_context` 绑定由可信调用者提供的应用上下文。调用者须在解密时提供相同上下文，不可从待解密 envelope 复制上下文。明文最大 1 MiB，上下文最大 1 KiB；不承诺大对象流式处理。
5. 密文 envelope 为 `or1.<key_id>.<nonce_base64url_no_pad>.<ciphertext_and_tag_base64url_no_pad>`。AAD 由固定域分隔前缀、版本、长度前缀 key ID 和调用者上下文组成；解析严格限定四个字段，拒绝未知版本、尾随字段、非规范 Base64、越界 envelope、错误 nonce 长度及短标签。未知 key ID、AAD 不匹配、错误密钥或篡改返回稳定且脱敏的错误，不回退、不 panic。
6. 生成命令为 `openrust key generate`，仅输出一个 32-byte OS CSPRNG key 的标准 Base64 值到 stdout；不修改配置文件、Keychain 或环境变量。该输出是敏感信息，操作者应将其放入受控 secret store，并保持 `.env` 未跟踪。
7. `config show` 必须递归遮蔽 `keys` 对象及其他密钥 map 的值，同时保留非秘密配置形状；Figment/配置验证错误不包含原始密钥。
8. 加密密钥轮换通过配置和滚动重启：先将新、旧 key 安全加载到所有实例，再改 active ID，最后在确认仍需读取的数据、备份和副本均可由新 key 处理后才可人工移除旧 key。框架不热更新、不自动重加密、不扫描或修改用户数据。

## 验收

- 离线单元/集成测试：RFC 8452 已知向量、二进制/空明文往返、nonce 随机性、多实例互解、显式上下文、当前 key 写入、旧 key 解密、active key 切换、未知/退休 key、篡改 envelope/nonce/tag、错误上下文、非法/超限输入、配置与错误 Debug 脱敏。
- `config show` 和配置提取测试证明 key map 与错误不泄密；`openrust key generate` 测试检查输出可解码为 32 bytes 且不写文件。
- `根目录集成宿主` 仅在 `encryption` feature 下注册插件；CLI 生成项目 `--encryption` 的 check/test 与最简运行配置示例通过。
- 默认和 all-features workspace fmt/clippy/test/docs、cargo deny、生成项目验证全部通过。无外部密钥服务和网络依赖。

## 轮换操作

设原 active 为 `old`，新 key ID 为 `new`：

1. 在所有实例配置中同时提供 `old`、`new`，仍以 `old` 为 active，滚动重启并确认实例能解密现存密文。
2. 将所有实例的 active 切换为 `new`，滚动重启；新写入使用 `new`，旧数据仍用 `old` 解密。
3. 应用按业务需要迁移已有数据并验证备份恢复后，才可在所有实例中移除 `old`。OpenRust 不自动迁移密文。
