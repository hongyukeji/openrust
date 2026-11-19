# Passport-02 契约对照与 HTTP 管理验收

2026-11-20，接续 `ecc607f2b` 的[命令、关系与撤销](client-management.md)，完成该单元留下的 PHP 对照和 HTTP 管理验收。固定 Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 的实际 Client::secret、Scope::toJson；原文和[SHA 清单](contracts-source-sha256.json)保存在本批证据目录 `/private/tmp/openrust-passport-oracle-20261120/`。没有进入 Passport-03 或开放 OAuth 协议端点。

## 实现

Client 的 secret 输入先按 PHP ?string 转换，plainSecret 在进入哈希前更新；哈希接受完全相同的转换结果。浮点使用核心已有 precision=14 格式化，通过隐藏扩展桥复用，不另建数字比较引擎。null 清除，非法数组在 typed setter 前拒绝；哈希错误保留原持久属性，但本次 plainSecret 已赋值。取消异步哈希也不会把赋值顺序推迟到哈希成功后。

Client fill／fillAsync 逐项赋值；先前成功的字段不因后续失败而回滚，未执行到 secret 时不修改 plainSecret。非 secret 属性继续委托已有 guards／casts，forceFill 仍保留现有 unguarded 生命周期。Rust Attributes 是项目既有 BTreeMap，按属性键顺序执行；并非 PHP 任意插入顺序的动态数组。三语正文明确这项语言适配。未改核心 Model 的其他批量赋值策略。

Scope::toJson 默认保持 id→description 顺序、Unicode／斜杠转义。新增 ScopeJsonOptions 和 toJsonWithOptions，覆盖四项 HEX、NUMERIC_CHECK、UNESCAPED_SLASHES、PRETTY_PRINT、UNESCAPED_UNICODE、PRESERVE_ZERO_FRACTION、UNESCAPED_LINE_TERMINATORS；可位或和 fromBits。它专门编码 Scope 的两个 UTF-8 字符串字段，不作为通用 PHP 对象序列化器。数字检查复用核心同一 PHP numeric parser；短浮点数字复用 serde 的 round-trip digits，按固定 PHP JSON 阈值和指数／零分数规则布局。Rust 字符串不能承载非法 UTF-8，对应容错／抛错选项无可触发输入。

## 固定 PHP 运行时对照

[生成器](../../../../vendor/openrust/passport/tests/reference/client_contracts.php)校验三个源码 SHA，再 require 原始 Client、Scope 和 ResolvesInheritedScopes。使用固定 `php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9`（实际 PHP 8.5.12），无网络、只读源挂载。

- **538 条 secret**：固定 bit 输入覆盖 512 个确定性浮点及正负零、指数布局、最大值／次正规值、INF／NAN，另含 null／bool／整数／字符串／数组。加载真实 PHP setter 闭包，测试壳只提供 Attribute 容器和可控的哈希失败边界，观察 plainSecret、是否进入哈希和异常状态；没有复制 setter 算法。
- **1729 条 Scope JSON**：真实 Scope 方法运行 19 种选项／组合，覆盖 Unicode、代理对、控制符、斜杠、HTML 字符、行分隔符、整数边界／溢出、负零、指数及数字字符串。对照完整 JSON 字节，不只比较解析后的对象。
- **432 条数组**：真实 PHP in_array 和 JSON 载体，覆盖异质标量、嵌套、关联顺序、数字键、整数溢出、空白和 Unicode。Rust 使用已有 PhpArray 和唯一核心比较桥。

[固化结果](../../../../vendor/openrust/passport/tests/fixtures/client_contracts.json)共 2699 条，不将重复或组合视作不同测试函数。独立重复生成字节一致；修改 Scope 源码 SHA 的负样本被拒绝。PHP 8.5 对 NAN→string 的一条 warning 单独保留在 fixture，Rust 对照其字符串和状态，不声称复现 PHP warning 通道。该运行时夹具不加载完整 PHP Eloquent、数据库或密码学哈希实现；不能替代完整 PHP ORM／跨语言数据库 oracle。

## 实际入口验收

- 最低 Rust 1.94.0：client 9、client_contracts 3、client_lookup 12、client_management 11、lifecycle 10、mysql 1，以及核心持久化／关系各 1，共 **48 个不同函数通过**。统一 accepted.log；格式收尾后对三个契约及 11 个管理用例再次定向复验，final-targets.log。MySQL 包装器实际版本 **8.4.13**，相关 REQUIRE 与 URL 开关显式启用，未执行 SQLite。
- 所有 secret 对照通过真实 TCP→HTTP Kernel 在所属应用内执行；13 条输入实际异步哈希并校验哈希接受的字节，覆盖非有限值。补批量赋值前后错误、旧哈希保留及 plainSecret 顺序。
- 应用自定义夹具路由用真实 Auth::onceBasic、Eloquent UserProvider 和 MySQL 用户表认证，调用公开仓储完成 HTTP 创建→隐藏秘密重读→更新回调→轮换→撤销→再次未命中。认证标识 subject 与模型主键不同；两个用户、两个应用、相同用户 ID／凭据跨应用、未认证／错误密码、错误 UUID、越权读取／更新／轮换／删除均拒绝。真实保存后重读验证新秘密、旧秘密拒绝及访问令牌撤销。夹具路由只存在测试中，不冒充包内生产管理端点。
- 根实际二进制重建；根 HTTP、API／Accept JSON／Web 404、Artisan inspire／Passport 帮助、原始 ./artisan inspire、SIGINT 和端口释放通过。客户端命令的 PTY／取消和 1001 令牌边界由上一批实际验收，本批完整管理回归再覆盖分批撤销。
- 整 workspace／all-targets／features pail,passport／locked 严格 Clippy、隔离与工作区格式检查通过。三语文档构建、12 项根／子目录桌面／移动浏览器检查，以及 6 项 Passport 正文检查通过。

隔离基线为 `ecc607f2b`，13 份本批源码／测试／三语正文逐份 SHA 一致。协作者后续 Pail／Log 和在途测试不叠加到该验收。初轮测试路由误用了 Router 不提供的 post／patch／delete 简写，按已有 match API 修正；Clippy 的引用模式及格式复制时机问题已修正并复验，失败日志保留。无依赖版本／Cargo 变化。

## 单元结论

Passport-02 客户端与仓储的计划范围已完成实现及上述实际验收；各批语言适配继续有效，不宣称 PHP 动态类加载、任意 PHP 字节字符串或所有运行时 warning 相同。Passport-03 及后续 OAuth2／Guard／设备／PAT／安装／自定义整合仍未交付，V06 保持部分验收。发布前 Windows、其他数据库和完整安全／依赖公告继续留在原账本。本批仅核定本地提交，不推送。

## 产物清理

确认本批无活动构建／服务／验收容器后，逐份核对 13 份工作区／隔离源码及 accepted-overlay.tgz 的 SHA，直接删除专属 source／target。逻辑字节分别 92,275,538 与 5,669,159,313；df 可用实际增加 5,563,273,216 字节，清理后约 68.979 GB。固定 PHP 镜像为本批新拉取、无容器引用，已按精确摘要移除；没有清理共享镜像／缓存或数据卷。所有源码覆盖包、固定上游、生成脚本、结果及日志保留，详见本地 cleanup.json。
