# 开发者文档边界整理：docs/en/packages/oidc.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/packages/oidc.md (迁移前)
+++ docs/en/packages/oidc.md (开发者正文)
@@ -117,7 +117,7 @@

 ## Browser SSO and logout {#sso-logout}

-API-O02 verifies two independent RP applications with different client IDs, application keys, database schemas and local users against one controlled OP. Sign in at the OP once; the second RP can authorize through the existing OP browser Session. Each RP still owns its local Session and resolves `(issuer, subject)` separately. `prompt("login")` asks the OP to authenticate again; `maxAge` additionally checks the returned `auth_time`.
+Configure each RP with its own client ID, application key, database and local user bindings. Sign in at the OP once; the second RP can authorize through the existing OP browser Session. Each RP still owns its local Session and resolves `(issuer, subject)` separately. `prompt("login")` asks the OP to authenticate again; `maxAge` additionally checks the returned `auth_time`.

 RP-initiated logout follows OpenID Connect RP-Initiated Logout 1.0 (2022-09-12). Set `OIDC_POST_LOGOUT_REDIRECT_URI` (`post_logout_redirect`) to a fixed callback registered at the OP. After `user()` and successful local account binding, call `Oidc::provider("work", request)?.rememberLogout(&authentication)?`. This explicitly saves only the verified ID Token in the server-side Session. `LogoutHint` is opaque; changing the public token fields cannot replace this verified hint. Treat all Session storage and outbound logout URLs as sensitive.

@@ -169,9 +169,9 @@

 In the fixed return handler, call `Oidc::provider("work", request)?.completeLogout().await?` and return a local signed-out page. Completion checks Session binding, configuration, destination, expiry and a shared atomic replay claim. A failed return consumes its attempt; it never logs anyone in. It acknowledges the return state and does not prove that every remote credential was revoked.

-Ending the OP browser Session leaves the other RP's existing local Session valid. Existing access and refresh tokens follow the OP's separate revocation policy. Front-channel/back-channel logout notifications and universal single logout are outside this implementation. Browser acceptance covers concurrent authorization, forced authentication, issuer/binding failures, CSRF, local/OP logout, concurrent/expired/replayed return state, missing OP support and bearer-token boundaries. The fixture is a controlled test OP, not a production identity provider.
+Ending the OP browser Session leaves the other RP's existing local Session valid. Existing access and refresh tokens follow the OP's separate revocation policy. Front-channel/back-channel logout notifications and universal single logout are outside this implementation.


 ## Testing and deployment {#testing}

-The controlled-provider tests use actual TCP HTTP, RSA signatures, same-kid rotation, Session middleware and MySQL 8.4. Two RP replicas load the same callback Session before claiming the state in the shared atomic database cache; only one calls the token endpoint. Tests also cover PKCE/client authentication, failed claims/Discovery/UserInfo, expired or replaced attempts, cross-app cookies, local binding and email collision without merging. Real third-party authorization requires that provider’s issuer, registered test application, client credentials, approved callback and test account. API-O02 browser results and logout boundaries are described above.
+Register the application and callback with your identity provider and configure its issuer and client credentials. Use an account authorized for that application. For multiple RP replicas, share Session storage and an atomic database or Redis cache to prevent callback replay.
````
