# 应用数据加密

`EncryptionServiceProvider` 在统一容器中登记加密服务，首次解析时读取生效配置并校验密钥。

容器中的 `dyn ApplicationEncrypter` 默认由 `ConfiguredEncrypter` 实现，对应 Laravel 的 `encryptString` 与 `decryptString`。它读取同一应用的
`app.cipher`、`app.key` 和 `app.previous_keys`，支持 AES-128/256 CBC 与 GCM。
默认 cipher 为 AES-256-CBC。用 `openrust key:generate` 生成应用密钥；配置中的
`base64:` 前缀只在加密服务读密钥时解析。新密文使用当前密钥，历史密钥仅用于解密。
明文上限为 1 MiB，载荷输入上限为 2 MiB。

`encrypt_serialized_string` 和 `decrypt_serialized_string` 对应 Laravel 加密 PHP 字符串值的路径，
例如二次验证密钥和恢复码 JSON 字符串。长度使用字节数，密文认证通过后才检查长度与帧边界；
解出的缓冲在释放时清零。此入口的类型为原始字节，PHP 对象及通用混合值另属值契约。

实现 `ApplicationEncrypter` 时，原始字节方法提供载荷加解密，默认值方法复用 PHP 字符串帧。
`current_key`、`previous_keys` 和 `all_keys` 返回借用视图，供名称认证和密钥轮换使用。
这些方法返回敏感材料，调用方应在当前操作中使用，避免写入日志、配置输出或序列化结果。

启用 `encryption` feature 并注册 `EncryptionServiceProvider`，即可使用进程内的 `Encrypter` 服务。在
`[security.encryption]` 中配置 `active_key_id`，并在 `keys` 映射中提供 Base64 编码的 32 字节密钥。
`openrust key generate` 会将新密钥输出到标准输出；请将其存入密钥管理工具或已忽略的本地
`.env` 文件。

`encrypt` 和 `decrypt` 处理字节切片，并使用带版本的认证密文封装。
`encrypt_with_context` 会将调用方提供的可信上下文绑定到密文，但不会把上下文保存在封装中。
明文上限为 1 MiB，上下文上限为 1 KiB。

只有当前 active key 用于加密。应用迁移完所有数据并确认备份可恢复前，必须保留历史密钥用于解密。
轮换通过修改配置并滚动重启完成；OpenRust 不会自动重写应用数据。滚动部署期间，所有运行实例所需的密钥都必须保持可用。

默认 Cookie 和 XSRF 头通过同一个应用绑定处理载荷，Fortify 用该绑定处理账户密钥及恢复码的 PHP 字符串值。
`Encrypter` 保留 AES-256-GCM-SIV/or1 上下文保护，用于显式选择的 Cookie 保护和 JWT 管理 grant 等场景。
