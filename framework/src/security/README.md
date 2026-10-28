# 应用数据加密

启用 `encryption` feature 并注册 `EncryptionPlugin`，即可使用进程内的 `Encryptor` 服务。在
`[security.encryption]` 中配置 `active_key_id`，并在 `keys` 映射中提供 Base64 编码的 32 字节密钥。
`openrust key generate` 会将新密钥输出到标准输出；请将其存入密钥管理工具或已忽略的本地
`.env` 文件。

`encrypt` 和 `decrypt` 处理字节切片，并使用带版本的认证密文封装。
`encrypt_with_context` 会将调用方提供的可信上下文绑定到密文，但不会把上下文保存在封装中。
明文上限为 1 MiB，上下文上限为 1 KiB。

只有当前 active key 用于加密。应用迁移完所有数据并确认备份可恢复前，必须保留历史密钥用于解密。
轮换通过修改配置并滚动重启完成；OpenRust 不会自动重写应用数据。滚动部署期间，所有运行实例所需的密钥都必须保持可用。
