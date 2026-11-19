# Passport-02 交接边界

2026-11-19 总控在首批验收后收到侧对话分工同步：总控转回核心遗留与整合，扩展由 OpenRust · Plan 接续。本文件记录可核实停点与修改边界，不代表对方已经确认接收，不通过本文件增加其他任务的提交／发布权限。

## 已交付

- 本地 master 提交 **`8cb7a05a5f44a57a8601adce5ab371b38efeaf87`**，23 文件；没有推送。
- 本批源码、测试、三语正文、账本和验收记录均已提交，**没有遗留 Passport-02 未提交差异**。提交后公共索引为空；50 份其他 WIP 的内容 SHA 核对未变。
- [验收与最小 tree](README.md)、[18 个源码／测试／正文 SHA 和 34 个应用基线](source-sha256.json)。24 个测试函数、MySQL 8.4.13、实际 HTTP／Artisan、严格 Clippy、三语构建及 18 项浏览器检查通过。只核定该首批范围，整单元仍未完成。

## 后续修改归属

总控暂停新增 Passport 代码；Plan 可从上述提交接续扩展。边界明确前不由双方同时编辑同一功能。

| 范围 | 接续边界 |
| --- | --- |
| `vendor/openrust/passport/` | 扩展接续，固定 Passport 13.10.0；未完成列表见下节 |
| `docs/{en,zh_CN,zh_TW}/packages/passport.md` | 与扩展同批同步，保持本批已过范围及未过声明 |
| 核心 Eloquent／Support 必要配套 | 本批只有 getCreatedAtColumn 和隐藏的比较桥；后续若需要改共享核心，先明确文件／接口和归属，再动手，不能和核心任务同时写同一文件 |
| `current.md`、`progress.md`、`core-closure-ledger.md` | 共享文件，逐段维护自己的记录，不整文件覆盖；扩展继续维护 V06–V09，核心主要跟踪 V02–V05 等既有缺口 |
| init／Session／Encryption／SMTP、历史 patch、translation-database | 原协作者 WIP，本次没有接收、覆盖或提交它们 |

## 资源与证据

本地 `/private/tmp/openrust-passport-02-client-20261119/` 保留：commit.json（实际提交清单）、accepted-overlay.tgz、accepted-source-sha256.json、baseline-sha256.json、mysql-accepted.log、native-final.log、clippy-accepted.log、fmt-accepted.log、root-smoke.py／log、docs-isolated-browser.log、docs-accepted-build.log、docs-accepted-sections.log、cleanup-local.json 和 cleanup-remote.log。

隔离源码和编译产物原为该目录 `source/`、`target/`，已按每批收尾规则删除，不能声称缓存仍可复用。下一批从已提交源码重建自己的快照与 target，不使用共享 root target 冒充隔离证据。远程 `/tmp/openrust-passport-lookup-20261119` 包装目录与本批无引用 MySQL 镜像已移除；全部数据卷及共享 Rust 镜像保留。清理后本地可用约 80.07 GB，高于 50 GB 停线。

## 下一步与未过范围

1. 先收口 Client 未绑定实例的 passport.connection 回退。主线 Model.query／持久化默认取静态 metadata；本批仓储通过显式连接正确工作，不能据此声称任意 Client::query／save 已适配。若扩展需虚拟 getConnection，应与核心明确必要接口和事务连接约束，避免借用名称与拥有配置值产生不一致。
2. 完整 Client 关系／工厂／scope 继承、plain_secret 显式追加、UUID 选择机制。当前实际模型支持本批查询／写入，不代表完整 Client 公开面冻结。
3. 按真实 Token／RefreshToken 模型生命周期实现 delete 撤销链，再撤销 Client，保留事件／连接／错误传播；不能用裸批量 SQL 替代完整模型生命周期。
4. 完整 passport:client 的选项、默认值、交互、输出与错误码，实际 Artisan 创建各类客户端；本批只证明原有 Artisan 不回归。
5. 补固定 PHP oracle：数组／比较、setter 失败顺序、浮点转字符串、Scope JSON options／Unicode／斜杠字节规则，以及完整 HTTP 管理和跨用户拒绝路径。旧版复用源和固定 SHA 见 README。

V06、Passport-02 整单元保持部分验收；Passport-03 仍须等待其前置。核心总控下一步只读复核 V02 ViewData／非有限数和 V03 Blade 边界，随后按依赖闭合；V04 Pail 身份时序与 V05 Windows 原生终端不能遗漏。

## 2026-11-20 接续

原对话已提交连接回退 `7720f142e` 与 scope／明文追加 `6e1ea9f2d`。本对话接续的[编译绑定客户端工厂](client-factory.md)通过上述限定范围验收，详见记录中的语言适配和未验收声明。当前停点仍属 Passport-02：完整关系、UUID 配置、撤销链、客户端命令与其余 PHP 对照，未进入 Passport-03。源码与三语正文逐份核 SHA；保留共享协作者改动。

## 2026-11-20 命令与撤销接续

[客户端命令、关系与撤销](client-management.md)已通过限定范围验收，44 个测试函数与实际 PTY／HTTP／Artisan／三语检查见正文。UUID 实例策略、编译模型关系、自定义 Token／Refresh 注册、1000 分批撤销及 passport:client 已处理。Scope JSON、setter 失败顺序、浮点转换和其余 PHP oracle 仍待；Passport-02 保持部分验收。

## 2026-11-20 PHP 契约与管理闭合

[PHP 契约与 HTTP 管理验收](contracts.md)完成，Passport-02 计划范围已验收。48 个测试函数、2699 条固定 PHP 对照记录、实际 MySQL／认证 HTTP 管理／根入口及三语验收见正文；最终 14 项复验通过。上方历史未完成列表由后续各批逐项闭合。下一单元 Passport-03 授权码与 PKCE，依赖 Passport-02、04-04、05-08；启用 Fortify 动作时接 API-F01。V06 仍为部分验收，不声称完整 OAuth2 已交付。

本批证据保留于 `/private/tmp/openrust-passport-oracle-20261120/`，含 accepted-overlay.tgz、源码 SHA、固定上游、PHP 生成结果／重复与负样本、验收脚本及日志、cleanup.json。专属 source／target 已删除，不能复用原缓存；实际释放 5.563 GB，剩余约 68.979 GB，全部数据卷保留。只核定本批文件本地提交，不推送；同期 Pail／终端测试及历史 WIP 保留。
