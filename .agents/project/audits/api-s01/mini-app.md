# 微信小程序登录

2026-11-21，接入微信小程序登录，并按用户要求重新核对 GitHub／Laravel 生态的现有方案。

## 来源与设计

- 网页微信／QQ 继续使用已验收的 Laravel Socialite 5.33.0 与 SocialiteProviders Weixin／WeixinWeb／QQ，固定提交及源码对照见 [Tencent 驱动记录](tencent.md)。不重建已有网页授权流程。
- [EasyWeChat](https://github.com/w7corp/easywechat/tree/ba8022698a613e37b10ed1e4db604c5a96af0a63)，MIT，固定提交 `ba8022698a613e37b10ed1e4db604c5a96af0a63`。调研时 GitHub API 显示约 1.04 万星；采用理由是其 MiniApp Utils 已有直接、成熟的 `codeToSession`，不是星数本身。实际读取 [Utils.php](https://github.com/w7corp/easywechat/blob/ba8022698a613e37b10ed1e4db604c5a96af0a63/src/MiniApp/Utils.php) 并执行其方法核对协议。
- [overtrue/laravel-wechat](https://github.com/overtrue/laravel-wechat/tree/133828b7950acceff9b11deb442b65a9cf203292)，MIT，固定提交 `133828b7950acceff9b11deb442b65a9cf203292`。实际读取 ServiceProvider 与 config/easywechat.php，确认 Laravel 生态以独立 MiniApp 服务及 app_id／secret 配置承接小程序，而非网页 OAuth 跳转。
- 核对微信官方[小程序登录](https://developers.weixin.qq.com/miniprogram/dev/framework/open-ability/login.html)与[code2Session](https://developers.weixin.qq.com/miniprogram/dev/server/API/user-login/api_code2session)：wx.login code 由服务端 GET /sns/jscode2session 交换，参数 appid／secret／js_code／grant_type，返回 OpenID、会话密钥及可选 UnionID。code 单次使用；session_key 不下发客户端。旧版 v0.x 的路径检索未发现可复用的小程序登录实现。

## 实现

- `mini_app::MiniApp::codeToSession` 沿用 EasyWeChat 方法名；通过现有 SocialiteManager／Facade 的 `miniApp()` 获取。没有伪造 OAuth access token、回调地址、state、refresh、无意义的 ProviderDefinition 钩子或新管理器。
- 配置在 services.weixin_mini_app，沿用生态 app_id／secret；环境前缀 WEIXIN_MINI_APP 与现有 WEIXIN 驱动一致，完整变量 WEIXIN_MINI_APP_APPID／WEIXIN_MINI_APP_SECRET。不新增 SDK 生产依赖、数据表或自动路由。
- 复用应用 HTTP Client，5 秒超时，凭据与响应观察记录脱敏，禁止重定向，无自动重试。每次调用独立交换，响应和身份不存共享驱动状态。
- Session 为 Rust 类型化返回值，包含 openid／可选 unionid／Zeroizing 会话密钥，无 Serialize／Debug。`session.user()` 接回 Socialite User，只携带公开身份；raw／attributes／credentials 均不含 session_key。HTTP 状态、供应商错误码、非法 JSON、缺失／非法身份与密钥明确失败，错误不回显供应商响应。较 PHP 返回数组与直接拼接错误载荷，这是现有 Rust 凭据保护机制的必要适配。
- 应用负责本地绑定、颁发自己的登录态和业务路由；UnionID 不自动授权合并。昵称／头像／手机号授权及完整微信 SDK 不属于本次登录交换范围。三语正文补齐实际用法及限制。

## 验收

隔离源码与独占 target：`/private/tmp/openrust-weixin-mini-20261121/`，源码基线 `8131dd07f72f3531bdd7e6d066a0b53270a5ad98`，不包含其他工作区未提交内容。target 初始用 APFS 独立克隆既有编译缓存；后续写入和清理仅作用于本次路径。Rust 1.94.0、增量关闭、jobs=3、dev/test debug=0。初次构建工作目录发现不符后中止；以下结果均来自隔离源码。

- `cargo +1.94.0 test --locked -p openrust-socialite --test mini_app`：7 函数通过。实际默认端点／查询编码、错误矩阵、可选 UnionID、会话密钥隔离、真实 TCP 并发两应用／多用户、微信拒绝 code 重用、禁止重定向，以及真实 Kernel JSON API／服务注册／Facade／无 Web Session。三语 Rust 示例在测试中编译并实际调用。最终日志 mini-delivery.log。
- 运行固定 EasyWeChat Utils.php 的真实 codeToSession，PHP 宿主仅替换 HTTP／Account；比较有／无 UnionID 及拒绝 code。请求 fixture 接入 Rust 断言；重复运行逐字一致，篡改源文件被 source guard 拒绝。参考入口 tests/reference/mini_app.php，固定源码 SHA 写在入口，fixture 为 tests/fixtures/mini_app.json。PHP 8.5.12 固定镜像 ID 沿用 Tencent 审计；禁网／只读挂载，php-repeat.json／php-negative.log 留证。没有照抄算法冒充 PHP 对照。
- 隔离 MySQL 8.4.13 运行完整 Socialite 包，49 个函数通过，包含原有真实本地账户绑定登录；日志 socialite-all.log，夹具批次 e4570911b16ecc1b329d6f3bfa2a9cfc 收尾无容器残留。最终新增 fixture 和测试改名后专项7函数再验通过。
- 根 services 配置测试 1 函数通过，独立凭据、环境读取及无需 redirect 已核对，见 config-test.log。
- `cargo +1.94.0 clippy --workspace --all-targets --all-features --locked -- -D warnings` 与 `cargo +1.94.0 fmt --all --check` 通过，见 clippy-final.log／fmt-isolated-final.log。依赖 imgalifier-avif 既有 cfg 警告未放宽 workspace 检查。
- 三语内容检查、VitePress 构建、根与 /docs 两挂载三语桌面／移动 12 组合通过；Socialite 专项 6 页核对新增小程序章节与接口／环境变量。见 docs-content-final.log／docs-build.log／docs-browser.log／docs-socialite-final.log。

没有真实小程序 AppID／AppSecret／测试用户，微信生产端点与真机登录未验。新协议不访问数据库或缓存，因此未扩称三库／三缓存回归；本次未改开发数据与供应商配置。当前不交付应用自己的账号注册策略或业务前端。

## 交付与清理

仅提交本次实现、测试、三语文档及本记录／current 交接；保留并行支付、Cargo.lock 和文档 API 相关改动，不推送。完成验证后仅清理本批独占 target、站点 dist／cache，并解除本批 node_modules 符号链接，保留源码、日志、共享缓存与全部数据库卷。实测清理结果在下方补记。

清理前后 Available 为 32,221,640／35,590,636 KiB，净增加 3,368,996 KiB，记录 cleanup.json。删除独占 APFS 克隆 target 和站点产物，未删除任何数据库卷。源码、参考资料与验收日志保留。
