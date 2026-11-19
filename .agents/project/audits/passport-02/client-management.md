# Passport-02 客户端命令、关系与撤销

2026-11-20，接续[客户端工厂](client-factory.md)，固定 Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的 Client、Token、RefreshToken、AuthCode、ClientRepository::delete 与 Console/ClientCommand。原文副本／SHA 保存在 `/private/tmp/openrust-passport-completion-20261120/upstream` 与[输入清单](management-source-sha256.json)。旧版模型／关系实现已在前序批次核对；本批复用主线 Eloquent、事件、查询分批与异步属性转换，不引回旧版 JSON 模型快照。

## 实现与语言适配

- 增加真实 Token、RefreshToken、AuthCode 模型及字符串主键元数据。三者非自增，revoked／expires_at 转换按固定原文；仅 Token 开启时间戳。Token::can/cant 保留空 scopes、PHP 宽松星号通配及应用内 scope 继承。空 JSON 标量按 PHP empty 处理，非空标量仍由数组转换拒绝。
- Client 提供 tokens／authCodes／owner／user；Token 提供 client／refreshToken／user；RefreshToken 提供 accessToken；AuthCode 提供 client。关系指向模型主键，不能误用 URL 路由键。Rust 显式绑定返回模型类型，自定义类型使用 tokensFor／clientFor／refreshTokenFor／accessTokenFor；多态通过现有 MorphTo::target。Token::user::<U>(静态认证标识列) 校验 U::get_auth_identifier_name，Client::user::<U>() 使用主键。由调用者绑定编译 provider 类型，未实现 PHP 类名字符串自动实例化。
- Passport::useTokenModel/useRefreshTokenModel 使用应用内共享注册；已解析仓储也能读取后续注册。delete 按实际 Token 模型主键排序，每批 1000 个，批量水合实际 RefreshToken 模型；逐个 refresh→token，最后 client。保存经过各模型的转换／事件。错误传播，保留之前成功的保存；事件取消产生 false 不停止链，不偷偷增加事务。AuthCode 不撤销。
- clientUuids 默认 true，构造 Client 时保存开关。已有自增表可在构造前关闭，实例不会因随后改开关改变保存策略。默认发布的 UUID 迁移不变。核心增加实例唯一键／自增钩子；默认 usesUniqueIds 行为仍委托原静态钩子。
- 核心关系连接按 related.resolveConnectionName 解析，已绑定父模型在所属应用的同步 Facade 上下文中解析；不同命名连接从其所属 DatabaseManager 取得。绑定模型比较真实连接来源；未绑定模型身份比较保留无需数据库的原契约。新增内部同步作用域，不设置进程全局应用。
- passport:client 六分支与优先序 personal→password→client→implicit→device→授权码。名称／provider／逗号回调、public、no-interaction、设备确认、成功／ID／秘密输出按原职责实现。未配置 api provider 时，非交互选择保留 null，不擅自选择第一个 provider。个人分支只输出成功。数值客户端 ID 输出普通数字。现有 Output 尚无 Laravel 两列组件，布局使用现有 info/comment/writeRaw；提示使用已有锁定 console 0.16.6，交互支持输入、选择、确认与 Ctrl+C，未冒充逐字 ANSI／屏幕布局一致。

## 验收

隔离源码基于 `2079b5333b5eb07feae2f3f2dad7b3d9d33b981c`，加 24 份本批输入。未叠加协作者 Blade／Pail／Log 在途工作；后续 HEAD 的 Blade 提交不计入本批测试。保存 accepted-overlay.tgz、输入 SHA、脚本、日志。Cargo.lock 仅给 Passport 增加已锁定 console 依赖，无版本升级。

- 最低 Rust 1.94.0：Passport client 9、client_lookup 12、client_management 10、lifecycle 10、mysql 1，以及核心模型持久化 1、关系 1，共 **44 个不同测试函数通过**。使用项目 MySQL 包装器，实际 MySQL **8.4.13**；核心 Eloquent／Relation 环境开关均显式设置，未执行 SQLite。最终统一日志 accepted.log。
- 新增覆盖真实 TCP HTTP 撤销顺序、刷新错误、token 错误、取消保存继续链、自定义 Token／Refresh 模型及 main/oauth 连接、跨应用已绑定关系、owner 主键与认证标识区别、运行时 UUID 和数字表的 query／仓储／工厂创建，以及 **1001 个令牌跨分批边界**。
- 实际 Console Kernel 创建六类客户端、组合选项优先序、默认名称／回调、多个回调、公开／保密／个人输出、无默认 API provider；字符串 key 的 foreignIdFor 实际 MySQL 列为 char(36)。
- 根实际二进制重建，HTTP `/`、`/up`、API／Accept JSON／Web 404、Artisan inspire／Passport 帮助、原始 ./artisan inspire、SIGINT 与端口释放通过。真实 PTY 创建公开授权码和保密密码客户端，检查 provider 选择、回调默认值、设备确认、秘密输出；Ctrl+C 非成功退出，数据库行数不变。日志不输出生成秘密。
- cargo +1.94.0 fmt --all --check、完整 workspace/all-targets/features pail,passport/locked 严格 Clippy 通过。
- 三语构建、`/` 与 `/docs/` 桌面／移动 12 项浏览器检查、Passport 页面六项内容检查通过；保留既有 bundle 大小提示。

初轮新增夹具错误（缺参数／所有权、应用上下文、MySQL unsigned 断言）与核心关系身份比较回归均保留失败日志，修正后重新验收。PTY 脚本的 macOS EOF 读取空字节循环已修正；内容脚本最初误计 8 个二级标题，按三语真实 7 个标题修正。没有忽略或降低产品断言。

## 尚未闭合

本批未运行新的 PHP 数据库／命令运行时 oracle，不把固定原文核对冒充 PHP 执行。精确 Scope JSON options／Unicode／斜杠字节规则、secret setter 失败顺序、浮点字符串转换和其余数组 oracle 仍待；完整 Passport-02／V06 保持部分验收，不进入 Passport-03。Windows 原生终端与完整 OAuth2 协议仍不在本批通过范围。仅本地核定提交，不推送。
