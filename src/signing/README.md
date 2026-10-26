# 签名链接

启用 `signing` feature，并使用 `[security.signed_links]` 配置注册 `SignedLinksPlugin`。签名密钥使用独立 key ring，与 JWT 和应用加密密钥相互隔离。新链接使用当前 active key；保留的历史密钥可验证轮换前创建的链接。

```rust,ignore
let links = SignedLinks::new(SignedLinksConfig::new(
    "https://app.example",
    "links-2026-01",
    key_ring,
))?;
let link = links.issue("email.verify", "account:42", 900)?;
let claims = links.verify(link.expose(), "email.verify").await?;
```

Token 会绑定签发者、用途、主体、签发与过期时间、密钥 ID 以及随机 token ID。验证时必须指定预期用途，并遵守配置的最长有效期。持有者可以读取签名 token，因此 subject 不应包含秘密信息。`SignedLink` 的 `Debug` 输出会隐藏 bearer 值。

默认情况下，链接无状态，并持续有效至过期或签名密钥退役。若要跨应用实例撤销单个链接，请添加 `with_database_revocations(db)`，并将 `SignedLinkRevocationMigration` 加入应用现有的 migrator。共享表只保存 token ID 的 SHA-256 摘要。自定义共享撤销存储可实现 `SignedLinkRevocationStore`；生产默认配置不提供本地内存撤销存储。

撤销存储发生故障时，验证会失败关闭。URL 构造、链接投递、验证后的授权及响应行为由应用负责。现有 `VerificationCodes` 继续维持独立的一次性原子消费契约。
