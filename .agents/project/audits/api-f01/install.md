# API-F01 安装与整簇验收

2026-11-20，API-F01 本机范围实现与验收完成。Passkeys 仍归 API-F02，不能据此宣称 Fortify 1.43.0 全部默认功能完成。

## 来源与范围

Fortify 1.43.0 `3db382a3d5d6a0d537de32a190ce3b945e621713`，Framework 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc`，文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8`。安装命令、发布资源、默认动作、Provider 与迁移按 [固定源码清单](fixtures/upstream-manifest.json) 核对；邮件模板及消息类另见 [固定清单](fixtures/mail-template-manifest.json)。本批没有执行完整 PHP 邮件渲染，邮件证据为固定源核对与原生实际 SMTP。

实现前查阅 `v0.x` 的 Fortify Provider／限流实现；旧安装器对应路径不存在。沿用主线发布、源码修改锁、迁移和验证器，未移植旧接口。此前注册、邮件验证、密码重置、密码规则、current_password 和异步限流的独立审计共同构成本簇证据。

## 实现与语言适配

- 自动发现只注册资源和 `fortify:install`，运行时 Provider 由安装器显式接入。避免未安装配置就触发默认 Passkeys 门禁。发布仍使用真实运行时 Provider 标识。
- 命令复用原生 `vendor:publish`、源码锁及原子替换。先检查布局，发布前释放锁，再加锁重新规划并修改；重复安装不重复迁移、不覆盖自定义动作。保留 CRLF 与文件权限。不自动编译或运行迁移。
- Rust 需要静态模块和 Provider 装配，安装器发布应用动作／配置，接入配置表、Provider、User Clone／双因素模型和敏感属性隐藏。未知布局在资源发布前失败并指明手工装配。
- 四种默认动作使用原生验证、Hash、Eloquent、通知和密码仓库；`Data::fromRequestValue` 保留数组键、嵌套上传与深度检查。密码更新通过新增有默认实现的 `updateFromRequest` 获得真实 Request，`current_password:web` 独立解析 web Guard。旧动作仍可实现原接口；默认安装动作缺少 Request 时明确失败，避免拿待更新用户代替 Guard。
- 默认 Provider 注册 login／two-factor／passkeys 命名异步限流。配置区分发布 stub 与包内部默认值；保留上游默认 Passkeys，但在 API-F02 完成前要求移除该 feature。2FA 迁移 up／down 已交付，Passkey 迁移未发布。
- 默认 User 会发送验证通知；安装后需启用邮件验证路由。无界面应用需提供命名 `password.reset` 前端路由。三语正文说明配置、关闭视图和编译步骤。
- 真实 SMTP 暴露严格 Blade 可选变量缺失：邮件 layout 的 mail_head、通知模板的 hasAction／regards 增加缺省回退。仍使用原生通知和 Markdown 邮件渲染。

## 验收证据

独立 `git archive` 基线 `7afe9bad0`，源码和日志位于 `/private/tmp/openrust-fortify-install-20261120/`。未叠加随后 Socialite OAuth1 提交，不替代最终主线 V17 整合。

- `native-final.log`：Rust 1.94.0、验证器 33 函数和 Fortify 54 函数通过。含安装 3、发布动作编译／独立 Guard 2、实际 multipart 转换 1；MySQL 8.4.13。
- 同日志：`cargo +1.94.0 clippy --offline --workspace --all-targets --locked --features pail,passport,fortify -- -D warnings`、全仓 fmt、原根 main 构建、真实 HTTP／Pail／独立 Artisan／SIGINT 全部通过。
- `generated-rollback.log`：`tests/fortify_install.py` 从原根二进制安装、重复安装与自定义保留，编辑无界面配置后只开 fortify 重建、MySQL 迁移、真实 HTTP 与三封 SMTP 邮件；包括 CSRF 419、验证 422、注册／邮箱规范化、Session／CSRF 轮换、签名验证、资料变更清验证、密码更新／确认、2FA／TOTP／恢复码／恢复码登录／停用、退出、密码重置失败重试及成功重放拒绝／不自动登录、旧新密码、用户名分桶限流和正常关闭。之后 `migrate:rollback --step=1` 再 migrate 成功。
- 同日志：两个原生邮件预览回归、生成应用 `clippy --bin main --features fortify -- -D warnings` 和 feature 关闭构建通过。此前 `generated-mail-fix.log` 也已通过完整流程，新增回滚后再验。
- `docs-build.log`：三语 VitePress 构建通过，保留既有大 chunk 提示。`docs-browser.log` 的根路径／子路径、三语桌面／移动 12 组合及 `docs-fortify.log` 的三语两路径 Fortify 内容／导航 6 组合通过。
- 运行统一使用专属 target、`CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`，MySQL 经 `tests/mysql_acceptance.py` 隔离。源码输入逐字节核对与 SHA 见 [清单](source-sha256-install.json)，产物回收见 [记录](cleanup-install.json)。

首次锁重入、验收夹具 DB Value 序列化和邮件可选变量缺失的失败日志／生成源码保留；修正后通过，不将早期失败日志视为全部通过。

限制：本机 macOS、MySQL 8.4.13；SMTP 使用临时回环服务器，未验外部服务 TLS／认证；跨平台／完整发布矩阵、Passkeys 与最终所有并行模块的主线整合继续。未推送。
