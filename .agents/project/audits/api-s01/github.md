# API-S01 首批：共享 OAuth2 与 GitHub

2026-11-20：本批骨架、实现和本机验收完成；API-S01 整体未闭合，继续其他计划驱动、OAuth1／OIDC 路径及本地身份映射。控制供应商验收不等于真实供应商账户授权。

## 固定参考及旧版

Socialite `v5.33.0` 固定提交 `8032bb1fa9d74aae54438f216e4d45db90d27ea9`，SHA 见 upstream-sha256.json。首批实际核对 SocialiteManager、Contracts、AbstractUser、Two AbstractProvider／GithubProvider／User／Token。固定文档基线仍为项目计划 `960eedf0911155ad16326ade49c0ea5d18eda0b8`，没有随供应商目录扩大交付范围。

旧版 `v0.x`／`6d1a81924` 中没有 Socialite 目录，存在独立 auth/oidc.rs，留待 API-O01 核对，未复制为第三方登录。复用当前 HTTP Factory、应用容器／包发现、Session／原生 RedirectResponse 和 Eloquent 之外的远程身份职责。

源码隔离自 `2383fdbda8df68151f142c738deca31f5ae20735`。同期 `179d3f07a` 验证器与 Fortify WIP 未加入此验收快照。根 Cargo.lock 仅插入本包依赖与包记录，保留同期 idna 变动；没有增加工作区之前不存在的外部依赖版本。

## 实现及 Rust 映射

`vendor/openrust/socialite` 提供 SocialiteManager、SocialiteServiceProvider、facades::Socialite、contracts::Factory／Provider／User、two::AbstractProvider／GithubProvider／User／Token。可选根 socialite feature、自动包发现与 services.github 三环境变量接线；不自动添加业务路由、本地用户表或本地登录。

manager 属于应用；driver(name, &mut request)／with 返回借用该请求的 provider。请求体不是 Sync，因此异步 provider 方法借用 &mut self，维持真实 Send HTTP Future。用户缓存只活在该 provider 借用内，避免跨请求及应用共享 PHP 风格持久对象。配置／注册仍应用内隔离，扩展通过 ProviderDefinition 与 manager.extend；自定义配置逐次读取。相对 redirect 拼到合法 HTTP(S) app.url，与固定 Manager 对齐。

默认 Session state 随机40位字母数字，消费后恒时比较，保留 PHP empty 对空字符串与字符串0的行为；数组或非字符串回调拒绝，state 先于 token 交换消费。共享可选PKCE使用96位verifier、S256及Session消费；跳转和回调两次构造都须启用。stateless只关闭state，PKCE仍需Session。scopes去重合并、setScopes去重替换，参数按上游顺序合并；本批公开可选参数支持标量，嵌套参数留待后续核对。

Github使用固定授权／token／profile／email端点与GitHub Accept／token授权头，原生HTTP补充User-Agent。包含user:email才加载邮箱，挑选primary且verified项；邮箱失败返回None，其他profile／token失败传播。真实表单交换、refresh、userFromToken、raw及nodeId映射已提供。User id保留JSON类型；PHP扁平token字段原生分组到显式credentials，使用Zeroizing字符串且没有自动Serialize／Debug实现，不自动输出凭据。

共享HTTP客户端新增显式redactPayload()，同时省略请求快照query／Authorization／body和响应事件／录制的headers／body；真实传输与返回完整。保留旧内部redact_payload行为，既有SMS等调用不被扩大修改。beforeSending／afterResponse仍是显式处理真实传输的钩子。错误对外不显示供应商响应凭据。

## 实际验收

证据 `/private/tmp/openrust-socialite-20261120/`。Rust1.94.0，7项Socialite真实TCP／HTTP／双应用Session测试、HTTP Client全部14项回归及根纯配置1项，共22个不同函数通过；文档两个处理器作为独立integration target实际编译。配置函数虽读取多数据库配置值，不建立任何数据库连接。

覆盖真实token表单、已验证邮箱与email失败、远程用户完整映射、refresh、实例缓存边界、错误status／JSON／缺token／缺code、缺Session、自定义driver与配置隔离、相对redirect和非法app.url、敏感事件及录制正反边界。两应用真实Kernel／加密Session Cookie验证跨应用state失败、重放失败、PKCE challenge／96位verifier实际发送。固定PHP夹具7组state的首次与重放、5组scope／参数字段、真实Github方法的映射／邮箱失败／refresh／HTTP选项均保留。

PHP oracle 在固定 `php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9`（8.5.12）中实际require八份固定PHP源码，Request／Session／Arr／HTTP只提供宿主夹具，Probe只暴露protected方法，没有复制OAuth／映射算法。重复运行逐字一致；修改GithubProvider源码的负样本在加载前被SHA拒绝。宿主php7.4缺libtidy，未用于对照；使用现有共享Docker镜像，未删镜像或卷。

strict workspace／all-targets features pail,passport,socialite Clippy及fmt通过。三语8个章节、构建和根／docs挂载12项通用＋6项Socialite浏览器检查通过。最终feature根二进制包含发现的Socialite provider；MySQL8.4.13 wrapper下真实HTTP／默认错误响应／Artisan帮助／原始./artisan／SIGINT与端口释放通过。wrapper验证MySQL版本，本批Socialite流程本身不执行SQL；未冒称MySQL身份映射已验收。

失败证据保留：初始测试缺axum json feature／文档处理器未映射容器错误、测试MutexGuard跨await已修正；PKCE测试回调未启用同一开关已修；固定PHP对照揭示字符串0边界后修正并重验。根烟测误调用既有占位about导致失败，恢复正式已实现入口后通过，未修改about产品范围。

## 后续范围与收尾

API-S01只完成首批共享OAuth2与GitHub；Facebook／X／LinkedIn／Google／GitLab／Bitbucket／Slack及OAuth1／OIDC、真实MySQL本地用户归属、受控协议扩展继续开发。真实GitHub授权尚需测试OAuth应用、client id／secret、注册回调域名及账户，本批没有这些实机结果；没有将第三方token等同Passport／Sanctum或SSO。

本批核范围本地提交，不推送，保留同期Fortify／验证器改动。按归属核闲置后清专属source／target与root-main，保留源代码覆盖包／PHP固定源码／SHA／成功失败日志、共享缓存、镜像和全部数据库卷；磁盘与卷前后见cleanup.json。用户持续开发指令有效，批次结束后继续后继驱动。
