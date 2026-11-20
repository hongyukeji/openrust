# 开发者文档入口整理：docs/README.md

2026-11-20。文档站维护命令迁至 `../../guides/documentation.md`，开发者入口保留三语导航和贡献入口。

````diff
--- docs/README.md (迁移前)
+++ docs/README.md (开发者正文)
@@ -1,54 +1,7 @@
-# 文档
+# OpenRust 开发者文档

 [English](en/README.md) · [简体中文](zh_CN/README.md) · [繁體中文](zh_TW/README.md)

-三语 Markdown 可直接在 GitHub 阅读，各语言的目录页同时用于生成 VitePress 侧栏。
+文档介绍应用开发所需的安装、配置、公开接口、示例及支持限制。三个语言版本采用相同的章节路径和阅读顺序，可直接在 GitHub 阅读。

-## 本地预览
-
-需要 Node.js 24。首次使用或 `package-lock.json` 更新后，在 `docs/` 下运行一次 `npm ci` 安装依赖。
-
-```sh
-cd docs
-npm run dev
-```
-
-默认打开 <http://127.0.0.1:1313/>。端口被占用时会自动尝试下一个端口，以终端显示的地址为准。按 `Ctrl+C` 停止。
-
-## 构建并预览
-
-在 `docs/` 下执行以下命令。完整的静态网站会生成到 `docs/.vitepress/dist/`，需要压缩时直接压缩该目录即可。
-
-```sh
-npm run build
-```
-
-如需预览构建结果，可以继续执行以下命令。
-
-```sh
-npm run preview
-```
-
-构建产物的预览地址为 <http://localhost:1313/>。若开发预览占用该端口，先按 `Ctrl+C` 停止开发预览。
-
-部署到子目录时，访问入口要带末尾的 `/`，并配置托管服务在路径不存在时返回产物中的 `404.html` 和 HTTP 404 状态。
-
-## 版本入口
-
-版本和 Rustdoc 入口在 `.vitepress/config.mjs` 的 `publication` 中配置。
-
-## 文档内容检查
-
-导航章节必须包含正文。生态能力尚未提供时，正文说明实际支持边界和已有原生入口；不使用空白页或只有标题的页面代替功能说明。三个语言版本保持相同路径和导航顺序。
-
-在仓库根目录运行静态检查，再构建站点并验收真实页面：
-
-```sh
-node docs/tests/check_content.mjs
-cd docs
-npm run build
-node tests/check_browser.mjs
-node tests/check_scope.mjs
-```
-
-浏览器脚本使用 Playwright；已有安装位于其他路径时，通过 `DOCS_PLAYWRIGHT_MODULE` 指定模块，通过 `DOCS_BROWSER_PATH` 指定浏览器可执行文件。`check_scope.mjs` 在 `/` 和 `/docs/` 逐页检查三语导航，并核对未完整移植章节的支持状态与原生引用。空导航章节在 VitePress 构建时直接报错。
+参与代码或文档贡献，请阅读[贡献指南](zh_CN/prologue/contributions.md)。
````
