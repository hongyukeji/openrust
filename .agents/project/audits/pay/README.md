# 国内支付扩展

2026-11-21，用户授权按建议实现独立支付扩展。首期为微信支付 API v3 普通直连商户公钥模式、支付宝 RSA2 普通公钥模式的一次性支付；订阅计费、Stripe／Paddle、服务商／分账／自动扣款和证书模式不纳入此批。计划见[支付扩展](README.md)。

## 方案来源与取舍

官方搜索工具返回空结果，实际联网通过 GitHub API 及官方仓库原始源码核对，固定副本及元数据保留在 `/private/tmp/openrust-pay-20261121/upstream/`。未以 Star 数作为方案质量结论，未复制完整 SDK 或引入其 PHP 运行时。

- [Yansongda Pay](https://github.com/yansongda/pay/tree/01cf002156d99f61fcba6308be810554dfd879d3)，MIT，核对 `Pay`、支付宝／微信 Provider 与验签插件；采用场景化支付、查单／退款／回调的职责分工，不搬入完整插件流水线、全局容器及其他未授权供应商。
- [Laravel Pay](https://github.com/yansongda/laravel-pay/tree/b2b2594e97f2ee2e1e672b55f8085b25819b9612)，MIT，核对 ServiceProvider／Facade 和 `pay` 配置。沿用 Provider／Facade／具名商户配置；Rust 管理器依应用隔离，采用项目现有 Laravel Manager 的实例缓存与显式重载机制。
- [微信官方 PHP SDK](https://github.com/wechatpay-apiv3/wechatpay-php/tree/3c9f5c2b71d181e9c244494f695cf5abb53cadc4)，Apache-2.0，核对 Formatter、RSA、AES-GCM、客户端与 JSAPI 签名示例。按其 API v3 签名串、可信公钥 ID、时间窗口及先验签后解密的边界实现；密钥解析和密码运算复用现有 RSA／aws-lc，不自写密码算法。
- [支付宝官方 PHP SDK](https://github.com/alipay/alipay-sdk-php-all/tree/e7e5857f64b0e98516552df2525764376b67ef70)，仓库 API 许可证元数据为空，未复制进项目或引入为生产依赖。核对 AopClient 的 RSA2、公参排序、回调 `sign_type` 排除及同步响应原始 JSON 验签；保留其官方外部源码作为独立 PHP 对照输入。
- [Laravel Cashier](https://github.com/laravel/cashier-stripe/tree/d5c015490ff77f2c138fe79b86d2a982787fe3ac)，MIT，只核对支付与订阅计费边界，未把国内一次性支付包装成 Cashier 已交付能力。
- `git ls-tree -r v0.x` 未找到支付、支付宝或微信支付扩展；仅有无关的 payload 加密源码。本批没有把第三方登录 OpenID 接入误计为支付代码复用。

`PayManager`／`pay.drivers.<name>` 是 OpenRust 对具名商户管理的语言适配，不声称 Yansongda 或 Laravel 内置相同的 Rust API。原生 HTTP Factory、容器与 SQL 事务继续使用；新增依赖均为仓库已有锁定版本，仅为 serde_json 启用 raw_value，不增加另一套 HTTP／密码／队列框架。

## 实现与公开范围

独立 `vendor/openrust/pay`，根 `pay` feature 与 `config/pay.rs`，包发现登记 `PayServiceProvider`；包Provider的boot钩子登记原生通知收件箱迁移。凭据按网关实例快照，`forgetDrivers` 后后续解析读取新配置，已持有实例保留旧快照。密钥支持 PEM 文件或内联 PEM，来源二选一；微信允许预配置多个可信公钥用于轮换，不从回调自动下载或信任公钥。

微信 Native／JSAPI（含小程序）／H5／App，支付宝扫码／电脑网页／手机网页／App；查单、关单、退款与退款查询。微信前端调起参数由商户私钥签名。CNY 金额使用整数分，拒绝浮点、不合法金额和路径注入。订单与退款单号稳定复用，不自动重试；失去响应、验签失败或不可信结果返回 OutcomeUnknown，由应用先用原单号查单。

支付成功回调先验证签名及商户／应用身份，再匹配可信业务订单号与总额。支付宝重复键与非法百分号／UTF-8编码拒绝；微信验签时间窗口五分钟、可信key ID选择后AES-GCM解密。仅接受支付成功通知；退款通知和其他事件明确不支持，退款通过签名查询确认。

可选 `DatabaseNotifications` 将已验证事件的身份哈希、规范化指纹与处理状态存入 `payment_notifications`。无操作upsert写锁、锁定读取、业务处理器、完成标记位于同一事务；回滚和取消可重试，重复交易不再次调用处理器，身份重用而内容变化明确失败。业务订单所有权、状态、金额变更与跨交易业务幂等仍由应用验证，外部履约需应用事务发件箱；不将此表描述成外部副作用 exactly-once 保证。

## 验收记录

隔离源码、固定上游及日志位于 `/private/tmp/openrust-pay-20261121/`，初始源码基线 `b168b31aa`（完整SHA见临时revision）。显式 Rust1.94、独占target、增量关闭、jobs=3、dev/test debug=0。初次Cargo误在活跃树启动、产物仍在本批独占target，发现后中止并转到隔离源码；后续所有构建均在隔离源码执行。

- 包内协议9函数通过：实际TCP下微信四场景与支付宝四场景、请求RSA验签、微信调起签名、支付宝原始非规范化JSON响应验签、查单／关单／部分退款／退款查询；回调签名／身份／币种／总额／重复头与重复表单键／非法编码／GCM失败／过期／未知key ID负例；15秒真实超时只发一次、重定向拒绝、不合法输入不发HTTP。含应用Provider／Facade／真实HTTP内核及配置快照隔离，HTTP观察记录中正文／授权头／响应正文脱敏。日志 `pay-tests-final.log`；示例同时编译。
- 独立参考：固定支付宝官方AopClient实际 `rsaSign` 和微信官方 `AesGcm::encrypt` 生成两组PHP样本，Rust验证通过。官方外部输入SHA守卫记录在 tests/reference/source-sha256.json，生成入口拒绝源文件被篡改；使用既有PHP8.5镜像ID `50956f094e90`，禁网／只读根文件系统。宿主php7.4二进制因缺libtidy不可用，改用已有镜像，未修改系统安装。
- 通知收件箱同一函数已在SQLite、PostgreSQL180006、MySQL8.4.13通过：原生迁移、业务写入后错误回滚、取消后恢复、八个并发回调仅一次处理、重复成功、金额不匹配前置失败、相同身份不同金额拒绝。正式SQL wrapper使用隔离凭据和实例，保留全部数据卷。日志 `pay-tests-final.log`／`notifications-{postgres,mysql}.log`。只改数据库通知收件箱，不新增或修改缓存驱动。
- 三语内容检查及VitePress构建通过；支付正文根／docs两挂载、三语桌面／移动12组合及通用12组合浏览器通过。日志 `docs-content.log`／`docs-build.log`／`docs-pay-browser.log`／`docs-browser.log`。
- 全workspace／all-targets／all-features严格Clippy已通过（`clippy-final.log`）；初轮发现根配置错误使用不存在的env::string，已改为项目env::get并完整复验。其他初始测试编译问题（同步disconnect误加await、捕获闭包传入函数指针、Arc内核调用后复用）已修正，失败日志保留，不将初轮失败计作通过。最终应用原生Artisan、配置及集成复验结果在后文记录。

真实商户账号、产品权限、真实扫码／客户端SDK、公开回调网络与真实资金收付未验；不声称已通过厂商生产验收。未修改任何开发数据库或真实商户信息。只核范围本地提交，不推送。


## 最终装配复验

为避免与并行应用骨架清理交叉修改，通知收件箱迁移最终由 PayServiceProvider 的 boot 钩子调用 loadMigrationsFrom 登记，取消本批对 AppServiceProvider 的改动。登记不执行数据库查询或读取商户密钥。固定迁移源文件位于包 database/migrations，进入现有API源码指纹扫描范围。

- 最终源码包内共11函数通过：protocol9、notifications1、failures1，另有示例编译；failures补验经过签名的厂商拒绝与未签名响应的不同错误、错误／事件脱敏、显式公钥轮换和端点约束。日志pay-final-source.log／failures.log。支付宝时间格式使用框架自带的Asia/Shanghai时区数据，避免部署系统时区数据库依赖。
- 根配置1函数＋pay_configuration2函数通过（root-tests-final.log）。真实独立Artisan在SQLite、PG180006、MySQL8.4.13重新通过启用feature后的迁移、重复迁移保留收件箱数据、状态和回滚；密钥路径故意指向不存在文件，确认迁移不读取凭据。日志runtime-{postgres,mysql}-final.log。SQLite实际版本3.54.1（notifications-sqlite-final.log）。
- 最终全workspace/all-targets/all-features严格Clippy（clippy-complete.log）及全仓fmt检查（fmt-final.log）通过。仅依赖内部既有编译告警，不存在本包未处理warning。配置文件env::get与根feature接线已覆盖根测试和all-feature编译。
- 调整Provider装配说明后的三语内容／构建（docs-build-final.log）和支付页两挂载三语桌面移动12组合（docs-pay-browser-final.log）再次通过；先前通用12组合保留有效。PHP固定源码对照重生成逐字一致，篡改官方AopClient输入被source guard拒绝（php-repeat.log／php-negative.log）。

未改缓存行为，不将SQL通知测试计作缓存专项验收。保留并行对话的微信模块整理、Settings示例清理与核心文档改动。最终仅提交支付包、配置／feature、根定向测试、三语支付入口和本批记录；不提交其他工作区改动。

## 产物清理

确认本批构建与浏览器进程已退出后，删除独占target、文档dist／cache，仅解除隔离源码的共享node_modules符号链接。保留源码、上游、日志、共享依赖和全部数据卷。df可用由39,020,008 KiB增至43,400,980 KiB，净增加4,380,972 KiB；并行对话仍在构建，净值受其活动影响。清理明细保留在本批临时cleanup.json。

## 已完成计划归档（2026-11-23）

国内支付与订阅计费的本地协议／三库验收已完成；Cashier记录见同目录cashier.md。真实商户、资金和实际客户端联调仍未验，不因计划删除改变授权或支持范围。

原计划 `payments.md` 已删除；历史设计、调研及执行前快照可从提交 `f80096ea6` 的 `.agents/project/plans/payments.md` 查阅。本文保留实际验收与未验边界，不因归档补计测试。

## DRV-02 自定义支付驱动 {#custom-drivers}

固定 Yansongda Pay 通过容器定位具名收单服务，Laravel 集成负责 Provider 与 Facade 装配。本批沿用固定 Laravel `FilesystemManager` 和项目现有短信／翻译管理器的实例内工厂与配置快照，添加 `PayManager::extend` 和外部可实现的 `PayDriver`。内置 `wechat`／`alipay` 名称保留，外部实现仍由现有 `Pay::driver`／`Pay::default_driver` 解析。`openrust-v0.x` 的支付模块文件检索未找到可复用驱动契约。未新增依赖。[固定 Yansongda Pay](https://github.com/yansongda/pay/blob/01cf002156d99f61fcba6308be810554dfd879d3/src/Pay.php) · [固定 Laravel Manager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Filesystem/FilesystemManager.php)

自定义工厂收到具名商户配置，失败后可重试。替换工厂后由 `forget_drivers` 刷新后续解析，旧 Gateway 保持原实例；不同应用的注册隔离。外部 Gateway 执行既有订单与退款输入校验。外部驱动负责验签和匹配商户，成功后提供 `DriverNotification`；仅 Gateway 将其转换成不可由调用方直接构造的 `VerifiedNotification`。自定义身份对驱动、已验证商户及交易结构化编码，保留内置两家原有身份、回执和指纹。`DatabaseNotifications::process` 的去重、行锁、业务回调、完成标记继续使用同一事务。

Rust 1.94.0 在 `177b24933` 快照运行 Pay 包测试，包括既有微信／支付宝协议、收件事务与新外部 HMAC 驱动。新包级用例覆盖具名配置、工厂失败重试、实例复用、应用隔离、替换和旧句柄、支付／查单／关单／退款、不支持操作、输入拒绝、原始签名或商户不符、类型化回执与可信订单校验。

`tests/pay_driver_acceptance.py` 使用独立应用的公开 Provider／Facade 入口，在 SQLite、PostgreSQL 18.7 和 MySQL 8.4.13 各通过真实迁移、6 次 HTTP 回调及独立 Artisan。隔离 HTTP 夹具使用固定鉴权标记和原文；HMAC 由前述外部驱动测试单独覆盖。先验证非法通知拒绝、业务写入后的失败使两表回滚，再验证成功与重复均返回预期 HTTP 回执且只写入一份业务记录。生成配置／路由缓存后更换进程环境中的商户值，HTTP 和 Artisan 均继续使用缓存配置并确认重复，不再执行业务写入。本批无需真实商户、外部网络或资金交易；真实供应商验收继续单列。

Pay 包 14 项测试、包所有目标与独立应用目标严格 Clippy、全工作区格式检查和 Python 语法均通过。应用 Clippy 首轮因磁盘停线中断，复跑发现夹具中多余的同类型转换。删除该转换后重新构建夹具，三库 HTTP／Artisan 及格式检查复验通过。结果保留于 `target/tmp/pay-custom-driver`。
