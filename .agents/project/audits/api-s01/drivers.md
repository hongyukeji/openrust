# API-S01：六个新增 OAuth2 驱动

2026-11-20：本批骨架、实现与本机验收完成，API-S01 整体仍未闭合。

## 固定参考与实现

沿用 Socialite v5.33.0 / 8032bb1fa9d74aae54438f216e4d45db90d27ea9，十三份真实 PHP 源码 SHA 由 tests/reference/drivers-source-sha256.json 锁定。旧版没有 Socialite，对应 OIDC 线索留待 API-O01。隔离基线 4e7042a241653f3027c09ef4aa90a1129c221912；本批不变更 Cargo 依赖。根工作区同期验证器及 plan WIP 保留。

新增 GitlabProvider、BitbucketProvider、LinkedInProvider、LinkedInOpenIdProvider、SlackProvider、SlackOpenIdProvider，接通应用内 manager、六组 services 环境配置和三语正文。ProviderEndpoints 允许受控 HTTP 端点注入。GitLab 按固定源码使用 /api/v3/user 与 query access_token，host 支持路径前缀，非法配置类型拒绝。Bitbucket 仅显式 getAccessToken 使用 Basic，默认授权码交换沿用父类；可选邮箱失败返回 None。LinkedIn 保留 RestLi 头、projection、locale、100/800 图片及邮箱错误传播。Slack 区分用户 authed_user 与 asBotUser 顶层 token、user_scope 覆写顺序及组织映射。两个 OpenId 类型实现固定上游 Bearer UserInfo，不验证 ID Token，不能作为完整 OIDC RP 或 SSO 交付。

共享 AbstractProvider 增加驱动可覆写授权字段、token 响应及显式直接 token 方法，沿用 Session state、PKCE 和敏感 HTTP 观察隔离。请求借用期与凭据 Zeroizing 分组继续沿用首批 Rust 适配。

## 实际验收

证据 /private/tmp/openrust-socialite-drivers-20261120/。Rust 1.94.0，socialite 全包 13 个不同函数（GitHub 7＋本批 6）和根纯配置 1 个函数通过；docs_example 编译，不计测试函数。真实 loopback HTTP 验证六驱动 token 表单、Basic／Bearer／query、profile／邮箱／projection、refresh、Slack 用户／bot 凭据、远程撤销 401、错误状态／JSON及多应用注册隔离。HTTP Client 14 项沿用首批通过证据，本批未修改核心客户端，不重复计数。

PHP 8.5.12 使用已有固定镜像 php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9，禁网、只读挂载十三份固定源码。Probe 只暴露 protected 方法，宿主 Arr／Request 提供基础操作，未复制驱动算法。六映射、默认／stateless／参数字段、Slack bot 和 GitLab host 对照通过，重复运行逐字一致，篡改 GitlabProvider 的负样本在 require 前被 SHA 拒绝。

严格 workspace／all-targets features pail,passport,socialite Clippy 与 fmt 通过。三语八章节构建、12 项通用及 6 项 Socialite 浏览器检查通过。最终 feature 根二进制在 MySQL 8.4.13 wrapper 下原始 ./artisan inspire、Passport 帮助、真实 HTTP 默认路由／错误响应、SIGINT／端口释放通过。本批 Socialite 流程未执行 SQL，不声称完成 MySQL 本地身份归属。

初轮语法错误、PHP Probe protected 方法访问错误及其非 JSON 输出均已修正；最终真实 PHP fixture 和整包测试重新通过。负样本错误由 PHP 写入 stdout，验收同时核 stdout／stderr。失败日志保留。

## 剩余范围与收尾

继续 Facebook／Google／X／Twitter、OAuth1、真实 MySQL 本地用户归属及独立 API-O01／02。真实供应商授权仍需各家测试应用、client id／secret、登记回调域名及账户；受控 HTTP 不冒称实机授权。

本批 20 份输入与隔离源码 SHA 一致，保留 accepted-overlay.tgz、固定源码及成功／失败日志。清理已闲置专属 source／target／root-main，镜像、共享缓存和全部数据库卷保留，磁盘及卷前后见 drivers-cleanup.json。本地核范围提交，不推送；继续后续开发。
