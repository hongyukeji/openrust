# Fortify 双因素账户动作

## 当前范围

本批实现明确选择加入的 `TwoFactorAuthenticatable` Eloquent trait、RecoveryCode、四个账户动作，以及加密与恢复码生成器的应用级覆盖。**尚未接通正式双因素 HTTP 管理／挑战路由、动态模型装配和 QR SVG；API-F01 继续开发。** `two_factor_model` 中的 `/api/factors` 是测试夹具，不能作为已交付产品路由的证据。Provider 仍在启动时拒绝开启未完整接通的双因素功能。

基线为 `b725bf3ee`，隔离目录 `/private/tmp/openrust-fortify-model-20261120/`。此前 Session 批次见 [登录与密码确认](session-authentication.md)。本批核对 `v0.x:vendor/openrust/framework/src/auth/fortify/actions.rs` 的密钥／恢复码与加密处理；旧 authority／交易令牌机制没有替代固定 Fortify 1.43.0 的模型 forceFill→save→事件顺序。

## 行为与语言适配

- `EnableTwoFactorAuthentication::call(user, force)` 按 PHP empty 判断已有 secret；缺失时或强制时生成 secret 和八个恢复码，使用原生 Encrypter 的 PHP 字符串序列化加密，再 forceFill／save／派发事件。force 不清除既有确认时间，重复非强制启用不产生额外生成或事件。
- secret-length 复用框架原生 PHP `(int)` 转换，默认 16；负数不能表达为 Rust usize 时明确失败。引擎继续保留首批 Provider 的错误行为。
- Confirm 动作按原生 Provider 验证，空字符串或 `"0"` 不调用引擎；失败使用 code 字段与 `confirmTwoFactorAuthentication` 错误包，成功按原生应用时钟写入确认时间。补齐 Model 的 `getDateFormat()`，沿用已有 metadata 储存格式。
- Disable 按三个字段的非 null 状态决定是否保存／发事件，并按 confirm 配置或原确认时间决定是否清除 confirmed_at；重复全 null 操作不保存、不重复事件。
- 恢复码默认使用原生 `Str::random(10)` 两段与连字符。自定义生成器为应用持有的强类型回调；支持清除覆盖，应用之间不共享。八项编码保留 PHP 默认 JSON 的斜杠及 UTF-16 转义。
- `replaceRecoveryCode` 保留 PHP 的整段明文 JSON `str_replace`，包括替换所有出现以及原始 Unicode 字符串不能匹配已转义 JSON 的行为。保留外层加密器解析、生成替代码、内层解密器解析与保存顺序。没有宣称该 read→replace→save 是原子恢复码消费。
- 复用原生 CBC／GCM、密钥轮换、MAC 验证和 PHP 字符串序列化。`Fortify::encryptUsing(Some/None)` 对应显式覆盖／清除；默认从当前应用容器解析原生 Encrypter。原生模型没有 PHP 进程全局 `Model::$encrypter`；本批不引入进程全局状态或伪造该回退。
- `TwoFactorAuthenticatable` 采用 Rust trait 显式 opt-in，模型方法传入所属 Fortify 依赖；PHP `__invoke` 对应泛型 `call`。RecoveryCodes 返回 `Vec<String>`：生成器限定返回 String；损坏密文、非法 UTF-8／JSON、非字符串列表或非 Text／Null 属性显式失败，不声称兼容 PHP 对任意损坏字段的弱类型返回。
- 事件使用原生 EventContext 借用已保存模型，保留原对象地址检查；事件字段持有 AuthIdentifier，`user::<T>(context)` 提供同步原模型。`RecoveryCodeReplaced.code` 使用 Zeroizing，Debug 不输出恢复码。明文 secret、解密载荷与生成缓冲区同样在使用后清零。

## 验收

- 直接运行固定原版 Enable／Confirm／Disable／GenerateNewRecoveryCodes、TwoFactorAuthenticatable 与 RecoveryCode 源码。13 步生命周期与 Rust 实际 TCP／MySQL **8.4.13** 对照一致：状态、重复操作、错误与正确 OTP、替换、重生、force、禁用及 Unicode／斜杠／反斜杠转义；恢复码 JSON 原始明文也逐字节对照。
- PHP harness 替代 Collection、配置／时钟、事件、模型和加密边界；模型 save 成功，Provider 采用固定 secret／已知 RFC 验证码。不将其称为完整 PHP Eloquent／HTTP／加密验收。Rust 一侧实际使用原生 Eloquent／MySQL、Encrypter 和 AWS-LC TOTP；加密列不保存明文，字符串主键 `001` 与原模型事件引用通过。
- 对照结果重复生成一致，SHA256 `8416d4cc1b1a9ab4d087078affa5887b4eb6ef7dcec5c5ca71ac27ad09337e4e`。七份原始输入逐文件 SHA 校验；原动作源码添加注释的负样本在 PHP 执行前被拒绝。
- 两个独立测试覆盖应用间密钥／生成器隔离、清除覆盖、严格功能选项、Text／Null 属性、非法 JSON／密文，以及加密失败时不修改模型、secret 加密失败不生成恢复码、恢复码加密失败保持八次生成调用等顺序。
- 本批共 3 个新函数；整个 Fortify 29 个不同测试函数和原生认证／Web 验证／事件的 12 项回归全部通过。SQL 测试只使用 MySQL 8.4，不运行 SQLite；不把未提供连接而返回的测试计为执行成功。
- Rust 1.94.0 全 workspace／all-targets、features `pail,passport,fortify`、`-D warnings` Clippy 及 fmt 通过；实际根 main HTTP→Pail、恢复、超时清理、SIGINT／端口释放及独立 Artisan 通过。命令与日志位于 `regressions.py`／`regressions.log`。
- 本批未改三语产品正文，完整 API-F01 交付时一起补齐／构建／浏览器验收。

```sh
python3 -B .agents/project/audits/api-f01/fixtures/prepare_two_factor_model.py \
  /private/tmp/openrust-fortify-20261120/upstream /private/tmp/two-factor-model.json
CARGO_INCREMENTAL=0 python3 -B tests/mysql_acceptance.py -- \
  cargo +1.94.0 test --locked -p openrust-fortify --test two_factor_model
```

正式 Cargo 在上述独立 source 工作目录执行，并显式设置该目录专属 CARGO_TARGET_DIR。编译阶段夹具遗漏 AuthError 导入、错误调用 Route::post 和捕获变量的 ConfigLoader 闭包均已修正；早期失败日志保留，正式结果重新验收。源码一致性见 [source-model-sha256.json](source-model-sha256.json)，清理结果见 [cleanup-model.json](cleanup-model.json)。

42 份源码／Cargo／对照输入与验收快照 SHA 一致。专属闲置 target 已直接清理（逻辑 5.899 GB，df 实际增加 5.056 GB），清理后 Available 62.365 GB（58.082 GiB）；全部数据卷、共享缓存／镜像、源码和日志保留，临时 MySQL 标签容器已回收。
