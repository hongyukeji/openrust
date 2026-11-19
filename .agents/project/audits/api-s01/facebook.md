# API-S01：Facebook Graph 与 Limited Login

2026-11-20：本批骨架、实现与本机验收完成，API-S01 整体仍未闭合。

## 固定参考与实现

固定Socialite v5.33.0 / 8032bb1fa9d74aae54438f216e4d45db90d27ea9、Firebase PHP-JWT7.2.2沿用Google批。真实phpseclib4.0.3 / 6077cecfed9e371993d3aa339b0afeb23f7bf636及其两项依赖固定见facebook-dependencies.json，属于上游声明范围，不冒称原Composer lock解析。实际PHP文件SHA由tests/reference/facebook-source-sha256.json在加载前核对。旧版无Socialite，复用当前HTTP／Session／JWT基础。隔离0c7b8460e，未纳入同期验证器f5548cac4及后继限流WIP；本批无Cargo依赖变动。

新增two::FacebookProvider、manager和FACEBOOK三环境变量。固定默认Graph v23.0、email scope、默认fields、query access_token与非空且非字符串0的client secret所算HMAC-SHA256 appsecret_proof。代码交换不指定Accept，refresh保留父类Accept；getAccessTokenResponse保留expires，并在expires_in缺失／null时填入，不覆盖已有非null值。真实PHP揭示Arr::add首参数按值求值先于Arr::pull副作用，不能误删返回expires。

ProviderOptions将fields／usingGraphVersion／asPopup／reRequest／withNonce保存在请求借用期，不修改应用内共享定义；动态URL及授权字段经过可覆写入口。lastToken使用Zeroizing，只在实际读取远程身份前记录。Rust无可选位置参数，PHP userFromToken(token,nonce)适配为先withNonce再userFromToken(token)。新增选项profile入口默认委托既有getUserByTokenWithConfig，Google及其他驱动回归保持通过。

Limited Login从未验证header检测非null kid，随后按固定Facebook JWKS首个匹配key强制RS256，忽略JWK alg元数据，严格issuer=https://www.facebook.com及clientid标量audience；必须有期待nonce，显式withNonce优先于with参数nonce，恒时比较。期待nonce必须为字符串，数字／布尔期待值如原PHP拒绝；签名claim的标量nonce按上游字符串化。无期待nonce或验证失败均不fallback至Graph，空kid按固定Facebook方法而非Google keyset语义处理。withNonce仅设期待值，不自动产生／存储／发送授权nonce；调用应用负责保存并关联授权尝试。此路径保留Socialite可选日期规则，不冒充完整API-O01 RP。

## 实际验收

证据/private/tmp/openrust-socialite-facebook-20261120/。Rust1.94.0，全包29个不同函数（既有23＋Facebook6）、纯配置1项通过；docs integration target编译。真实loopback Graph／token／JWKS HTTP覆盖HMAC proof／空secret／字符串0、fields／版本／popup／重试参数、交换表单与Accept、三种expires输入、映射／lastToken、授权码直接JWT、同kid轮换、应用与请求选项隔离、401撤销、错误status／JSON、敏感query观察脱敏及真实wire完整。

固定PHP8.5.12镜像禁网只读挂载真实Socialite／Firebase／phpseclib及依赖，Probe只暴露protected方法；Arr／HTTP宿主不复制驱动或签名算法。21组Limited Login正反例与Rust逐项一致：nonce缺失／错误／参数回退／显式优先、数字／布尔期待nonce拒绝、issuer／aud／数组aud、签名、未来／过期、可选exp、轮换、未知／空kid，以及JWK alg缺失／不同仍强制RS256。所有带kid失败都不转Graph。重复逐字一致，篡改FacebookProvider源在require前被SHA拒绝。使用既有Google批的两把fixture测试RSA密钥，不含供应商凭据。unknown kid触发原PHP的null-offset warning，独立保存stderr，不将警告混入JSON。

严格workspace／all-targets features pail,passport,socialite Clippy、fmt通过；纯配置验证Facebook三环境变量；三语8章节构建、12通用＋6Socialite浏览器通过。最终feature根二进制在MySQL8.4.13 wrapper下原始./artisan、帮助、真实HTTP默认路由／错误响应、SIGINT和端口释放通过。本批不执行SQL，不声称MySQL归属已验收。

失败日志保留：初次oracle未暴露protected auth方法、PHP warning混入JSON均已修；原PHP对照纠正expires返回保留语义及数字／布尔期待nonce的类型错误，最终整包与严格检查重新通过。

## 剩余与收尾

计划中的官方OAuth2供应商路径已逐批验收，继续OAuth1／真实MySQL本地身份归属、API-O01／02。真实Facebook授权仍缺测试应用、clientid／secret、登记回调及账户；受控服务不等于供应商实机，也不提升SSO状态。

本批输入SHA与隔离源码一致，保留accepted-overlay.tgz、固定PHP依赖源码／包、上游源码及成功／失败日志；核归属清理闲置source／target／root-main，保留共享缓存／镜像及全部卷，见facebook-cleanup.json。本地核范围提交，不推送，继续开发。
