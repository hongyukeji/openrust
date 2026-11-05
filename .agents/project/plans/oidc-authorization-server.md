# OAuth2 授权服务器与 OIDC 单点登录

## 授权与前置

自建Authorization Server/OpenID Provider已纳入原完整Goal，当前Fortify单体系与完整JWT选配先闭环，不并入本批冻结输入。最新真人批准官方机制为默认、API+Bearer JWT为显式选配；中心默认可沿对应官方Web身份机制，开发者选择JWT中心模式才执行全程无Session/Cookie契约。此前一律禁止中心Cookie或JWT为默认的中间口径已覆盖。


## 协议 Spec

1. OAuth2 服务端固定正式 Laravel Passport release/tag/commit 后对照源码、测试、CLI、配置及模型扩展；现有 auth-oidc 是 RP 客户端，不能替代授权服务器。
2. OIDC 属明确授权标准扩展，对照 Core 1.0 errata2、Discovery 1.0 errata2、RP-Initiated Logout 1.0、Back-Channel Logout 1.0 errata1 与 RFC 9700；不声称 Passport 内置完整 OIDC。
3. 首簇 Authorization Code + PKCE S256：public/confidential client 管理、精确 redirect_uri、scope/consent、state/client 关联、短期单次 code、client authentication、access/refresh 与撤销；回调只传 code，不暗启 implicit/password grant。
4. 次簇 OIDC：明确 openid/profile/email 范围，issuer/sub/aud/azp/exp/iat/nonce/auth_time、max_age/prompt、独立用途 ID Token、Discovery、JWKS/kid 轮换及 UserInfo。
5. SSO 簇以两独立 RP 的真实浏览器 A→中心→回调、B 复用中心身份、跨节点与重启验证；登录与 consent 分离，新 client/scope 不自动授权。
6. 退出簇明确 RP/中心退出、Backchannel 签名/重放/幂等/失败重试以及会话、refresh、access 撤销；无对应机制不承诺离线 JWT 即时全局失效。
7. 复用唯一 Container/Guard/UserProvider/Fortify/Policy、配置、持久和审计，不复活旧 MFA，不另建用户体系。
8. 核心服务与 Provider/CLI 可选；中心必要登录/consent/对接页面放唯一 example 与公开视图扩展，不启动冻结业务后台或新源码仓库。
9. root 独占 contract、迁移号、manifest/lock、共享文档、索引及构建/服务；前置明确的官方对照、协议测试与独立审查按互斥文件并行。
10. 各簇源码/调用方/示例/文档冻结后执行适用 workspace/feature/CLI 生成、SQLite 当前/最低版、MySQL/PG、两 RP 浏览器互操作与不同模型安全复审，再限定本地提交；不 push/tag/release。

## 分期退出

唯一执行队列为 todo.md：Fortify 前置闭环 → Passport 对标与授权码簇 → OIDC Provider 簇 → 双 RP SSO 与退出簇。其他适用 Passport 能力保留全量范围并依官方与安全策略分类，不因首簇限定而删除。当前仅登记已授权需求；版本固定、源码审计与协议实现均未验收。

| 同一队列中的功能簇 | 前置与负责人 | 文件归属与冻结 | 有限退出 |
| --- | --- | --- | --- |
| L090 Passport 授权码/PKCE | Fortify当前批次提交；root收敛，worker按分配实现 | 固定正式上游commit及新功能簇源码快照；auth授权服务/公开Provider与CLI，root独占迁移/manifest | client、code、token与撤销/错误安全测试及真实生成/数据库通过后独立本地提交 |
| OIDC OP | 前簇实际交付；root | 同一授权内核扩展，冻结Core/Discovery等标准版本与受影响输入 | metadata、JWKS轮换、ID Token/UserInfo用途与nonce等互操作通过 |
| 双RP SSO | OP交付与中心API Token持有/授权事务设计；root集成 | 唯一example与公开视图扩展、两独立RP fixture；UI与协议owner互斥 | A/B真实浏览器、TOTP、consent、跨节点/重启及无Cookie业务API分别取证 |
| SSO退出与撤销 | SSO前簇；root集成、不同模型安全复审 | 同源会话/通知/撤销内核与协议测试，服务资源由root统一 | RP/中心/Backchannel签名、重放、失败重试和撤销边界实际验证并提交 |

各簇冻结必须登记当时Git基线、完整受验文件SHA、owner停写及适用证据；本表不预填尚不存在的commit或测试结果。前置未满足时允许互斥的官方来源/协议审查先行，不能以规划派发称已实现。

## 认证中心与业务 API 传输边界

最终真人批准来源：01a19800-86c8-7593-b6cd-0761b1f19595，建议turn 01a1a9f1-9f4b-73f1-a29c-ad7511bd08d3与批准turn 01a1a9f1-fabc-70f0-afc5-7d42f9c25c74。默认保持对应官方机制；自有JWT选配是必交扩展，不是Passport/OIDC已交付证明。官方Web中心保登录再生、CSRF、最小Cookie域/路径与生产安全属性；不跨业务域共享中心Cookie，不能将中心身份复用当作自动批准所有client或scope。

开发者选择JWT中心模式后，前端在自身origin持Token，以Bearer调用认证、必要TOTP/恢复、近期确认、授权同意、刷新和注销；全流程不靠Web Session/Cookie/Set-Cookie或Cookie身份兜底。先设计Token持有、续期、回访及单次授权事务绑定，验证刷新、新标签、重启、XSS与撤销边界。跨origin跳转不会自动携带Authorization，不能以URL搬运JWT/refresh、自创替代授权码协议或透传其他origin凭据，不能无依据承诺任意浏览器无感SSO。

OAuth授权码+S256 PKCE与OIDC标准角色不变，回调只传code及必要state，安全交换Token并核nonce/issuer/audience/purpose。OAuth access、OIDC ID Token、自有应用JWT的验证域和用途分开。DB/缓存可保存短期事务、单次码、挑战、恢复码消费、撤销及审计，不能把Session改名复活。默认Web与显式JWT中心及双RP分别验证身份、consent、退出与无Cookie边界；当前批次之后再实现AS/OP/SSO。
