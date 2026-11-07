# 应用数据加密

`EncryptionServiceProvider` 在统一容器中登记加密服务，首次解析时读取生效配置并校验密钥。

`LaravelEncrypter` 对应 Laravel 的 `encryptString` 与 `decryptString`。它读取同一应用的
`app.cipher`、`app.key` 和 `app.previous_keys`，支持 AES-128/256 CBC 与 GCM。
默认 cipher 为 AES-256-CBC。用 `openrust key:generate` 生成应用密钥；配置中的
`base64:` 前缀只在加密服务读密钥时解析。新密文使用当前密钥，历史密钥仅用于解密。
明文上限为 1 MiB，载荷输入上限为 2 MiB。

启用 `encryption` feature 并注册 `EncryptionServiceProvider`，即可使用进程内的 `Encrypter` 服务。在
`[security.encryption]` 中配置 `active_key_id`，并在 `keys` 映射中提供 Base64 编码的 32 字节密钥。
`openrust key generate` 会将新密钥输出到标准输出；请将其存入密钥管理工具或已忽略的本地
`.env` 文件。

`encrypt` 和 `decrypt` 处理字节切片，并使用带版本的认证密文封装。
`encrypt_with_context` 会将调用方提供的可信上下文绑定到密文，但不会把上下文保存在封装中。
明文上限为 1 MiB，上下文上限为 1 KiB。

只有当前 active key 用于加密。应用迁移完所有数据并确认备份可恢复前，必须保留历史密钥用于解密。
轮换通过修改配置并滚动重启完成；OpenRust 不会自动重写应用数据。滚动部署期间，所有运行实例所需的密钥都必须保持可用。

现有 `Encrypter` 用于 AES-256-GCM-SIV/or1 上下文保护。Cookie、Session 与 Fortify 继续使用各自现有保护接线。PHP 序列化值与相关公开扩展另有对应范围。
