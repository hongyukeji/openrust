# 文档

[English](en/README.md) · [简体中文](zh-Hans/README.md) · [繁體中文](zh-Hant/README.md)

三语 Markdown 可直接在 GitHub 阅读，各语言的目录页同时用于生成 VitePress 侧栏。

## 本地预览

需要 Node.js 24。首次使用或 `package-lock.json` 更新后，在 `docs/` 下运行一次 `npm ci` 安装依赖。

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
