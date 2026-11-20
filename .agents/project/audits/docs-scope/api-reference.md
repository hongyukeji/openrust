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
- API导出列出 translation-database 骨架不代表数据库翻译已实现。没有改动Rust运行逻辑、依赖或数据库行为，本批未重复Clippy／数据库／根运行入口验收。V16发布快照与实际托管地址、V17最终整合及V18发布批准仍待；仅本地提交，不推送、不发布。

## 产物

最终导出 `api-delivery/`、`api-delivery.tar.gz` 和 `site-delivery.tar.gz` 保留在交接目录；核心侧保留最终站点与日志。交接前仅回收该批闲置target和中间输出3,423,736 KiB，详见 `cleanup-delivery.json`。早期独立构建目录 `/private/var/folders/20/lxn_zjvn3614f836z2cm3vww0000gn/T/openrust-api-docs-20261120-m6zucjkh/` 的target／依赖已清，释放1,133,383,680字节，最终site和日志保留。全部共享缓存、源码、数据库与数据卷保留。

核心侧最终普通站点验收后清理专属dist，逻辑大小60921061字节，可用空间增加64040960字节、剩余69703581696字节；源码、API站点、日志及共享依赖保留。
