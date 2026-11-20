# 国内支付扩展

2026-11-21 用户授权按建议开发。首期交付独立 `openrust-pay`：微信支付 API v3 普通直连商户（公钥模式）与支付宝开放平台 RSA2 普通公钥模式，具名商户配置、下单／查单／关单／退款／退款查询、签名响应验证和支付成功回调。优先 Native／JSAPI 与支付宝扫码／网页／手机网页／App；微信 H5／App 同批有协议测试才声明支持。订阅计费、Stripe／Paddle、服务商／分账／代扣及证书自动下载不纳入首期。

## 契约

- 根配置 `pay.default`、`pay.drivers.<name>`；Provider／Facade／PayManager 懒创建、按配置名共享，`forgetDrivers` 显式重载，凭据作为实例快照。
- 借鉴 Yansongda Pay／Laravel Pay 的支付职责、场景名称和 Provider 装配，遵循 Laravel Manager 生命周期。Rust 使用类型化订单和金额（人民币分），显式传入场景；不引入 PHP 插件流水线或全局静态配置。复用现有 HTTP Factory、RSA／AES-GCM 实现与数据库事务。
- 验证请求／响应／回调签名；微信验签使用商户预置信任公钥ID，回调验签后解密并核对商户和appid。支付宝同步响应验证原始JSON片段，异步参数只解码一次，拒绝重复键，核对appid／seller_id。不可从未验证回调下载信任密钥。
- 已验回调仍须与业务订单的编号／金额／币种核对。可选数据库通知收件箱仅在与业务更新同一事务成功后标记处理；失败／取消可重试，重复通知不重复执行。业务订单、发货、会员、外部副作用由宿主负责，不能把仅验签称作业务完成。
- 不自动重试支付请求；超时等结果未知先查单，退款使用稳定的商户退款单号，不产生新单号盲目补发。

## 调研与旧版

固定来源及原始源码存于 `/private/tmp/openrust-pay-20261121/upstream/`，审计记录保存提交与采用差异。官方网页搜索工具返回空结果，改用GitHub API与官方仓库原始源码。`git ls-tree -r v0.x` 未找到支付扩展或支付宝／微信支付实现，仅有无关payload加密模块；不将第三方登录冒充支付复用。

## 验收

隔离源码、独占target、Rust1.94、增量关闭、jobs=3、debug=0。覆盖本地实际HTTP与独立RSA签名样本，正常交易与签名／金额／身份／响应／重放失败；通知收件箱分别在SQLite、PG18、MySQL8验证事务回滚、重复与并发。严格Clippy／fmt，三语正文、站点构建与根／子目录浏览器检查。无生产商户密钥，不声明真实资金交易已验。首期骨架、实现与本机协议／三库验收已完成，详见[支付验收](../audits/pay/README.md)；真实商户资金交易仍未验。

## 完整订阅计费

2026-11-21 用户明确要求继续补齐订阅、Stripe、Paddle。以独立 openrust-cashier 包承载两种计费供应商，复用 HTTP Factory、包发现和 SQL 事务，不把人民币分类型套用多币种。根 cashier feature 与 cashier 配置；CashierManager／Cashier Facade、绑定可信应用身份的 Billable、SubscriptionBuilder／SubscriptionHandle，三语沿用 billing 与 cashier-paddle 入口。Stripe API 固定官方 SDK 版本，Paddle Billing API v1；不混入已退役 Paddle Classic。

交付客户创建与显式关联、托管 Checkout／客户门户、Stripe PaymentIntent／SetupIntent／支付方式／税号／余额／Meter Event、两家订阅生命周期（多价格、数量、试用、折扣、按比例结算、取消／恢复，Paddle暂停／激活）、账单／预览／厂商PDF、退款。Stripe试用由API设置，Paddle试用由价格目录设置；同一API的供应商差异明确失败，不伪造支持。高级 api 入口覆盖未封装参数，不承诺逐PHP方法二进制兼容。

回调校验原始字节HMAC、时间窗口、多签名与密钥轮换，拒绝跨账户／Stripe Connect事件。客户映射唯一、业务身份由宿主认证；事件收件箱与订阅／账单投影、应用SQL同一事务。客户锁下读取供应商当前状态，避免旧事件退回新状态。仅保存必要投影，不持久化卡号／地址／client_secret。Paddle不支持Stripe式幂等，持久操作认领后发送，同键重试一律拒绝补发，结果未知必须查单。

验收覆盖HTTP协议、失败／SCA状态、签名负例、并发／重复／乱序／回滚／取消、SQLite／PG18／MySQL8、原生Artisan发现迁移、feature关闭／开启、严格Clippy／fmt与三语浏览器。真实商户凭据和资金、真实托管页面及客户端SDK联调单列未验。

骨架、实现及本地协议／三库行为验收已完成，最终与核心配置归属提交285fcacd0组合验证。详见[Cashier验收](../audits/pay/cashier.md)。包配置／发布模板全部归属包目录，默认应用不预装；真实商户与客户端资金联调仍独立未验。
