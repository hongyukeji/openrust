# 开发者文档边界整理：docs/en/packages/fortify.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/en/packages/fortify.md (迁移前)
+++ docs/en/packages/fortify.md (开发者正文)
@@ -2,7 +2,7 @@

 ## Introduction

-Fortify supplies authentication routes and application extension contracts without shipping a user interface. This page covers the verified API-F01 authentication and API-F02 Passkeys lifecycle against Fortify 1.43.0.
+Fortify supplies authentication routes and application extension contracts without shipping a user interface. Its authentication API follows Fortify 1.43.0.

 ## Fortify and Sanctum

@@ -74,7 +74,7 @@

 ## Passkeys

-The API-F02 lifecycle uses Laravel Passkeys server 0.3.0 and the tested `@laravel/passkeys` 0.4.1 client. `fortify:install` publishes the `passkeys` table migration, adds `PasskeyAuthenticatable` to the default User, and registers `Passkeys::useUserModel::<User>()` in the generated provider. Keep `passkeys` in `fortify.features`, rebuild and migrate. Custom User models implement `openrust_fortify::passkeys::PasskeyAuthenticatable` and require explicit registration in their application provider.
+The Passkeys API follows Laravel Passkeys server 0.3.0 and works with the `@laravel/passkeys` 0.4.1 client. `fortify:install` publishes the `passkeys` table migration, adds `PasskeyAuthenticatable` to the default User, and registers `Passkeys::useUserModel::<User>()` in the generated provider. Keep `passkeys` in `fortify.features`, rebuild and migrate. Custom User models implement `openrust_fortify::passkeys::PasskeyAuthenticatable` and require explicit registration in their application provider.

 Set `fortify.passkeys.relying_party_id`, `allowed_origins`, `user_handle_secret` and `timeout`. Defaults derive the RP ID and exact origin from `APP_URL`, use `PASSKEYS_USER_HANDLE_SECRET` with `APP_KEY` fallback, and set a 60000 ms timeout. Production origins require HTTPS; HTTP localhost is supported for development. Origins include their port and exclude trailing slashes and paths. Keep the user handle secret stable when rotating the application key: handles are 64-byte hexadecimal HMAC-SHA256 values derived from the model table and key. Changing that secret changes handles for new registrations.

````
