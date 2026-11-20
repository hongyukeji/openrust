# API-F02 自定义模型、AAGUID 与资源发布

2026-11-20。本批接续已提交的 Passkeys 生命周期，固定基线及真实硬件／多平台边界沿用 [生命周期记录](README.md)。源码以 `81cae68b4` 的 git archive 建立于 `/private/tmp/openrust-passkeys-models-20261120/source`，叠加本批输入；独占同批 target，Rust 1.94.0，增量关闭、3 个构建任务、dev/test debug=0。保留其他协作者的 plan、translation-database、core 和历史补丁，不推送。

## 对照与实现

- 继续使用 laravel/passkeys-server 0.3.0 `2e7f893429d35acff45ac16d05ddcf883372a20e`。对照 `Passkeys.php`、`Passkey.php`、`Concerns/PasskeyAuthenticatable.php`、`Support/Aaguids.php` 和资源／Provider。旧版检查沿用生命周期记录：不存在可直接迁入的同契约 WebAuthn 模型。
- PHP 的 `usePasskeyModel` 转为应用实例内注册泛型 `M: PasskeyModel`。`Store<M>` 使用真实 M 的查询、创建、保存、事务刷新与删除，保留表、主键、casts、访问器、隐藏／附加字段和观察者。`PasskeyRecord` 保存实际 M 的共享异步锁，不将属性复制进默认 Passkey 后保存。原生事件共享该记录，typed context 使用实际 M 的 Clone 快照，避免监听器访问记录造成重复持锁。授权回调与响应契约接受记录。
- 静态 Rust 无法在运行时改变 `HasMany<User, Passkey>` 的类型参数，因此用户关系适配为 `PasskeyRelation` 的 getResults／exists／create；不声称支持完整 HasMany 查询 builder。`PasskeyRecord` 的读取和序列化为异步，`model::<M>()` 可取得同一实际模型，调用其他记录方法前释放锁。`userFor::<U>()` 保留显式类型 BelongsTo；useUserModel／userModel 按应用配置用户类型。
- 固定上游 MIT AAGUID 名称与图标由 PHP 原资源导出为 JSON。名称和图标分别延迟缓存，flush 清除缓存但保留已借用 Arc；无效 theme 的错误文本与 PHP 相同。默认附加 authenticator，图标仅显式 append。已通过 core 验证后读取 authData 的 AAGUID，none attestation 下仅作为展示，不作为厂商认证证据。StoredCredential 缺少该字段时使用 serde default。
- 显式注册 PasskeysResourcesServiceProvider 即可通过原生 vendor:publish 的 passkeys-config／passkeys-migrations 标签发布；无须配置认证 Origin。完整 PasskeysServiceProvider 也登记同资源。不添加传递依赖的自动发现假设。发布的 Rust 模块由应用接入编译；Fortify 保持其原配置和迁移安装路径。
- 新增 direct serde_cbor_2 =0.13.0，已在旧锁中；tokio sync 从 dev 使用扩至常规依赖。Cargo.lock 无新增版本或旧版本删除。LICENSE.md 修正为普通文件权限。

## 验收证据

证据目录 `/private/tmp/openrust-passkeys-models-20261120/`，成功记录和失败源码均保留。核定输入逐字节对齐 live／source，见 [输入 SHA](customization-source-sha256.json)。

- `aaguids-oracle.json`：固定 PHP 镜像 `php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9` 无网络执行真实上游 Aaguids 类，[生成器](fixtures/aaguids.php) 导出 71 组名称、两主题图标 SHA、unknown／大小写／缺失与错误 theme，和测试向量逐字节相同。
- `custom-generated3.log`：统一 mysql_acceptance.py 启动 MySQL 8.4.13，真实生成应用、固定官方 JS 0.4.1、Chrome CTAP2。自定义表 custom_passkeys、主键 pk、uppercase setter、hidden credential、custom_marker、实际 User 动态关系／exists；注册／验证／删除三个 typed 事件和 Saved／Deleted 观察者。继承全部 CSRF／密码确认／跨账户／跨 Session／Session 与 CSRF 轮换、remember、挑战并发消费、服务端过期、10/11 限流、独立挑战同计数器行锁竞争和 FK 级联。完整原 Fortify HTTP／三封 SMTP、迁移回滚重建、严格生成应用 Clippy、关闭 feature 构建通过。
- `default-generated.log`：同样完整生成应用和真实浏览器流程，在默认 Passkey 模型下复验通过，包含生成应用严格 Clippy 和关闭 feature 构建。
- `docs-build.log`、`docs-browser.log`、`docs-fortify.log`：三语构建、根路径／子路径三语桌面／移动 12 组合，Fortify 内容／内部链接 6 组合通过；保留 11 个二级章节。

首轮生成应用失败为夹具使用错误的 ModelEvent 导出路径；第二轮关闭 feature 时夹具 events 路由缺少 cfg。修正后全流程重跑成功。一次误在 live 启动 Cargo，使用独占新 target，已精准中断（exit 130），不计正式验收；全部正式结果来自独立快照。

## 范围

本机 macOS／MySQL 8.4.13／Chrome 虚拟 CTAP2；并未新增真实硬件、Windows／Linux、RS256 硬件或云同步证据。自定义模型实测整数 pk 及不同表名，不冒称已验收任意 UUID 主键／自定义数据库连接。PHP credential JSON 不作为 Rust 存储交换格式。共享 Session／原子 cache 的部署要求沿用原记录。

## 最终验收与收尾

- `native.log` 的验证器 33 与 Fortify 55 函数通过；`native-resume.log` 的 Passkeys 10 函数通过，共 98 个不同函数。恢复运行只补执行因测试整数推断失败而未完成的本包及后续门禁，未重复已过回归。全 workspace／all-targets、pail,passport,fortify 严格 Clippy 和全仓 fmt 通过；根 main 三 feature、真实异常 HTTP／Pail、恢复、清理、SIGINT 及独立 Artisan 通过。
- `browser-core.log`：真实 Chrome 注册 AAGUID 格式与缺字段 StoredCredential 重载后认证通过；有效签名的未允许 Origin／端口、未 UV、计数器回退和原生命周期全部负例通过。
- [37 份源码输入](customization-source-sha256.json) 与快照逐字节一致；[资源来源及 SHA](customization-resources.json) 单独登记。最终成功的自定义与默认生成应用均使用相同包源码。新增模型隔离测试中的模糊整数 into 已改为明确 i64 后通过。
- 全部进程退出后按归属清理独占 target、客户端依赖／bundle 和站点 dist，见[清理](customization-cleanup.json)。实际释放 10,274,804 KiB（10.521 GB），剩余 77,156,440 KiB（79.008 GB／73.582 GiB）。源码、生成应用、失败记录、共享缓存与全部数据卷保留。未推送。
