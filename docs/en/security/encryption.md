# Encryption

## Introduction

The application encrypter supports AES-128 and AES-256 with CBC or GCM. The default cipher is AES-256-CBC. CBC authenticates the payload with HMAC-SHA256. GCM uses its authentication tag.

## Configuration

Set `APP_KEY` in the environment file. Generate a key from the application’s configured cipher with Artisan. The command replaces the existing unquoted `APP_KEY=` assignment in the selected file. In production, replacing a nonempty key requires confirmation or `--force`. The `--show` option prints a new key for secure transfer without changing the file.

```sh
./artisan key:generate
```

### Gracefully Rotating Encryption Keys

Keep the previous keys in the comma-separated `APP_PREVIOUS_KEYS` value. Encryptions always use the current key. Decryption checks the current key followed by the previous keys in their configured order. Restart application processes with the new configuration, and retain a previous key for as long as the application must decrypt data written with it.

```ini
APP_KEY=base64:<current-key>
APP_PREVIOUS_KEYS=base64:<previous-key>
```

## Using the Encrypter

Use the `Crypt` facade within a request or command. `decryptString` returns bytes and reports invalid or unauthenticated payloads as errors. Use `encrypt(value, true)` and `decrypt(payload, true)` for interoperability with PHP’s serialized byte strings.

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
