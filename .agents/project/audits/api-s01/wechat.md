# 微信登录统一分组

2026-11-21，按用户批准的方案，将网站扫码、公众号网页授权与小程序登录的配置及实现统一归入 wechat，QQ 保持独立。

## 来源与取舍

沿用此前已联网核对的固定来源：[腾讯网页登录记录](tencent.md)中的 Laravel Socialite 5.33.0 与 SocialiteProviders，以及[小程序记录](mini-app.md)中的 EasyWeChat `ba8022698a613e37b10ed1e4db604c5a96af0a63`、overtrue/laravel-wechat `133828b7950acceff9b11deb442b65a9cf203292`（均 MIT）。后者集中配置并区分 official_account／mini_app／open_platform；采用集中配置、按应用类型隔离的机制。本站 web 分组与 services.wechat 路径是用户批准的 Rust 项目适配，不宣称是 Laravel Socialite 原生配置标准。

保留 SocialiteProviders 的 weixinweb／weixin／qq driver 名称，以及 EasyWeChat 风格 miniApp().codeToSession。复用已有 SocialiteManager、Facade 与 HTTP Client，不新增管理器、生产依赖、数据表或登录协议。

## 变更与迁移

- 网站扫码：services.wechat.web，WECHAT_WEB_CLIENT_ID／CLIENT_SECRET／REDIRECT_URI。
- 公众号：services.wechat.official_account，WECHAT_OFFICIAL_ACCOUNT_CLIENT_ID／CLIENT_SECRET／REDIRECT_URI。
- 小程序：services.wechat.mini_app，WECHAT_MINI_APP_APPID／SECRET。
- QQ：services.qq 与 QQ_* 不变。
- 公开类型统一为 openrust_socialite::wechat::{MiniApp, Session, WeixinProvider, WeixinWebProvider}。OAuth2 公共协议类型仍在 two。

旧顶层 services.weixinweb／weixin／weixin_mini_app 和 WEIXIN* 环境变量不再回退读取；三语正文提供迁移映射。当前本机 .env 与 .env.example 未发现旧微信键，未改写本机凭据。新增回归明确验证三种应用凭据、回调及 scopes 的选择，旧配置不能静默选择另一应用。

## 验收

隔离源码基线 `5de45c53b37b2345c2f57ace824fde7e07b78367`，叠加本次21条实现／测试／文档路径；与主工作区逐字节核对一致，删除路径两边均不存在。隔离目录 `/private/tmp/openrust-wechat-group-20261121/`，独占 target 从零构建，Rust 1.94.0、增量关闭、jobs=3、dev/test debug=0。不包含并行支付与设置装配的未提交改动。

- china／mini_app 专项15函数通过（protocol.log）。隔离 MySQL8.4.13 完整 Socialite50函数通过，含真实本地账户绑定；批次 eeec8e055701ec29b6f5e2af3743840f（socialite-all.log）。
- 根配置 socialite_china_configuration／configuration 各1函数通过（config-tests.log）；启用 socialite 的 artisan inspire 退出0（root-smoke.log）。
- cargo +1.94.0 fmt --all --check 与 clippy --workspace --all-targets --all-features --locked -- -D warnings 通过（fmt.log／clippy.log）。
- 三语内容及 VitePress 构建通过；Socialite专项6页、根与 /docs/ 三语桌面／移动12组合通过（docs-build.log／docs-socialite.log／docs-browser.log）。普通 API 指南12组合通过（docs-api-guide.log，--without-export）；没有将其计为完整 API 导出站验收。
- cargo +1.94.0 doc --locked -p openrust-socialite --no-deps 通过（rustdoc.log）；独立浏览器在 /api/ 与 /docs/api/ 验证四个 wechat 类型及 SocialiteManager，共10页的方法锚点及真实源码链接（docs-wechat-api.log）。同步修正站点快照检查中的 MiniApp 路径；未执行完整 API 导出。

本次只调整配置及模块归组，不改 SQL／缓存行为，不扩称三库三缓存全矩阵通过。真实微信／QQ应用凭据、供应商扫码授权与小程序真机联调仍未验。应用账号注册／绑定策略及供应商业务权限按原支持边界。

## 收尾

确认本批无构建／浏览器占用后删除独占 target、站点 dist，并仅 unlink 本批 node_modules 符号链接；源码、参考与日志保留。清理前后 Available 为35,712,408／41,137,524 KiB，净增加5,425,116 KiB（并行构建影响瞬时磁盘读数），详见 cleanup.json。本批MySQL标签无残留容器，未删除数据库或数据卷。仅核范围本地提交，不推送；保留其他协作者改动、开发数据与全部数据库卷。
