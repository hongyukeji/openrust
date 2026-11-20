# 开发者文档边界整理：docs/en/packages/passport.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/packages/passport.md (迁移前)
+++ docs/en/packages/passport.md (开发者正文)
@@ -195,7 +195,7 @@

 These are fixed example values; generate new values for each real authorization. Missing or incorrect verifiers, altered callbacks, expired codes and replayed codes fail without issuing tokens. The authorization step permits different ports for registered HTTP loopback callbacks; the token exchange binds the exact redirect string originally supplied.

-Rust stores a validated native request in Session and invokes a compiled view callback instead of PHP object serialization. Codes and refresh tokens use the existing application's authenticated encryption plus application context; their internal envelope is not PHP ciphertext compatible. Client/code locks and a single database transaction keep code consumption, access-token and refresh-token saves atomic. Custom token models participate through Eloquent casts, events and their actual connection; save cancellation or database failure rolls back issuance. The remaining grants and the combined API installer belong to subsequent units.
+Rust stores a validated native request in Session and invokes a compiled view callback instead of PHP object serialization. Codes and refresh tokens use the existing application's authenticated encryption plus application context; their internal envelope is not PHP ciphertext compatible. Client/code locks and a single database transaction keep code consumption, access-token and refresh-token saves atomic. Custom token models participate through Eloquent casts, events and their actual connection; save cancellation or database failure rolls back issuance.

 ## Device authorization grant {#device-authorization-grant}

````
