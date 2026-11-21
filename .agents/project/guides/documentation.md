# 文档站维护与验收

本文面向维护项目文档站的协作者。开发者正文边界见[项目规范](../conventions.md#文档规范)。

## 本地预览

需要 Node.js 24。首次使用或 `package-lock.json` 更新后，在仓库的 `docs/` 下运行一次 `npm ci` 安装依赖。

```sh
cd docs
npm run dev
```

默认打开 <http://127.0.0.1:1313/>。端口被占用时会自动尝试下一个端口，以终端显示的地址为准。按 `Ctrl+C` 停止。

## 构建并预览

在 `docs/` 下执行以下命令。完整的静态网站会生成到 `docs/.vitepress/dist/`，需要压缩时直接压缩该目录即可。

```sh
npm run build
```

如需预览构建结果，可以继续执行以下命令。

```sh
npm run preview
```

构建产物的预览地址为 <http://localhost:1313/>。若开发预览占用该端口，先按 `Ctrl+C` 停止开发预览。

部署到子目录时，访问入口要带末尾的 `/`，并配置托管服务在路径不存在时返回产物中的 `404.html` 和 HTTP 404 状态。

## 版本入口

版本和 Rustdoc 入口在 `.vitepress/config.mjs` 的 `publication` 中配置。

## 文档内容检查

导航章节必须包含正文。生态能力尚未提供时，正文说明实际支持边界和已有原生入口；不使用空白页或只有标题的页面代替功能说明。三个语言版本保持相同路径和导航顺序。

在仓库根目录运行静态检查，再构建站点并验收真实页面：

```sh
node docs/tests/check_content.mjs
cd docs
npm run build
node tests/check_browser.mjs
node tests/check_scope.mjs
```

浏览器脚本使用 Playwright；已有安装位于其他路径时，通过 `DOCS_PLAYWRIGHT_MODULE` 指定模块，通过 `DOCS_BROWSER_PATH` 指定浏览器可执行文件。`check_scope.mjs` 在 `/` 和 `/docs/` 逐页检查三语导航，并核对未完整移植章节的支持状态与原生引用。空导航章节在 VitePress 构建时直接报错。

## 内容边界检查

`node docs/tests/check_content.mjs` 检查三语正文、导航、相对链接及内部任务编号、项目记录路径和交付过程用语。自动检查只覆盖可识别模式；评审仍需确认正文面向应用开发者，并把验收经过、实现追踪和待办迁到 `.agents/project/` 对应主题。

命令入口按[文档规范](../conventions.md#文档规范)使用源码 `./artisan` 或部署后的 `./main artisan`。内容检查拒绝直接运行构建目录内二进制的 Artisan 示例，覆盖 debug／release、目标平台子目录、Windows 路径及 shell 续行；构建产物的位置说明和复制命令不受影响。`npm run build` 先执行入口规则正反例，再检查三语正文，违规时在站点构建前退出。修改规则时运行 `node --test docs/tests/command_entrypoints.test.mjs`，并在隔离文档快照中注入违规示例，确认内容检查和构建入口实际失败。

内部材料只保留仍有效的信息并归入已有主题；一次性检查保存简短结论、实际验证结果和提交号即可。历史原文与差异通过 Git 追溯，不另建逐页文档备份。

## 可追溯 API 快照

API 参考统一通过可追溯导出接入站点。Python 3.11+、Rust 1.94.0、锁定依赖、全部 feature、构建主机目标与无依赖库文档构成导出契约。在独立干净快照根目录执行：

```sh
npm --prefix docs ci
python3 -B docs/scripts/build_api.py --output "$PWD/tmp/openrust-api" --target-dir "$PWD/tmp/openrust-api-target"
DOCS_API_DIR="$PWD/tmp/openrust-api" npm --prefix docs run build
DOCS_API_DIR="$PWD/tmp/openrust-api" node docs/tests/check_api_export.mjs
node docs/tests/check_api_snapshot.mjs
node docs/tests/check_browser.mjs
python3 -B docs/tests/check_api_cli.py -v
```

输出目录须尚不存在，target 须为本批独占目录；源码归档另传 `--revision` 完整提交号。每次先清除专属 target 中旧 Rustdoc 页面，防止已删除接口残留。`build-info.json` 保存提交、目标、包清单、锁文件和源码指纹，站点核对完整源码文件集合及 SHA-256，拒绝过期导出。

源码指纹同时覆盖包根 `README.md`、`stubs/` 和 `config/`：扩展通过 `include_str!` 嵌入 README 作为 Rustdoc，通过模板发布应用源码，并将包配置编译为默认值；这些输入变化后也须重新导出，不能只核对 `src/`。新增其他目录的编译输入时，同步更新导出脚本、站点校验及过期输入负例。

`DOCS_API_DIR` 是站点接入 API 的唯一变量。联合产物位于 `docs/.vitepress/dist/`，其中 `api/` 包含 Rustdoc、源码页及本地搜索，侧栏使用完整页面导航。导出列出骨架包不表示其功能已经完成；发布前仍须核对源码提交、指南、接口参考和实际托管地址。

普通 `npm --prefix docs run build` 只生成指南，不显示 Rustdoc 入口；用 `node docs/tests/check_api_snapshot.mjs --without-export` 检查普通构建。浏览器环境变量见上文。开发者操作见[API 参考](../../../docs/zh_CN/reference/api.md)。
