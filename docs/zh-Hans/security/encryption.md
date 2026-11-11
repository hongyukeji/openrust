# 加密

## 简介

应用加密支持 AES-128 和 AES-256 的 CBC 与 GCM 模式，默认使用 AES-256-CBC。CBC 用 HMAC-SHA256 认证载荷，GCM 使用认证标签。

## 配置

在环境文件中设置 `APP_KEY`。Artisan 按应用配置的密码算法生成密钥，并替换所选环境文件中已有且未加引号的 `APP_KEY=` 赋值。生产环境替换非空密钥时需要确认或传入 `--force`。`--show` 会输出新密钥供安全转存，同时保留原文件。

```sh
./artisan key:generate
```

### 平滑轮换加密密钥

将旧密钥保留在以逗号分隔的 `APP_PREVIOUS_KEYS` 中。加密始终使用当前密钥，解密先尝试当前密钥，再按配置顺序尝试旧密钥。使用新配置重启应用进程，并将旧密钥保留到相关数据无需再解密为止。

```ini
APP_KEY=base64:<current-key>
APP_PREVIOUS_KEYS=base64:<previous-key>
```

## 使用加密器

在请求或命令作用域中使用 `Crypt` 门面。`decryptString` 返回字节，载荷无效或认证失败时返回错误。可用 `encrypt(value, true)` 和 `decrypt(payload, true)` 与 PHP 的序列化字节字符串互操作。

```rust
use openrust::{
    encryption::{DecryptException, EncryptException},
    support::facades::Crypt,
};

pub fn encrypt_token(token: &str) -> Result<String, EncryptException> {
    Crypt::encryptString(token)
}

pub fn decrypt_token(payload: &str) -> Result<Vec<u8>, DecryptException> {
    Crypt::decryptString(payload)
}
```
