# API-F01 密码重置 HTTP

2026-11-20。密码重置正式路由和应用动作已实现，全部定向、回归、严格检查与根入口验收通过。API-F01 继续安装和三语整体验收；Passkeys 为独立 API-F02，默认启用该未实现功能仍明确拒绝启动。

## 来源与实现

固定 Fortify 1.43.0 `3db382a3d5d6a0d537de32a190ce3b945e621713`：SendPasswordResetLinkRequest、PasswordResetLinkController、NewPasswordController、CompletePasswordReset、四种结果响应、两种视图响应和 routes/routes.php，文件 SHA 沿用 upstream-manifest.json。查阅本地 v0.x 的 auth/flows.rs 和 auth/fortify 路径：旧验证码、authority 和额外凭据撤销协议不适合当前 Laravel broker 契约，未复制。

- password.request／password.reset 视图与 password.email／password.update POST；原生 guest Guard、web／CSRF、Features、views、prefix 和嵌套 paths。默认没有额外 HTTP throttle；原生密码 broker 按命名配置处理签发限速。
- 发链接 FormRequest 先 required|email 验证，再按 fortify.email 字段转小写；重置控制器先转小写，再验证 token／email／password。邮箱字段与 username 独立；仅邮箱进入用户查询，不将 password_confirmation／extra 当 SQL 条件。保留请求全部输入供应用动作验证。
- 应用可通过 resetUserPasswordsUsing 或容器 ResetsUserPasswords 替换动作，收到 broker 实际可变用户；密码哈希和策略仍属于应用。registerPasswordReset::<T>() 注册 Model + CanResetPassword 的保存适配，完成动作轮换 60 字符 remember token、save，再派发原生 PasswordReset；不自动登录。保存 false 返回值不升级为异常，沿用上游顺序。
- callback 通过请求局部错误槽保留应用原 HttpError，让 native broker 先释放失败 claim，再恢复 ValidationException 422／Web 重定向或原始 500；未将应用验证错误降格为 backend 500。原生 claim／finish 保持并发兑换只成功一次，区别于 PHP broker 的非原子检查／删除；同一请求仅有一个 callback，错误槽不跨请求或应用共享。
- 四种结果响应可从容器替换。Rust 将 PasswordResetStatus 显式传入 PasswordResponse::toResponse，替代 PHP 容器的 status 构造参数。默认 JSON 成功 200 + 翻译消息；Web 成功 back／login 或配置重定向 + status；失败保留固定 PHP 的字面量 email 错误键和 only(email) 闪存，即使 fortify.email 改为 contact。views=false 时成功重置回退 home。
- 数字／布尔 token 通过既有 PHP 浮点格式桥按标量转为字符串；不可转字符串的数组／对象保持错误路径。新密码动作仍收到原始输入并执行自己的策略。原生 PasswordReset 事件载荷是 AuthIdentifier，而非 PHP 用户对象，未宣称载荷形状相同。

## 验收证据

独立 archive 基线 `179d3f07af91689c91667c3b791d2ef16feb40f7`，源码、编译和验收日志 `/private/tmp/openrust-fortify-reset-20261120/`。后续 Socialite 等提交保留，未叠入本批；最终主线整合仍需 V17。

- 6 个新增真实 TCP／MySQL 8.4.13 函数：无效字段与类型、原生 FormRequest 时序、命名 broker／默认 broker 隔离、发送／限流／令牌消费、remember 轮换、原密码失效和保持访客 Session；应用验证／模型保存失败重试；前缀／paths／视图、CSRF／guest；19 组 PHP 控制器／响应对照；事件失败保留已存密码和 remember token、并发只成功一次；views=false／功能关闭／容器响应替换及应用隔离。
- [原 PHP 夹具](fixtures/password_reset.php)实际执行两控制器、CompletePasswordReset、四响应和 SendPasswordResetLinkRequest 源文件。Request、broker、用户／save、应用动作、Guard、翻译和事件为边界替代；不运行 PHP HTTP、SQL、哈希或原生 Laravel broker。19 组 JSON／Web、五种 broker 状态、配置邮箱字段、应用拒绝及重定向，与 Rust 实际 HTTP 响应／闪存和成功事件比较；Rust 用真实 broker 令牌替换 PHP 边界的固定 token123，URL host 按实际 TCP 地址归一。应用验证异常 Web 302 由原生异常渲染器处理，PHP 边界捕获仅记录 422 异常本身。
- 固定 PHP 8.5.12 镜像；输出 SHA256 `2d37cde0817ee91068a1ba58e30f54e07c99d4ae798cf671e22e0932cd6ac9be`，重复一致、源码负样本拒绝。Rust 夹具显式加载固定英文 passwords 翻译，避免将无 lang 文件时的键名误当消息正文。
- 首轮翻译资源、MySQL prepared trigger 限制、请求 host 和 views=false 下 GET/POST 同路径 405 的断言修正保留日志；修正后六函数通过。未把 405 称为视图注册成功。
- 通知测试记录 getEmailForPasswordReset 与令牌，不冒称 SMTP 投递；沿用原生通知契约。SMTP、其他数据库、Redis broker 和完整发布矩阵本批未运行。

- 完整 Fortify 49 个不同函数、原生 16 项回归（既有 12＋密码 broker 1＋email 3）通过。Rust 1.94.0 workspace／all-targets features pail,passport,fortify 严格 Clippy、全仓格式、根 main 构建及真实 HTTP／Pail／恢复／超时清理／SIGINT／端口释放／独立 Artisan 均通过（regressions.log）。最终补充 views=false 默认 home 分支，同函数、严格 Clippy 与格式复验通过（final.log）。
- 53 份输入与快照逐字节及 SHA 核对一致；见 source-sha256-password-reset.json。专属 target 闲置核对后清理，释放 Available 6,658,556 KiB，剩余 89,828,468 KiB；全部 Docker 数据卷前后一致，源码和日志保留；见 cleanup-password-reset.json。
