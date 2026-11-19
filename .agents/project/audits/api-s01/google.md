# API-S01：Google OAuth2 与 JWT

2026-11-20：本批骨架、实现与本机验收完成，API-S01 整体仍未闭合。

## 固定参考与实现

沿用 Socialite v5.33.0 / 8032bb1fa9d74aae54438f216e4d45db90d27ea9。新增 Firebase PHP-JWT v7.2.2 / 99efeb17d6def8c0295777f2d45b07ff4fdb4e40 作真实上游依赖对照，属于 Socialite 声明的 ^7.0 范围；不声称这是原 composer.lock 的解析结果。Socialite及PHP-JWT源码 SHA 由 tests/reference/google-source-sha256.json 固定，PHP-JWT BSD-3-Clause。旧版无Socialite，旧auth/oidc.rs用jsonwebtoken等提供签名／声明校验线索，不搬旧公开RP／SeaORM。隔离基线 e6638ea47，未纳入同期验证器8498d530d及其后WIP。

新增two::GoogleProvider、manager与GOOGLE_CLIENT_ID／GOOGLE_CLIENT_SECRET／GOOGLE_REDIRECT_URI。默认openid profile email，普通Bearer UserInfo／prettyPrint=false、确认邮箱与头像、raw兼容id／verified_email／link、avatar_original。刷新仅在缺失refresh_token时保留原值，null保留None，轮换按响应。Option凭据沿用首批Rust分组。

长度>100且恰有两个句点的token走JWT，按固定端点读取JWKS，经jsonwebtoken11.1.0／AWS-LC验证算法与签名、kid与JWK alg对应，issuer严格https://accounts.google.com、aud必须与clientid相同的单一字符串；不接受数组aud，不使用header中的远端URL，不fallback至UserInfo。每次读取JWKS，实际支持同kid轮换。ProviderDefinition增加带ProviderConfig的可覆写profile入口和refresh响应归一化，默认兼容已有驱动；Google JWT必须经持有配置的AbstractProvider／manager入口校验clientid。

本批采用锁定jsonwebtoken11.1.0（MIT，MSRV1.88低于项目1.94），关闭默认PEM功能、使用已有AWS-LC。Cargo.lock只新增jsonwebtoken11.1.0、untrusted0.7.1、zeroize_derive1.5.0及相应依赖边，不更换既有包版本。成熟库负责JWT解析与签名；其默认必需exp、日期类型与固定Firebase存在差异，因此显式保留原PHP NumericDate规则：日期可缺失／null／有限数字字符串，nbf存在时覆盖iat回退，exp以当前整秒>=拒绝；issuer／aud逐项严格校验。非有限日期拒绝，拒绝远端HMAC JWK以防算法混淆。此兼容Socialite路径不要求exp／nonce、不做Discovery，不称完整OIDC RP；API-O01独立实现严格RP契约。

## 实际验收

证据 /private/tmp/openrust-socialite-google-20261120/。Rust1.94.0，全包23个不同函数（既有18＋Google5）及纯配置1项通过；文档integration target编译。固定PHP8.5.12镜像 php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9 禁网只读挂载真实Socialite／Firebase源码。Probe只暴露protected方法，Request／HTTP／Arr只作宿主；签名和JWK解析真实执行。重复逐字一致，篡改GoogleProvider源在require前被SHA拒绝。fixture PEM为本批生成的两把测试密钥，不是供应商凭据。

25组原PHP签名JWT正反例与Rust真实HTTP逐项一致：正常／无exp／数字字符串／null日期、nbf覆盖未来iat、错误issuer／aud／数组aud／缺必需声明、过期／未来nbf／未来iat、同kid轮换、错误签名、未知／缺kid、HMAC混淆、错误日期、key alg不匹配／缺alg、空JWKS及坏RSA参数。另验普通UserInfo／token表单／refresh保留、null与轮换、授权码交换直接JWT、跨client受众隔离、远端撤销401／错误JSON／JWKS失败及应用内注册隔离；无供应商私密body回显。JWT撤销不冒称在线内省，撤销测试为普通Bearer token。

严格workspace／all-targets features pail,passport,socialite Clippy及fmt通过。纯配置验证Google三环境变量；三语8章节构建、12通用＋6Socialite浏览器通过。最终feature根二进制在MySQL8.4.13 wrapper下原始./artisan、帮助、真实HTTP默认路由／错误响应及SIGINT／端口释放通过。本批Socialite不执行SQL，不声称完成MySQL本地身份归属。

## 剩余与收尾

继续Facebook、OAuth1、真实MySQL本地归属以及API-O01／02。真实Google授权仍需测试OAuth应用、clientid／secret、登记回调域名及账户，受控HTTP不等于供应商实机。

输入SHA与隔离源码一致，accepted-overlay.tgz、固定PHP-JWT源码包／源码、上游Socialite、成功／失败日志保留；核归属清理闲置source／target／root-main，镜像／共享缓存及全部卷保留，见google-cleanup.json。本地核范围提交，不推送，继续开发。
