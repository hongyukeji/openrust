# ADR-022 JWT 运行时与 claims contract

- 状态：Accepted（独立 contract review 完成；HS256 runtime 与 key ring 已实现；PostgreSQL 18 / MySQL 8.4 AuthFlows 和生成项目 HTTP 流程通过）
- 日期：2026-10-25
- 依据：[ADR-018](adr-018-jwt-primary-authentication.md)、[ADR-012](adr-012-auth-current-user-contract.md)

## 最小实现 Spec

- 复用 `jsonwebtoken` 11.1 的 `aws_lc_rs` backend；避免其 `rust_crypto` feature 引入受 RUSTSEC-2023-0071 影响的 `rsa 0.9`。v1 固定签名算法为 HS256，不从 token header 协商算法。
- `JwtConfig` 必须显式提供 issuer、audience、active key ID、可验证 key ring 和 access-token TTL；不提供可运行的硬编码密钥默认值。
- 每个签名密钥以 Base64 配置，解码后至少 32 个 CSPRNG 字节；`JwtKey`/`JwtConfig` 自定义脱敏 `Debug`，secret 不得出现在日志、错误或响应中，也不能直接嵌入派生 `Debug` 的全局 framework config。
- access claims 包含 `iss`、`aud`、正数 `i64` 账户 ID 的十进制字符串 `sub`、高熵随机 `jti`、`iat`、`nbf`、`exp`；使用 JWT NumericDate 秒。header 设置 `kid` 与严格匹配的私有 token type `openrust-at+jwt`，并声明本 contract 不声称符合 RFC 9068 OAuth access-token profile。PAT abilities 不混入 JWT claims。
- header 中的 `kid` 仅用于选取本地已配置密钥，算法必须严格为 HS256；缺失/未知 `kid`、错误 `typ`、伪造、issuer/audience 错误、尚未生效或过期均返回未认证，不降级匿名。
- `AuthSource::Jwt` 和 `AuthMode::Jwt` 与 Session/PAT 独立；`AuthMode::SessionOrToken` 中的 Bearer 仍明确表示 PAT，不根据 token 外形猜类型。JWT 模式不要求 `SessionPlugin`，验证成功后不逐请求查数据库。
- JWT access 在默认模式下禁用账户、密码重置或登出后最迟于 access 到期时失效；refresh token 应立即撤销。需立即撤销 access 时必须显式启用共享状态检查，不依赖节点内存。
- 轮换采用兼容 key ring：所有节点先加载新旧 key，再切换 active key；旧 key 保留至其可能签发的 token 最长有效期及时间容差结束。所有节点必须共享相同验证 key 集合；移除仍有效 token 的 key 会提前使 token 失效。
- 默认发行 TTL 为 15 分钟；配置发行 TTL 必须大于零且不超过 1 小时，独立 `max_valid_ttl` 不得小于发行 TTL 且不得超过 1 小时。显式设置 `jsonwebtoken::Validation.leeway = 0`、启用 `validate_nbf` 并声明必需标准 claims；手动校验 `iat`、`jti`、`typ`、NumericDate 秒、`exp > iat`、未来 `iat`（最多 30 秒时钟容差）及 `exp - iat <= max_valid_ttl`。30 秒容差只适用于 `iat`，不得延长 `exp` 或 `nbf` 的有效期。
- refresh credential 使用至少 32 bytes 的 CSPRNG 不透明随机值，数据库只存 SHA-256。已消费 token 的 hash/family 关联记录须保留至 family 最长有效期，以识别重放。轮换在数据库事务内条件消费旧记录并创建唯一子 token；并发败者按重放处理并撤销整个 family（包括并发成功产生的后继），但已签发 access token 仍按默认规则有效至过期。
- 验证覆盖算法混淆、错误/未知 `kid`、篡改、issuer/audience、时间边界、key rotation、refresh 并发与重放，并在 PostgreSQL/MySQL 运行集成测试。

## 取舍与边界

HS256 以成熟库实现，适用于框架同时签发和验证、各 API 节点共享受控密钥的默认部署。因任一验证节点持有签名能力，未来需要独立签发服务时再评估非对称算法；v1 不同时暴露多算法配置面。`openrust-at+jwt` 使用显式私有类型防止跨用途接受；不声明符合要求 `RS256` 等约束的 [RFC 9068](https://www.rfc-editor.org/rfc/rfc9068.html)。JWT access token 不承诺即时全局撤销；立即撤销必须另行定义共享 denylist/token-version 等机制。

`jsonwebtoken` 提供 JWT 编解码与验证机制；OpenRust 仍须完成配置验证、公开签发/验证入口、路由集成、错误语义、密钥轮换、双库 refresh store 与测试。refresh store 需使用数据库原子条件更新/事务防止重复消费。
