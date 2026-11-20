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
