# V16 API 参考与三语站点

2026-11-20–21 接手 Framework 核心任务，确认原 V02 停点已由后续提交完成；本批推进 V16。最终独立站点验收基线 `aeecaa346e9d8f1c99511b8aae97f5c4ee6d3377`，Rustdoc 输入与 `ea38d2ad7` 相同，不纳入并行数据库多语注释改动。

## 实现

API 统一使用 `docs/scripts/build_api.py` 导出，再由 `DOCS_API_DIR` 接入 VitePress；移除开发中重叠的裸 Rustdoc 入口与 npm 包装器。Python 3.11+、Rust1.94、锁文件、全部 feature、显式构建主机目标及无依赖库文档；每次清除独占 target 的旧文档，导出包含10库、源码页、搜索和提交／源码指纹。站点核对完整输入文件集合及 SHA-256，拒绝过期产物。普通构建不显示 Rustdoc 入口。

三语 API 指南集中维护用法和平台／骨架限制，首页与贡献指南引用该页；内部命令见[维护指南](../../guides/documentation.md#可追溯-api-快照)。Rustdoc 链接使用完整页面导航，避免 VitePress 前端路由误接管。

## 验证与边界

- 最终 Rustdoc 在 `/private/tmp/openrust-api-reference-20261120/` 的 `export-delivery.log`：10库成功；显式 host 覆盖无效 CARGO_BUILD_TARGET，废弃页面负例清除。Cargo1.94的 clean --doc 未实际删除显式 target 文档，故改为仅清理专属目录后重建。
- 同目录 `manifest-delivery.log` 和 `cli-delivery.log`：7种过期／非法导出拒绝与匹配接受、7个CLI拒绝测试通过；已有目录不覆盖，脏源码不允许伪装干净提交。
- 核心侧最终复核目录 `/private/tmp/openrust-api-core-20261120/`：三语内容检查每语106导航／108 Markdown；源码与最终交接导出脚本一致；普通及API构建成功，匹配／拒绝检查通过。
- `api-browser-final.log`：根路径／子目录下三语桌面／移动12组合实际点击 Rustdoc 入口，10库源码链接、Application搜索／类型页均通过，无页面脚本错误和本地HTTP错误。`api-guides-browser.log` 与 `guides-browser.log` 通用12组合通过；普通构建无API的专项检查通过。早期浏览器夹具失败和修正日志保留，不计入成功结果。
- 该次API导出列出 translation-database 骨架不代表数据库翻译已实现（后续扩展交付与整合见下文）。没有改动Rust运行逻辑、依赖或数据库行为，本批未重复Clippy／数据库／根运行入口验收。V16发布快照与实际托管地址、V17最终整合及V18发布批准仍待；仅本地提交，不推送、不发布。

## 产物

最终导出 `api-delivery/`、`api-delivery.tar.gz` 和 `site-delivery.tar.gz` 保留在交接目录；核心侧保留最终站点与日志。交接前仅回收该批闲置target和中间输出3,423,736 KiB，详见 `cleanup-delivery.json`。早期独立构建目录 `/private/var/folders/20/lxn_zjvn3614f836z2cm3vww0000gn/T/openrust-api-docs-20261120-m6zucjkh/` 的target／依赖已清，释放1,133,383,680字节，最终site和日志保留。全部共享缓存、源码、数据库与数据卷保留。

核心侧最终普通站点验收后清理专属dist，逻辑大小60921061字节，可用空间增加64040960字节、剩余69703581696字节；源码、API站点、日志及共享依赖保留。

## 数据库翻译交付后的 API 整合

2026-11-21，数据库翻译扩展 `022784d08` 交付后，将三语 API 指南的骨架说明替换为实际 DatabaseLoader 与 localization 使用入口。API 导出基线 `18a44b15771301eae70050bbf589861a7efdb98c`，三语站点叠加本次指南修改；未纳入并行数据库注释、迁移合并与短信重构。

在 `/private/tmp/openrust-core-review-20261121/combined-source/` 以 Rust1.94、全 feature 与显式 host 重新导出10库，构建三语联合站点。`check_api_export.mjs` 的7种拒绝与匹配接受、`check_api_snapshot.mjs` 的根／子目录三语桌面／移动12组合及10库源码／搜索、`check_browser.mjs` 的12通用组合全部通过；每语106导航／108 Markdown内容检查通过。日志为同批 `api-export.log`、`api-manifest.log`、`api-browser.log`、`browser.log`。这次验证不代表实际生产托管或最终生成应用验收完成；V16最终发布快照、V17整合与V18批准继续保留。

随后以本批提交 `599bb7d30bf7aa89ad54c1e598f5ae6ff757f224` 覆盖隔离归档中的全部受版本控制文件，再次生成 `api-export-delivery/`；该目录的 `build-info.json` 与提交完全一致。10库、公开与模块两处 DatabaseLoader 页面及 withTable／withMaxRows／withFallback 方法已核对；联合站点重建、7种拒绝／匹配接受和12组API浏览器检查再次通过。日志为 `api-export-delivery.log`、`site-delivery.log`、`api-manifest-delivery.log`、`api-browser-delivery.log`、`api-loader-delivery.log`，导出保留供复核。内部验收记录后续补充不改变此源码与锁文件指纹。


## 2026-11-21 设置、原生时间与社交登录最终快照

源码固定为 `b168b31aa063a289a5a025ed5080a99f19bb6c7e`，包含设置 S1、数据库翻译、统一短信、PostgreSQL 绑定修复、Jiff/time 迁移及微信小程序。同期另行开发的支付扩展 WIP 不在此快照；不以工作区最新状态代替本批 Git 归档。

- `docs/scripts/build_api.py` 与站点校验同时将包根 README 和 stubs 纳入指纹，覆盖 `include_str!` 文档与发布模板；新增其他编译输入时同改导出、校验和负例。导出固定内部工具链 `RUSTUP_TOOLCHAIN=1.94.0`，规则写入维护指南与项目规范。
- Rust 1.94、全 feature、显式 aarch64-apple-darwin 导出 11 库，包含 Settings。配置无效 CARGO_BUILD_TARGET 仍正确使用 host；旧页面清除已核对。最终 `build-info.json` 的 2,013 个输入逐个与上述 Git 归档对比完全一致，含固定 TZif、来源／许可证、嵌入 README 和模板。
- 三语 Settings 入口与原生时间说明已更新。每语 107 导航／109 Markdown；普通指南构建、无导出 API 导航与通用浏览器通过，普通指南输入与最终快照一致（导出脚本的工具链设置变化不影响指南产物）。
- 正式联合站点构建、匹配接受及 9 类过期／非法输入拒绝、7 个 CLI 拒绝用例通过；新增 README／模板变更拒绝，保留新增源码文件拒绝。
- `/` 与 `/docs/` 下三语桌面／移动 12 组 API 导航、11 库源码页、Application 搜索和 Date／Settings／MiniApp 方法锚点通过。通用 12 组、Socialite 6 组及全部导航页面／156 个支持边界页面检查通过。

证据与交付位于 `/private/tmp/openrust-final-integration-20261121/`：`source-final.txt`、`api-archive-verified.json`、`api-export-final.log`、`api-manifest-final.log`、`api-cli-final.log`、`site-final.log`、`api-browser-final.log`、`browser-final.log`、`socialite-browser-final.log`、`scope-final.log`；保留 `api-delivery/` 及 `source/docs/.vitepress/dist/`。首轮提交号参数录入有误的导出仅作预检，不计最终交付；该产物已删除，正式导出直接读取 Git 生成的提交号文件。

当前快照通过本机文档验收，不表示生产托管地址、全部平台或用户发布批准已完成。无版本升级、tag、推送或部署。


## 2026-11-21 扩展配置与 Cashier 合并后的 API 快照

源码固定 `0582cc8bd39bf3a88b77a3857a7b4d1f24054fc4`，包含核心配置归属修复 `285fcacd0`、Pay 与 Cashier。使用 Git archive 独立源码及本批专属target；只将两份待提交的API验收脚本复制到快照，库源码及开发者指南不叠加工作区改动。保留未跟踪memory目录。

- Rust1.94.0、jobs=2、增量关闭、dev debug=0，使用既有 `build_api.py` 按显式host及全部feature导出13库；启用broken／private intra-doc链接错误检查并成功完成。API导出清单的2,039份输入逐个与固定Git归档核对一致，包含Settings／Pay／OIDC／Reverb／Cashier的包config。现有导出器和站点已覆盖config目录，本批没有增加另一套指纹机制。
- `check_api_export.mjs`新增上述五份配置模板的变更拒绝；14类非法／过期输入拒绝及匹配接受通过。7项CLI拒绝回归通过。维护指南明确包配置也是编译输入。
- 三语内容检查每语108导航／110 Markdown、完整联合站点构建通过。`check_api_snapshot.mjs`在 `/` 与 `/docs/` 的三语桌面／移动12组导航、两挂载全部13库源码链接、Application搜索与现有Date／Settings／MiniApp方法检查通过；新增CashierManager、Billable的checkout／newSubscription／billingPortal／invoiceDocument及PayManager方法锚点均通过。
- 联合站点的Cashier两页×三语×两挂载×桌面移动24组、普通12组及全部导航6组浏览器检查通过，支持说明页为每语24页。浏览器首轮默认可执行文件不存在，改为显式指定已有Chromium1243后全部退出0；初轮日志保留，不计为通过。文档构建有既有chunk体积提示，未当作无警告构建。

交付与证据目录为 `/private/tmp/openrust-api-cashier-20261121/`：`api-delivery/`为独立Rustdoc导出，`source/docs/.vitepress/dist/`为三语联合站点；`archive-verified.json`、`api-export.log`、`api-manifest.log`、`api-cli.log`、`site-build.log`及`check_*.log`记录本次结果。旧快照仅适用其自身源码版本，不再用旧11库快照代表当前计费扩展。

本批只扩展API验收脚本、维护说明及内部交接，没有修改Rust运行逻辑；沿用Cashier主题记录的三库及严格Clippy结果，没有重跑整个运行时行为矩阵。真实商户／客户端SDK／SCA／资金交易、其他平台及实际生产托管未验，无推送、tag或部署。确认target无打开文件且所有验收进程退出后，删除本批闲置target并仅unlink共享node_modules，保留源码／日志／最终API与联合站点；可用空间47,406,544→48,297,792 KiB，净增891,248 KiB。Docker卷清单前后相同（为空），未操作数据库。
