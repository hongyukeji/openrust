# API-F01 current_password 安装前置

2026-11-20。原生 `current_password`／`current_password:guard` 与 `Rule::currentPassword` 实现及定向验收完成；继续 Fortify 安装、默认动作和三语整簇验收。

## 来源与适配

固定 Framework 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 ValidatesAttributes::validateCurrentPassword、AuthManager、BcryptHasher／AbstractHasher、输入转换中间件；文件 SHA 及完整验证器运行时见 fixtures/current-password-manifest.json 和 password-rule-manifest.json。查阅本地 v0.x validation.rs／tests.rs 及 auth/fortify/actions.rs；旧动作协议不符合当前命名 Guard 规则，未复制。

- FormRequest 使用 `Validator::validateRequest(&mut Request)`，在规则实际执行时调用当前应用 AuthManager 的真实默认／指定 Guard 和原生 Hash；保留 shouldUse 覆盖。PHP 的空字符串／"0" Guard 参数回退默认，访客返回验证失败，未知 Guard／不可转换数组和哈希器异常保留执行错误。
- required／nullable／sometimes／bail 沿原生验证器时序，跳过的规则不查询 Guard。原生 HTTP 默认空字符串转 null；密码字段不受 TrimStrings 修剪，普通验证规则仍使用 PHP trim 判定空白。
- Rust 的动作接口显式传用户、无 PHP Request 容器单例，因此非 HTTP 验证通过每个 Validator 的 `setUserResolver` 明确提供命名用户；真实 HTTP 调用 validateRequest 优先解析该请求的 Guard。不持有全局用户、密码或共享请求快照。缺少请求和解析器时明确返回 Execution，不静默查询其他身份。用户解析器必须来自应用可信认证上下文。
- PHP password_verify 将 null 转空串并产生弃用诊断，Rust 对应原生标量转换，无 PHP 弃用诊断；错误的哈希算法沿原生 Hash 错误传播，未当作普通错误密码。保留 null／bool／数字语义，真正的空密码哈希与错误密码区分。

## 验收证据

独立 archive 基线 `8498d530d34ec82b98d13f6f6bfbab1eb615ad07`，源码、日志与固定源码保留在 `/private/tmp/openrust-current-password-20261120/`；完整 PHP 运行时复用上一批保留目录，不属于可删编译产物。

- 原 PHP 完整 Validation／翻译、原 AuthManager::guard 与 BcryptHasher／AbstractHasher、原 ConvertEmptyStringsToNull::transform 运行；Guard resolve 和用户为边界替代，不冒称 PHP HTTP／Session／SQL。468 组涵盖三种真实 bcrypt 密码、默认／指定／不存在／空／"0" Guard、访客、类型与异常。PHP 弃用消息单独记录；输出 SHA256 `ab44ab0c3abe50b5124d6c038f1144e96a753295d75e4e3402c2d955dfcd6116`，重复一致，源码负样本拒绝。
- 两个新增函数：468 组真实 TCP／原生 RequestGuard／Hash／FormRequest，20 个并发请求的默认 Guard 隔离、显式用户解析器、Bail／nullable／required；无请求／无解析器明确 Execution、访客消息及 fluent 规则。初轮空字符串对照未包含 HTTP 输入转换，修正夹具后通过；未修改产品以迁就原始输入夹具。
- 验证器32个不同函数、Fortify49个不同函数在MySQL8.4.13下通过；Rust1.94 workspace／all-targets features pail,passport,fortify严格Clippy、全仓fmt、根main构建及真实HTTP／Pail／恢复／超时清理／SIGINT／端口释放／独立Artisan全部通过（regressions.log）。
- 本批未执行 SQLite、其他数据库、外部密码服务或完整发布矩阵；7份输入与快照逐字节及SHA核对一致，专属target闲置核对后删除，Available释放6,481,412 KiB、剩余92,313,484 KiB；源码／日志及全部Docker卷保留。见source-sha256-current-password.json／cleanup-current-password.json。
