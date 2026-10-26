# Signed links

Enable the `signing` feature and register `SignedLinksPlugin` with `[security.signed_links]`. Signing keys use a dedicated key ring, separate from JWT and application-encryption keys. New links use the active key; retained keys verify links created before rotation.

```rust,ignore
let links = SignedLinks::new(SignedLinksConfig::new(
    "https://app.example",
    "links-2026-01",
    key_ring,
))?;
let link = links.issue("email.verify", "account:42", 900)?;
let claims = links.verify(link.expose(), "email.verify").await?;
```

Tokens bind issuer, purpose, subject, issue and expiry times, key ID, and a random token ID. Verification requires the expected purpose and enforces the configured maximum lifetime. Keep subjects non-secret because the signed token is readable by its holder. `SignedLink` redacts its bearer value from `Debug`.

Links are stateless by default and remain valid until expiry or signing-key retirement. To revoke one link across application instances, add `with_database_revocations(db)` and include `SignedLinkRevocationMigration` in the application's existing migrator. The shared table stores only a SHA-256 digest of the token ID. Custom shared revocation stores can implement `SignedLinkRevocationStore`; local in-memory revocation is not provided as a production default.

Revocation storage failures fail verification closed. The application owns URL construction, delivery, authorization after verification, and its response behavior. Existing `VerificationCodes` retain their independent atomic one-time-consumption contract.
