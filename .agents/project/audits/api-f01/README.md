# API-F01 Fortify

## 当前范围

**首批双因素 Provider 与 Features 基础已实现并通过定向验收；API-F01 整单元尚未闭合。**

本批提供独立 `openrust-fortify` crate、对象安全双因素 Provider 契约、Google2FA 引擎、缓存防重放和 Features 查询。根应用增加可选 `fortify` Cargo feature；尚未启用自动发现、注册 Fortify HTTP 路由或改变安装器的未就绪拒绝。`two_factor_http` 是实际内核／TCP 上的 Provider 验收夹具，不是已交付登录挑战控制器。

注册、登录／登出、密码重置／确认、邮件验证、账户双因素管理、恢复码、认证流水线、动作及响应定制继续开发。Passkeys 为独立 API-F02；Fortify 1.43.0 上游默认包含该项，未实现前不能将默认完整安装称为已通过。三语 Fortify 正文继续保留占位，完整行为验收后一起填充。

## 固定参考

- Laravel Fortify **1.43.0**，提交 `3db382a3d5d6a0d537de32a190ce3b945e621713`。
- pragmarx/google2fa **9.0.0**，提交 `e6bc62dd6ae83acc475f57912e27466019a1f2cf`。
- paragonie/constant_time_encoding **3.1.5**，提交 `ceed38ba49d98b85808edfc4f04750ff61eb9739`；仅用于 PHP 对照，不作为 Rust 依赖。
- Laravel Framework／文档沿用 [固定基线](../../plans/laravel-rebuild-baseline.md)，未跟随浮动分支。
- 旧版复用参考 `v0.x:vendor/openrust/framework/src/auth/fortify/engine.rs` 及 `engine/tests.rs`；复用其 Base32／HMAC、窗口与缓存机制，接入主线契约、AWS-LC、Config 和 Cache Repository。旧版账户 authority／令牌机制未直接移植；须按当前上游、原生 Guard 和模型逐项适配。
- 三个归档及逐文件 SHA 固定在 [fixtures](fixtures/)。保留 Fortify／Google2FA MIT 许可；HMAC 和随机数采用既有 AWS-LC 依赖。

## 行为与 Rust 适配

- SHA1、六位、30 秒、默认窗口 1；按最早候选依次匹配。计数器保留 Google2FA 的零高 32 位及低 32 位截断行为，负时间与跨 32 位边界均入对照。
- secret 字节长度不足 8 先报长度错误；之后为 Base32 字符、2 的幂长度、最少 16 字符的顺序。允许小写，无效验证码不跳过 secret 检查。负窗口或缓存下界使候选区间为空时不检查 secret。
- 缓存键为 `fortify.2fa_codes.` 加验证码 MD5；保留 get→verify→put 顺序。有旧时间戳则写入匹配步数；无旧时间戳则成功后再次读取时钟。默认过期 60 秒，窗口 2 为 120 秒。缓存错误传播，put 返回 false 与 PHP 一样不改变验证成功；不宣称此 get／put 协议是并发原子消费。
- 配置仅整数更新引擎窗口，其余类型保留此前值。Rust 对数值溢出、分配／熵源错误和畸形缓存时间戳显式失败；缓存数字字符串及字节值不隐式转成时间戳。这是受信缓存损坏时的失败关闭适配，不声称与 PHP 的动态标量强转等价。
- `generateSecretKey(length)` 显式传入 PHP 可选长度；Fortify 调用方默认将使用 16。QR URI 保留百分号编码与原始 secret 参数；本批不生成 QR SVG。
- Features 复用框架的 PHP 字符串／标量宽松比较，兼容关联数组值；选项必须严格为 bool true 且功能已开启。错误 feature 列表用 `Result<_, ConfigError>` 表达 PHP TypeError。应用级 Config 句柄避免进程全局串扰。
- Rust 无默认参数：无选项的 `twoFactorAuthentication()`／`passkeys()` 返回原名称；`twoFactorAuthenticationWithOptions`／`passkeysWithOptions` 对应非空 options 参数。空选项不清除既有配置，非空选项整体替换对应配置组。

## 验收

隔离源码基线 `9bf0a11fb` 加本批文件，证据 `/private/tmp/openrust-fortify-20261120/`。同期 Passport-04 在途修改不叠加进本批快照，不替代 V17 最终主线整合。

- PHP **8.5.12** 镜像固定 digest；直接执行原 Fortify Provider／Features、Google2FA 与 constant_time_encoding 源码。746 组 Provider 结果／错误／时钟调用次数／缓存调用及 TTL，对照 85 组 Features，每组 11 项查询；另有 3 个 QR URI 和 11 个生成长度用例。Rust 逐项一致。
- PHP harness 仅替代 config／optional、时钟和可观察 get／put 缓存，缓存接口是最小类型替身；没有执行完整 Illuminate Cache 实现或 PHP HTTP 栈，不将替身称为完整框架验收。
- 全新下载／重复生成逐字节一致；对照 JSON SHA256 `073ffb1ad92a48d77abe0f67bd9037cb44c1817ed3c7b228a87a5ea477489302`。原始源码添加注释的负样本被 SHA 校验拒绝。
- Rust 1.94.0 九个定向测试函数全部通过（Features 4，Provider 4，实际 HTTP 1）。真实 ArrayStore 重放拒绝、动态配置持久窗口、畸形缓存与时间溢出；实际内核 TCP 204／422／500 与双应用缓存隔离通过。
- 全 workspace／all-targets，features `pail,passport,fortify` 严格 Clippy 和 `cargo +1.94.0 fmt --all --check` 通过。
- 本批不写 SQL；未借用数据库测试冒充登录／账户管理验收。根 main 二进制在临时 MySQL 8.4.13 环境下，实际 HTTP 500→awaited guest auth→Pail、模板恢复 200、超时会话清理、SIGINT 退出 0、端口释放和独立 Artisan inspire 均通过（`root-smoke.log`）。
- 未改三语站点源码，本批不重复构建无影响的文档站。后续 API-F01 完整验收包含三语正文、文档构建及浏览器。

复现：

```sh
python3 -B .agents/project/audits/api-f01/fixtures/prepare_two_factor.py /private/tmp/fortify-reference
CARGO_INCREMENTAL=0 cargo +1.94.0 test --locked -p openrust-fortify --tests
```

正式验收在独立 archive 工作目录执行，并使用专属 `CARGO_TARGET_DIR`。首次 md-5 格式化、范围写法、HTTP 夹具导入／错误构造与归档前缀问题均保留失败日志；修正后重新通过，不计初次失败为成功。源文件清单与快照比对见 `source-sha256.json`。本地提交限定本批范围，不推送。

## 首批收尾

14 份源码／测试／许可及 Cargo 输入与验收快照逐字节一致。已确认专属 target 无在用 Cargo／rustc／main 后直接清理，df 实际增加 4.973 GB，剩余 Available 61.354 GB（57.141 GiB）；全部 Docker 数据卷及共享 PHP 镜像／缓存保留，临时 MySQL 标签容器已回收。详见 [cleanup-foundation.json](cleanup-foundation.json)。源码、归档、SHA 与日志保留。
