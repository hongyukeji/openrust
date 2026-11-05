# Fortify 认证对应契约

## MFA、2FA 与用户选择

多因素认证（MFA）是总体安全能力名称；双因素认证（2FA）是其中使用两类因素的形式，TOTP 是验证码验证技术。本批对应 Laravel Fortify 的 Two-Factor Authentication，支持密码与 TOTP／恢复流程，不据此宣称支持其他尚未实现的认证因子；官方 API、配置及来源名称保持不变。

用户按需绑定和开启。当前应用 `confirm=true`：未绑定可正常密码登录，待确认不当作已开启，验证确认后登录执行双因素挑战；完成必要身份确认后可关闭。应用开放绑定功能不等于所有账户已开启。其他应用显式选用 `confirm=false` 时仍遵循固定官方语义，不将本应用策略强改为框架默认。

删除旧 MFA 仅指旧 OpenRust 独立实现及旧数据兼容，保留 MFA 概念、TOTP 与正常安全能力；不恢复 `auth-mfa` feature 承载术语。


## 当前 Fortify 单体系与显式 JWT API profile（最终生效契约）

最新真人决定：框架、根默认应用与未显式选择JWT的生成路径，以对应固定Laravel官方默认、生命周期和公开机制为准；Fortify原生Web/Session是其对应模式默认，不将所有场景一律Session或一律JWT。API+Bearer JWT是本批完整必交的显式选配，开发者选择后全程无Session/Cookie/CSRF-cookie/remember/隐藏Session/Set-Cookie或Cookie身份兜底。两种传输共用唯一UserProvider/Guard/Fortify三字段/Actions；旧auth-mfa兼容仍删除。此决定覆盖此前JWT为根默认、一律禁中心Cookie等中间口径，不改写历史验收。

**本批必须实际交付 JWT access/refresh API 与无 Cookie 管理闭环。** 显式 profile 只区分开发者选择及来源分类，不降低交付优先级、不推给应用自行实现；Fortify不内置此流程，按Laravel Guard/UserProvider/Actions公开机制扩展。Laravel官方生态已有Passport OAuth2与Sanctum，不能笼统称Laravel不支持Token，也不以本API代替这些包的全量对应。

真人已确认：旧 OpenRust MFA 代码和数据均不兼容；不建设旧数据迁移、重绑、接管或长期fallback。共享数据库、其他WIP与原始证据不删除。原Goal/main/20%保留与credits=false不变。

1. **框架官方基线**：固定Fortify v1.41.0 / `c456642584c102f6a6aafee9932299542da637b5` 的StatefulGuard+Session、三字段、Actions/Provider、login.id/login.remember及八份加密可重读恢复码原样对应；JSON不等于无Session。Sanctum Bearer PAT与SPA Cookie模式分开，Passport是OAuth2；自有JWT不冒称二者已完成。
2. **用户选中的应用profile**：根应用显式选择JWT及生成JWT API profile后全程Bearer，无Session middleware、临时Web Session、Cookie/CSRF-cookie/remember-cookie及隐式Set-Cookie。框架默认与其他生成模式不被该profile覆盖；不得擅自把JWT换成PAT。
3. **唯一二因素**：API密码验证后，仅在需要时返回短时/限次/一次消费且绑定原始ID、Guard、Provider与用途的challenge；通过同一Fortify状态/算法/Actions验证TOTP或恢复码后才颁发access/refresh。DB/缓存挑战不等于Web Session；此为明确API扩展，不是官方默认或Rust被迫适配，不复活另一MfaService/状态体系。
4. **管理与跨入口安全**：Bearer管理经受控近期密码/必要二次证明；复用同一Actions。JWT/PAT/refresh、Session/remember、OIDC/attach与资源逐项防绕过；必要版本/nonce/事务/取消/审计/限流能力按实际不变量经官方扩展保留，不按authority/proof字样一刀切。
5. **实际删除**：auth-mfa feature、MfaService、旧独立管理/挑战/表定义、旧生成profile、legacy hash/seed/adoption/floor/fallback及专用marker；不换名隐藏旧机制。旧攻击/并发/回滚/审计/期限/隔离测试迁到唯一新机制；非官方旧协议本身不再是兼容义务。
6. **有限退出**：源码/所有调用方/CLI与显式profile/唯一示例/文档收齐→冻结→main/default/all-feature/严格文档/MSRV/适用consumer、SQLite当前/3.26及受影响MySQL/PG、实际new/check/test、API完全无Cookie流程与官方Web分别实测、独立安全复审→脚本限定本地提交。Google真实客户端仍外部未验，不push/tag/release。

owner=root统一contract、Cargo/lock、迁移、共享文档、索引、构建和服务；worker按互斥文件交接后编码。当前为未提交WIP；删除前558文件归档及真人授权回执在`tmp/fortify-single-system-20261105/`，旧候选与失败/清理事实在`tmp/fortify-totp-alignment-20261104/`，不作为新体系已验。唯一停点：完成删除/现代化内核与显式API profile接线，不继续旧兼容矩阵。

## 显式 JWT API 管理适配 Spec

- `FortifyJwtApiServiceProvider` 将复用同一 Fortify 服务注册内核，只挂显式 API routes；官方 `FortifyServiceProvider` 继续原 Web 默认，两者的路由选择不通过改全局默认实现。
- API 第一因素复用配置 Guard/UserProvider/AuthenticateUsing 的账户查询与密码验证，不能将现有 email-only 快捷入口冒充完整配置契约。
- 登录挑战绑定原始账户 ID、Guard、Provider、来源、用途与当前因子代际；只有真实二因素完成才颁发 access/refresh，不将 challenge 当资源凭据。
- 管理准备入口先验证真实当前 Bearer/存活凭据家族及近期密码，再签发短期、用途受限、绑定该家族的管理授权；必要因子证明必须来自同一 Fortify 状态。
- 管理授权只允许绑定/确认/读取配置材料/恢复码轮换/禁用，不允许资源访问、JWT/refresh 签发或跨用户、跨 Guard/Provider 使用。
- 因子变更使旧资源凭据立即失效，但同一受控管理授权可在短期限内继续完成 pending→confirm；不能为了避免自锁而放宽资源认证的代际检查。
- 管理动作在同一账户锁下重核存活家族、授权期限/用途/身份、当前因子代际，并复用 TwoFactorActions 与 authority 原子变更；取消、审计失败或晚到验证都不能残留有效授权。
- 管理grant使用专用认证加密域，绑定issuer/audience、Guard/Provider、存活家族、密码摘要、随机nonce和至多300秒且不超过父凭据的期限；不复制因子secret/recovery/confirmed状态，不作为资源凭据。Pending只允许原grant精确continuation，失去continuation时通过当前Bearer+近期密码明确重新开始Pending绑定，生成全新材料，不泄露原绑定秘密、不能重置Modern状态。
- 根应用和生成 profile 使用同一公开适配，显式不挂 Cookie/Session/CSRF-cookie/remember；默认 Web 流程仍另测。
- 验收含 pending 管理连续性、另一设备/已撤销家族/旧代际/过期/用途混用/并发/取消/失败回滚，并在真实 HTTP 逐响应断言无 Set-Cookie。

公开adapter、显式生成profile及根应用选择器已编码；最小库与根server类型检查曾在分输入通过，尚未本批集中行为/真实服务验收。独审发现的管理隔离、密码重哈希、密码挑战撤销、签发域/重放副作用及事务末位校验须修复并复审，不能据源码与格式检查报告无Cookie API已交付。

## 历史证据

旧双轨Spec已撤销并归档于`tmp/fortify-single-system-20261105/superseded-fortify-contract.md`；不是当前保留义务。有效通用安全回归按上述新contract实际迁移验收。
