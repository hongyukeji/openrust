# API-A01 用户认证入门套件

## 范围与状态

执行基线为[正式计划](../../plans/user-auth-starter-kit.md)及其S01至S10契约。2026-11-25用户授权持续实现A01-01至A01-05，遇到实现问题按固定基线与既有契约裁决；实测通过后分批本地提交，不推送。范围和完成门禁没有缩减。

| 批次 | 状态 | 剩余工作 |
| --- | --- | --- |
| A01-01 安装与基础认证 | 开发中 | 显式安装预设、基础账户流程与响应适配、安装保护、真实Artisan／HTTP。 |
| A01-02 邮件与生命周期 | 未闭合 | 邮件落地、资料变更、跨进程凭据撤销与失败恢复。 |
| A01-03 账户中心 | 未闭合 | Token客户端协议、重新认证、头像、设备与注销。 |
| A01-04 网页与可选认证 | 未闭合 | 完整Blade页面、2FA／Passkeys／第三方登录接线及三语教程。 |
| A01-05 组合验收 | 未闭合 | 两种预设、三库／缓存、常用系统与最终支持矩阵。 |

## 首个实现单元：响应与安全资源

沿用固定Fortify 1.43.0的[LoginResponse](https://github.com/laravel/fortify/blob/v1.43.0/src/Http/Responses/LoginResponse.php)与[RegisterResponse](https://github.com/laravel/fortify/blob/v1.43.0/src/Http/Responses/RegisterResponse.php)：HTML依然委托原生响应，业务JSON仅由显式套件Provider替换。未将普通Fortify默认输出全局改写，也不因为存在响应绑定而声称安装预设可用。

旧版复用检查：`openrust-v0.x/vendor/openrust/framework/src/auth/fortify/api_http.rs`以及`tests/server/auth/fortify.rs`。旧JWT登录、挑战和刷新体系不符合本计划的Sanctum PAT／Session分工，未直接搬入；借鉴秘密不入URL与认证响应禁止缓存的边界，复用当前ApiResponse、Fortify响应契约和Auth上下文。

默认UserResource只读取id、name、email、email_verified_at，保留字符串主键；不读取整个模型、密码、remember token及因素秘密。自定义资源通过既有响应绑定替换。

### 本地验收（2026-11-25）

所有Cargo命令由`tests/build_guard.py --workflow user-starter`运行于`target/tmp/user-starter/source`冻结快照、Rust 1.94.0；不纳入其他对话未提交的Artisan输入修改。

- `cargo +1.94.0 test -p openrust-fortify --test starter_responses --locked`：8项通过、0忽略。共20次HTTP请求，其中3次经过回环TCP；覆盖JSON成功／未认证、原生HTML重定向、204零正文、密码重置响应与字段错误、三语消息及应用隔离。默认根User模型、时间属性转换及字符串身份均已实测。此处验证响应契约，不冒充完整注册／重置或数据库持久化流程通过。
- `cargo +1.94.0 clippy --workspace --all-targets --locked -- -D warnings`与工作区格式检查通过；最终日志为`workspace-clippy-final.log`及`fmt-final.log`。
- 三语文档构建通过，既有全站浏览器检查与新增三语文章12组检查通过，覆盖`/`及`/docs/`、手机／桌面、公开类型与锚点、无横向溢出。新增页面日志`article-browser.log`，全站`browser-second.log`，构建`docs.log`。首次默认浏览器版本不可用的失败保留；随后显式使用本机Chromium1243通过，未下载额外浏览器。
- 构建中32GB停线自动安全清理释放82,322,956,288字节，清理后可用114,121,220,096字节；保留源码、日志、依赖、Cargo锁原inode、数据库及数据卷。首次中断的workspace检查不计通过，恢复后重验通过。继续复用本工作流的缓存，不每批清空。

原始证据在`target/tmp/user-starter/`。源码SHA核对清单为`response-source-sha.json`。这一单元不包含安装命令、完整UI、邮件投递、令牌签发、数据库／缓存驱动矩阵或Windows实机；上述范围按批次继续实施，没有因本单元通过而关闭。
