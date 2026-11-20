# API-O01 OpenID Connect RP

2026-11-20，独立基线 `a5550c3a55a5f532d4e89e939eb079a9d4bc2d15`。本批提供独立可选 `openrust-oidc` 包、根 oidc feature、配置、三语正文与原生应用归属动作。受控供应商本机协议范围验收；真实第三方授权仍须各供应商 issuer、测试应用、客户端凭据、获准回调和账户。双应用浏览器 SSO／联邦退出属于 API-O02，继续开发，不提升其状态。

## 契约与复用

协议固定 OpenID Connect Core 1.0 incorporating errata set 2（2023-12-15）、Discovery 1.0；官方原文、URL及SHA保存在references.json和本批证据目录。旧版 `v0.x:vendor/openrust/framework/src/auth/oidc.rs` 已读取并保留 old-oidc.rs，按职责提取非对称算法白名单、唯一kid、JWK用途／算法、issuer／audience／azp／nonce及at_hash校验原则。旧SeaORM、AuthFlows、全局HTTP客户端及公开构造“已验证身份”的接口不符合当前契约，未复制。当前接入原生HTTP Client／应用容器／Session／缓存／Eloquent和Session Guard，VerifiedIdentity仅可由包内验证流程构造。

没有对应的Laravel OIDC RP包基线，不把Socialite的可选exp／宽松NumericDate语义带入本包。公开OidcManager、facades::Oidc::provider返回请求借用Provider，异步redirect／user；应用注册固定Web路由。包发现注册服务，默认不开放登录端点，不把Passport称为OIDC OP。

## 实现

- Discovery每次重新校验精确issuer、code／subject／签名算法／S256／客户端认证支持；端点要求HTTPS并默认同issuer origin，额外来源必须列入allowed_origins。显式本地测试开关只允许字面IPv4／IPv6 loopback HTTP。拒绝URL凭据、fragment及授权端点预含保留参数。所有HTTP禁redirect，单请求超时与1MiB响应上限，敏感payload观察记录使用现有redactPayload。
- 使用AWS-LC安全随机state／nonce各32字节，64字节verifier和S256。Session保存带配置指纹、时间与max_age的尝试，回调先消费并校验。原子Cache.add在token HTTP前认领已用state，覆盖同Session并发副本；失败同样消费，需要新建授权重试。跨进程保证依赖共享原子数据库／Redis缓存，array仅单进程。
- 授权码交换支持form编码后Basic、Post、公共客户端None，绑定redirect与PKCE；校验Bearer／非空access及ID Token，显式返回零化凭据，未提供刷新grant API。每次回调重新读JWKS，支持同kid密钥轮换，无失败fallback。
- 使用固定jsonwebtoken11.1.0／AWS-LC进行成熟签名验证，现有依赖版本均未升级。配置仅允许RS256/384/512、PS256/384/512、ES256/384；检查唯一非空kid、JWK算法／用途／操作，拒绝HMAC、无签名及token自带密钥／URL。要求iss／sub／aud／exp／iat／nonce，subject遵循非空ASCII≤255字节；多aud要求azp，所有存在azp都匹配client。核对有效期、iat最大年龄、nbf／auth_time未来时间、请求max_age、at_hash与c_hash算法半摘要。签名只解码验证一次，再从已验证JSON反序列化类型。
- UserInfo必须等于已验证subject；新邮箱仅接受本次UserInfo自身的显式布尔验证标记。身份／凭据没有默认敏感Debug／Serialize，错误Display不输出供应商私有响应。
- 应用示例按二进制(issuer,subject)查询已有归属，用原生SessionGuard::loginUsingId登录并轮换Session／CSRF；不按邮箱合并、不新建账户、不存远端token。三语正文逐字包含实际编译执行的动作源码。

## 验收

Rust1.94.0：OIDC 4个原生函数＋2个真实HTTP／MySQL函数，以及根纯配置1函数。签名测试覆盖全部8个显式算法及相应SHA半摘要、同kid轮换、缺声明／错误签名／issuer／aud／azp／nonce／日期、key元数据／重复kid／token自带URL、HMAC混淆。受控OP真实接收authorize、单次code、Basic/Post/None、redirect、PKCE并签发JWT；RP使用真实TCP→Kernel→Web中间件→Session→facade。

MySQL8.4.13通过统一 `python3 -B tests/mysql_acceptance.py -- ...` 实际执行。两个RP共享数据库Session和原子缓存，test-only回调barrier让两个副本均已加载同一Session，只有一条token调用、一个200与一个400。成功登录按issuer＋007归属用户101，虽然UserInfo邮箱属于用户102也不合并；登录Session迁移，后继跨副本恢复101，登录前Cookie失效。不同不透明subject 7／0007即使邮箱验证也拒绝403，用户数不增加。其他HTTP测试覆盖跨应用Cookie、过期／错误配置指纹／非法verifier、授权拒绝、消费state、Discovery issuer／origin／参数冲突／S256、token／JWKS／UserInfo错误与subject不匹配。所有失败不输出私有响应。

最低Rust全工作区全目标严格Clippy（features pail,passport,fortify,socialite,oidc）与fmt通过；配置环境变量／默认值，feature根main构建，真实根HTTP／原始./artisan inspire／Passport帮助／SIGINT端口释放通过。根生成的包发现清单包含OidcServiceProvider。三语站点构建通过，通用浏览器12项＋OIDC正文6项覆盖`/`与`/docs/`，7个h2与公开方法／配置／动作检查通过。文档大chunk警告为现有构建提示。

隔离基线包含已提交Fortify安装，排除同期Passkeys及Fortify整合WIP。本批不声称完成所有并行模块的最终整合。Cargo.lock只新增本包块及根依赖边，没有升级既有依赖；复制时只施加OIDC增量，保留协作者Passkeys的Cargo变化。

## 证据与收尾

证据：`/private/tmp/openrust-oidc-20261120/`。保留references.json／官方原文／old-oidc.rs、accepted-overlay.tgz、SHA、配置／测试／Clippy／fmt／根入口／文档日志。初次借用／测试签名编码／UserInfo夹具不一致及Clippy写法错误日志保留，修正后复验通过；没有以缺少MySQL跳过的输出冒充实际验收。测试DER密钥由已有Socialite合成测试密钥转换，仅用于本地受控OP；不含真实凭据。

全部进程与本批容器结束后，删除本批source／target／root-main及中间可重建密钥转换文件；保留源码包、记录、共享缓存／镜像和全部数据卷。实测空间与卷核对见cleanup.json。本地核范围普通提交，不推送。

收尾差异检查仅移除三语正文末尾的多余空行，无渲染变化；overlay与SHA已同步。
