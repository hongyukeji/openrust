# API-S01：X／Twitter OAuth2

2026-11-20：本批骨架、实现与本机验收完成，API-S01 整体仍未闭合。

## 固定参考与实现

沿用 Socialite v5.33.0 / 8032bb1fa9d74aae54438f216e4d45db90d27ea9；九份真实 PHP 源码 SHA 由 tests/reference/x-source-sha256.json 锁定。旧版无 Socialite，不复制旧 OIDC。隔离基线 04c7084d5，本批不修改依赖。保留同期验证器与 plan WIP。

新增 two::TwitterProvider／XProvider。X 复用 Twitter 的协议逻辑，替换三个默认端点。默认 scopes 为 users.read users.email tweet.read；默认 PKCE 96位 verifier／S256，授权查询 RFC3986。stateless 仍需 PKCE Session，授权字段最后写入字面 state=state，回调不检查 state。代码交换 Basic＋表单 client_secret；刷新 Basic＋grant_type／refresh_token／client_id，不发送表单 client_secret。profile query user.fields=profile_image_url,confirmed_email，映射 data 中的确认邮箱。

manager 支持 x、twitter-oauth-2 和 oauth 严格整数2的 twitter；沿固定 PHP 缺失／null 配置回退，已有不完整对象不回退。根 X_*／TWITTER_* 接线，TWITTER_OAUTH 以数字默认1读取，环境2能真正选择OAuth2；默认OAuth1尚未交付，明确失败，不偷偷切到OAuth2。共享 ProviderDefinition 增加默认PKCE／查询编码／刷新secret控制；授权字段定制增加stateless参数，Slack回归保持通过。请求借用期／Zeroizing凭据适配沿用首批。

## 实际验收

证据 /private/tmp/openrust-socialite-x-20261120/。Rust1.94.0，全包18个不同函数（原13＋新增5）和根纯配置1项通过；文档integration target实际编译。新增真实HTTP与双应用加密Session Cookie覆盖默认PKCE、40位state、96位verifier与S256实际匹配、跨应用拒绝、消费后重放失败、Basic交换／刷新、scope分割及确认邮箱映射。stateless字面state、参数覆写优先级与空格／加号／波浪号／星号RFC3986编码逐项匹配PHP；无Session时redirect／交换失败，userFromToken可用。远端拒绝／无token／错误JSON／错误profile／撤销401均拒绝且不回显供应商私密body。

实际运行固定PHP8.5.12镜像 php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9，禁网只读挂载真实源码。Probe只暴露protected方法，映射／默认及stateless字段／参数／编码／token与refresh HTTP选项由原PHP执行。重复逐字一致，篡改TwitterProvider的负样本在require前被SHA拒绝。没有复制驱动算法作为oracle。

严格workspace／all-targets features pail,passport,socialite Clippy、fmt通过。根纯配置验证TWITTER_OAUTH整数1／2与X/Twitter环境凭据。三语8章节构建及12＋6浏览器通过。最终feature根二进制在MySQL8.4.13 wrapper下原始./artisan、帮助、真实HTTP默认路由／错误响应、SIGINT与端口释放通过；Socialite本批不执行SQL，不声称MySQL本地身份映射。

初轮测试使用错误的Repository构造及Response方法已修；strict Clippy发现测试set配置返回值未检查，改为unwrap后最终全包及严格检查重验通过。失败日志保留。

## 剩余与收尾

继续Facebook／Google、OAuth1、真实MySQL本地归属和独立API-O01／02。真实X/Twitter授权仍缺测试应用凭据、登记回调域名和账户，受控HTTP不等于实机授权。三语正文明确已交付与未交付边界，不称完整OIDC或SSO。

输入SHA与隔离源码一致，保留accepted-overlay.tgz、PHP源与成功／失败日志；按归属清理闲置source／target／root-main，全部卷、镜像和共享缓存保留，见x-cleanup.json。本地核范围提交，不推送，继续开发。
