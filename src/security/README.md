# Application data encryption

Enable the `encryption` feature and register `EncryptionPlugin` to use the in-process
`Encryptor` service. Configure `[security.encryption]` with an `active_key_id` and a `keys`
map containing Base64-encoded 32-byte keys. `openrust key generate` prints a new key to stdout;
store it in a secret manager or an ignored local `.env` file.

`encrypt` and `decrypt` operate on byte slices and use a versioned authenticated envelope.
`encrypt_with_context` binds caller-provided trusted context to the ciphertext without storing
that context in the envelope. Plaintext is limited to 1 MiB and context to 1 KiB.

Only the active key encrypts. Historical keys remain available for decryption until applications
have migrated all data and verified recoverable backups. Rotation is a configuration change and
rolling restart; OpenRust does not rewrite application data automatically. Keep all keys needed by
all running instances available during a rolling deployment.
