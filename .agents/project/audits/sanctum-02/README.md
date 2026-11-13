# Sanctum-02 令牌与 SPA 认证

## 令牌签发与存储

对照固定 Sanctum v4.4.0 的 `HasApiTokens`、`PersonalAccessToken` 与 `HasApiTokensTest`。复用 `v0.x` 的安全随机数、SHA-256 摘要及作用域撤销思路，使用当前连接的 Query Builder。Rust 将数据库连接、归属类型和整数 ID 显式传给 `PersonalAccessTokens`。签发凭据由可清零的字符串持有，数据库只保存摘要。凭据由前缀、40 位随机字母数字和 CRC32b 组成，返回时附加 `id|`。固定 Goravel 用作应用连接装配参考。 [HasApiTokens](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/HasApiTokens.php) · [PersonalAccessToken](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/PersonalAccessToken.php) · [Goravel 数据库 Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)

SQLite 和独立 MySQL 8.4.13 用实际 Artisan 迁移后的表验证并发签发、凭据格式、按 ID 和直接摘要查找、错误凭据、能力与过期判断、归属隔离和重复撤销。三语示例编译。下一批接 HTTP 守卫的用户解析、最后使用时间及过期拒绝，再验证 SPA 的 Session、Cookie 和 CSRF。 [Laravel 签发 API 令牌](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/sanctum.md#issuing-api-tokens)

Rust 1.94.0 格式、启用 `sanctum` 的 618 项工作区测试、全目标严格 Clippy、三语隔离构建和 18 项浏览器检查通过。8 项进程夹具由对应集成测试启动。
