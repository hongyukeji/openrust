# 微信双入口与 QQ 互联

2026-11-21，用户要求接入微信及QQ，并明确微信网站扫码与公众号网页授权均需要。延续已有 Socialite 驱动、Session state 与应用负责本地账户绑定的职责，不添加数据库表、自动注册或按昵称／邮箱／UnionID自动合并账户。

## 来源与方案

- 继续以 Laravel Socialite5.33.0固定提交 `8032bb1fa9d74aae54438f216e4d45db90d27ea9` 的共享授权流程为基线。
- 实际核对 MIT 的 SocialiteProviders 驱动源码与注册名称：[Weixin](https://github.com/SocialiteProviders/WeChat/blob/e61ce5d0f0ebf1c1ff57c6645815985203538908/Provider.php)、[WeixinWeb](https://github.com/SocialiteProviders/WeChat-Web/blob/8b19355caf083bc092c7b25fe1189ae0744dbfbf/Provider.php)、[QQ](https://github.com/SocialiteProviders/QQ/blob/b2b3652ef74f49d6eb0ba9804cfe118511a6c467/Provider.php)。前两包已更名为 socialiteproviders/weixin、weixinweb；保留正式 driver 名 `weixin`／`weixinweb`／`qq`，而不是按旧仓库名自造别名。三份 composer.json 均声明 MIT；没有增加PHP或Rust生产依赖。
- 实现前复核已有 AbstractProvider／ProviderDefinition、现有供应商测试与旧版 auth/oidc.rs。v0.x没有当前Socialite或这三种驱动可直接复用，旧SeaORM身份接口不替代现有Eloquent／Session归属机制。

## 实现与必要适配

- 新增 WeixinProvider、WeixinWebProvider、QqProvider，manager内注册，使用 services.weixin／weixinweb／qq。各自使用 WEIXIN／WEIXINWEB／QQ 的 CLIENT_ID、CLIENT_SECRET、REDIRECT_URI三环境变量，根配置实际读取已验证。
- 微信授权参数为 appid，分别使用公众号 oauth2/authorize＋snsapi_userinfo、网站 qrconnect＋snsapi_login，保留 wechat_redirect fragment。GET授权码交换使用 appid／secret／code／grant_type；刷新使用专用刷新端点；公众号 snsapi_base只返回OpenID。
- QQ使用get_user_info授权范围，GET令牌请求携带fmt=json，先GET me取OpenID，再GET用户资料。withUnionId(true)请求额外UnionID，结果放入标准User.attributes；昵称／头像映射按固定PHP驱动，原始profile保留，不伪造邮箱。
- 上游可变provider把OpenID／UnionID保存在实例字段。Rust驱动定义由应用共享，因此微信把OpenID存入请求所属ProviderOptions；QQ把me结果与profile在单次响应内部传递，map后User.raw仍是原始profile。没有跨请求共享可变身份状态。直接微信userFromToken须先setOpenId。
- ProviderDefinition增加默认可选的令牌交换／刷新与响应上下文钩子；现有POST／Basic／PKCE／state路径保留原行为。这是对PHP覆写getAccessTokenResponse机制的Rust适配，不另建授权管理器。
- HTTP 200中的errcode／ret／error同样失败；微信资料OpenID须匹配令牌OpenID，QQ me.client_id须匹配本应用，缺失或空身份拒绝。错误消息不回显供应商载荷；GET中的凭据同样通过redactPayload遮蔽记录，禁止重定向携带凭据。
- 三语Socialite页补入口、配置、静默授权、直接令牌、UnionID、本地绑定及真实供应商前置；保留原有章节结构。登录回调与账户注册／绑定由应用按既有示例接入，不自动安装产品路由或UI。

## 验收

隔离源码／独占target及日志：`/private/tmp/openrust-socialite-china-20261121/`。最初基线 `0e0c81a832034c96ff8bd77f9b85fcd71c7e2d65`；最终集成已提交的settings、PG绑定与统一短信入口，基线 `bb9357b7aebb48dd0a1562c42cda0cae21a543e5`。未覆盖协作者尚未提交的时间依赖迁移。Rust1.94.0，CARGO_INCREMENTAL=0、CARGO_BUILD_JOBS=3、dev/test debug=0；所有Cargo命令在快照运行。

- 新china目标7函数通过：默认端点／授权字段、真实TCP令牌交换与刷新／映射、公众号静默授权和显式OpenID、供应商错误与身份不一致、共享驱动并发身份隔离、真实Kernel加密Cookie的state消费／重放／跨应用拒绝，以及固定PHP对照。
- 使用现有PHP8.5.12固定镜像，禁网、只读挂载固定Laravel及三份生态驱动源码。Manager与HTTP宿主只作适配，实际调用源驱动的方法，比较授权字段和全部User映射；GET参数由真实Rust远端逐项断言。源文件哈希在require前核对，重复结果逐字相同，篡改参考源码被拒绝。脚本／fixture置于socialite测试目录；不是把Rust期望复制成PHP算法。见php-oracle.json、php-repeat.json、php-negative.log。
- MySQL8.4.13隔离夹具运行完整socialite包，42函数全部通过，其中已有本地账户归属测试实际建库执行，非缺环境跳过。新增协议本身不执行SQL；没有将此称为微信／QQ真实账号验收或三库新归属测试。日志socialite-integrated.log。
- 根原有配置与三组新配置2函数通过；根feature二进制Artisan inspire通过。日志config-integrated.log、root-smoke.log。
- workspace／all-targets／all-features／locked严格Clippy（-D warnings）及fmt通过，见clippy-delivery.log、fmt-final3.log。早期编译借用／测试API与格式问题均已修复，失败日志保留。第三方imgalifier-avif原有cfg警告不属于本批源码警告，未放宽workspace严格检查。
- 三语内容检查／站点构建、根及/docs两挂载三语桌面／移动12组合、Socialite专项6页通过。见docs-content-integrated.log、docs-build-integrated.log、docs-browser-integrated.log、docs-socialite-integrated.log。

## 限制与交付

未拥有真实微信开放平台／公众号／QQ互联应用凭据、登记回调及测试账号，实际供应商授权和手机扫码未验。调用应用须分别配置凭据与对应域名后完成联调；微信／QQ OpenID按各应用namespace绑定，UnionID不自动授权账户合并。没有改开发数据库或缓存，未重跑无关三库／三缓存全行为及完整发布平台矩阵。

17个实现／测试／正文任务文件最终与受验源码一致；仅暂存本批与本审计／current自己的交接段，保留并行时间库等改动。本地提交，不推送。产物清理实测在下方补记。

清理前后Available为37,796,140／43,238,496 KiB，净增加5,442,356 KiB；删除独占target与快照站点dist／cache，仅解除共享node_modules符号链接。临时MySQL与PHP容器确认无残留；源码、日志、共享依赖、开发数据库及全部卷保留。清理后约44.28 GB（41.24 GiB），见cleanup.json。
