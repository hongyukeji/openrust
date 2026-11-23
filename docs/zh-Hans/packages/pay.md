# 支付（扩展）

`openrust-pay` 支持微信支付 API v3 普通直连商户的公钥模式，以及支付宝 RSA2 普通公钥模式的一次性支付。复用应用 HTTP 客户端、容器与数据库事务。订阅计费、Stripe、Paddle、服务商、分账、自动扣款、证书模式及证书自动下载不属于本包支持范围。

Stripe 订阅与 Paddle Billing 使用 [Cashier](billing.md) 和 [Cashier（Paddle）](cashier-paddle.md)。

## 启用与配置 {#configuration}

根应用启用 `pay` Cargo feature 后，包发现登记 `PayServiceProvider`，其 boot 钩子登记回调收件箱迁移，登记过程不查询数据库。使用 `DatabaseNotifications` 前先迁移：

```sh
cargo run --features pay -- artisan migrate
```

包自身提供默认配置，默认应用不包含 `config/pay.rs`。需要定制时运行 `./artisan vendor:publish --tag=pay-config`，在 `config/mod.rs` 的模块声明和配置映射中分别加入 `mod pay;` 与 `("pay".to_owned(), pay::load(env)?),`，然后重新构建。发布默认保留已有文件，传入 `--force` 才覆盖；配置缓存快照保留所选商户配置。

`PAY_DRIVER` 设置 `pay.default`，默认 `wechat`。商户配置放在 `pay.drivers.<name>`，`driver` 为 `wechat` 或 `alipay`。不同商户或应用 ID 使用不同配置名。解析网关前不访问网络或读取密钥文件。

| 驱动 | 环境变量 |
| --- | --- |
| 微信支付 | `PAY_WECHAT_APP_ID`、`PAY_WECHAT_MCH_ID`、`PAY_WECHAT_SERIAL_NO`、`PAY_WECHAT_PRIVATE_KEY_PATH`、`PAY_WECHAT_API_V3_KEY`、`PAY_WECHAT_PUBLIC_KEY_ID`、`PAY_WECHAT_PUBLIC_KEY_PATH` |
| 支付宝 | `PAY_ALIPAY_APP_ID`、`PAY_ALIPAY_SELLER_ID`、`PAY_ALIPAY_PRIVATE_KEY_PATH`、`PAY_ALIPAY_PUBLIC_KEY_PATH` |

私钥为 2048 位 RSA 的 PKCS#8 或 PKCS#1 PEM 文件。平台公钥为 SPKI 或 PKCS#1 PEM，通过商户可信管理渠道取得。微信 `serial_no` 是商户签名证书序列号，`public_key_id` 是以 `PUB_KEY_ID_` 开头的平台验签公钥 ID，两者职责不同。API v3 密钥必须正好 32 字节。轮换时可在 `public_keys` 中增加其他可信公钥 ID 与 PEM 内容。

配置也可用 `private_key`／`public_key` 内联 PEM 替代对应路径，每把密钥只能指定一种来源。相对路径以进程工作目录解析，也可使用绝对路径。配置缓存及密钥文件须由部署权限保护。可选 `endpoint` 必须使用 HTTPS，仅回环地址允许 HTTP 协议测试；默认指向官方微信 API v3 域名和支付宝网关。支付宝沙箱须同时使用沙箱端点及其对应凭据、公钥。

其他应用可依赖 `openrust-pay` 并登记 `PayServiceProvider`；它通过 `load_migrations_from` 登记 `PayServiceProvider::migrations()`。网关注册及直接支付操作不要求访问本地数据库。

## 发起支付 {#create-payment}

金额使用人民币整数分，也支持精确解析 `Amount::from_yuan("12.34")`；拒绝负数、零、指数及超过两位小数。商户订单号在查询和重试间保持不变，使用 1–32 位 ASCII 字母、数字、下划线或连字符。回调及可选返回地址须使用 HTTPS。

```rust
use openrust_pay::{Amount, Order, Scene, facades::Pay};

let order = Order::new(
    "order_001",
    "Coffee",
    Amount::from_fen(1234)?,
    "https://shop.example/pay/notify",
);
let payment = Pay::driver("wechat")?.pay(Scene::Scan, &order).await?;
```

`Payment` 是供前端使用的参数，不代表已经支付。按枚举处理：`QrCode` 用于渲染二维码，`Redirect` 用于浏览器跳转，`App` 为支付宝 App 订单串，`Jsapi` 为微信 JSAPI／小程序参数，`WechatApp` 为微信 App 参数。微信调起参数由商户私钥签名。

| 场景 | 微信支付 | 支付宝 |
| --- | --- | --- |
| `Scene::Scan` | Native 扫码 | 预创建扫码 |
| `Scene::Jsapi` | JSAPI／小程序；设置属于该应用的 `order.openid` | 不支持 |
| `Scene::Wap` | H5；从可信请求或代理取得 `order.client_ip` | 手机网页 |
| `Scene::Web` | 不支持 | 电脑网页 |
| `Scene::App` | App | App |

支付宝网页支付可设置 `order.return_url`。付款人的 OpenID 须通过对应应用的认证流程取得，其他应用的第三方登录标识不能直接替代。

## 查单、关单与退款 {#query-and-refund}

使用 `gateway.find(out_trade_no)`、`gateway.close(out_trade_no)`、`gateway.refund(&Refund)` 和 `gateway.find_refund(out_trade_no, out_refund_no)`。返回经过签名验证的厂商 JSON；须检查交易或退款状态，不能把请求成功直接当作资金处理完成。`Refund` 包含原订单号、稳定的商户退款单号、原订单 `total`、本次退款 `amount` 及可选 `reason`。退款金额不能超过原订单金额，支持部分退款。

请求不会自动重试。`Error::OutcomeUnknown` 表示无法确定厂商受理结果，包括响应丢失、响应签名无效等情况。先用原订单号或退款单号查询，再决定是否重试，不要为不确定请求生成新单号。经过验签的厂商拒绝返回 `Error::Rejected`；错误与 HTTP 观察事件不包含原始厂商消息和载荷。请求／响应正文与授权头会脱敏，但返回给应用的成功 JSON 仍包含业务数据，记录日志或序列化时应明确选择字段。

## 验证与处理回调 {#callbacks}

将原始请求头与原始字节交给 `gateway.callback`。微信先检查可信公钥 ID、RSA 签名与五分钟签名时间窗口，再进行 AES-GCM 解密；支付宝仅解码表单一次，拒绝重复键及非法编码，并验证 RSA2。两者均核对商户和应用身份。仅接受支付成功事件；退款通知及其他事件不能交给此处理器。退款流程使用退款查询确认结果。

从可信业务订单读取 `ExpectedPayment`，核对不可变的原始金额和订单号。支付包不负责创建业务订单、库存决策、发货或授予会员。

```rust
use openrust_pay::{DatabaseNotifications, ExpectedPayment, facades::Pay};

let notification = Pay::driver("wechat")?.callback(&headers, &raw_body)?;
let expected = ExpectedPayment {
    out_trade_no: business_order_number,
    amount: business_order_amount,
};
DatabaseNotifications::new(connection)
    .process(&notification, &expected, |tx, notice| Box::pin(async move {
        // Validate the business state and update it using this transaction.
        tx.table("orders")
            .r#where("number", "=", notice.out_trade_no())
            .r#where("status", "=", "pending")
            .update([("status".into(), "paid".into())].into_iter().collect())
            .await?;
        Ok(())
    }))
    .await?;
let response = notification.success();
```

`payment_notifications` 收件箱只存事件身份哈希、规范化指纹和处理状态，不存原始回调及凭据。按商户／应用身份和厂商交易号去重，支付宝重复的成功／完成通知不会重复执行处理器。同一身份的内容变化返回 `NotificationConflict`。`Processed` 与 `Duplicate` 均可在提交后确认；处理失败或取消时，业务更新与收件箱标记一起回滚。

处理器须使用传入的 `Transaction`，核对当前业务状态，并仅执行事务内 SQL。原始预期订单必须来自可信来源；若金额字段允许修改，须在处理器内加锁并重新核对。发送消息或调用外部履约服务应通过应用事务发件箱在提交后处理，此辅助类不保证外部副作用原子性。不同应用仅在共享业务订单归属时才能共用收件箱。

不使用辅助类时，调用 `notification.validate(&expected)`，并在业务事务中实现同等的持久去重。前端返回页或仅验签成功都不能直接标记履约。业务事务提交后再返回成功确认；验证或处理失败应返回错误响应，由厂商按协议重试。

## 网关生命周期 {#gateway-lifecycle}

`PayManager::driver(name)` 懒解析并复用商户配置和密钥快照。`get_default_driver` 读取当前默认名，`Pay::default_driver()` 解析它。`forget_drivers` 让后续解析使用新配置，已持有的 `Gateway` 克隆保留原快照。管理器按应用隔离。轮换密钥时刷新相关实例，不从未验证回调获取验签公钥。

真实商户入驻、产品权限开通、公开 HTTPS 回调及厂商端到端验证由部署方完成。协议测试通过不代表商户账号已经可以实际收付款。
