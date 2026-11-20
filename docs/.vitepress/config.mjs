import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { defineConfig } from "vitepress";
import { landing, languages, siteMarker } from "./landing.mjs";
import { loadApi, copyApi } from "./api.mjs";
import { fileURLToPath } from "node:url";

const apiExport = loadApi(process.env.DOCS_API_DIR, fileURLToPath(new URL("../../", import.meta.url)));

const publication = {
    current: "master",
    versions: [{ name: "v0.x", file: "README.md" }],
    rustdoc: apiExport ? "/api/index.html" : null,
};

const labels = {
    en: {
        title: "OpenRust Documentation",
        outline: "On this page",
        menu: "Menu",
        top: "Return to top",
        previous: "Previous page",
        next: "Next page",
        api: "API Documentation",
    },
    "zh_CN": {
        title: "OpenRust 文档",
        outline: "本页目录",
        menu: "菜单",
        top: "返回顶部",
        previous: "上一章",
        next: "下一章",
        api: "API 文档",
        search: ["搜索文档", "关闭搜索", "清除搜索", "搜索结果详情", "选择", "切换", "关闭"],
    },
    "zh_TW": {
        title: "OpenRust 文件",
        outline: "本頁目錄",
        menu: "選單",
        top: "回到頂端",
        previous: "上一章",
        next: "下一章",
        api: "API 文件",
        search: ["搜尋文件", "關閉搜尋", "清除搜尋", "搜尋結果詳情", "選擇", "切換", "關閉"],
    },
};

const rewrites = {};
const chapterTitles = new Map();

function localeConfig(language) {
    const text = labels[language];
    const { label, lang, path: prefix } = languages[language];
    const sidebar = [];
    let items = sidebar;
    for (const line of readFileSync(new URL(`../${language}/README.md`, import.meta.url), "utf8").split("\n")) {
        const group = /^## (.+)$/.exec(line);
        if (group) {
            items = [];
            sidebar.push({ text: group[1], collapsed: true, items });
            continue;
        }
        const chapter = /^(?:- )?\[([^\]]+)\]\(([^)]+\.md)\)$/.exec(line);
        if (!chapter) continue;
        const [, title, file] = chapter;
        const source = `${language}/${file}`;
        const target = `${prefix}/${file}`;
        if (source !== target) rewrites[source] = target;
        chapterTitles.set(source, title);
        chapterTitles.set(target, title);
        items.push({ text: title, link: file === "index.md" ? `/${prefix}/` : `/${target.slice(0, -3)}` });
    }
    // Rustdoc owns these pages; use a full navigation rather than the guide's SPA router.
    const apiSource = `${language}/reference/api.md`;
    const apiTarget = `${prefix}/reference/api.md`;
    if (apiSource !== apiTarget) rewrites[apiSource] = apiTarget;
    sidebar.push({ text: { en: "API reference guide", zh_CN: "API 参考指南", zh_TW: "API 參考指南" }[language], link: `/${prefix}/reference/api` });
    if (publication.rustdoc) sidebar.push({ text: "Rustdoc", link: publication.rustdoc, target: "_self" });

    return {
        label,
        lang,
        link: `/${prefix}/`,
        title: text.title,
        themeConfig: {
            sidebar,
            nav: [{
                text: publication.current,
                items: [
                    { text: publication.current, link: `/${prefix}/` },
                    ...publication.versions.map(({ name, file }) => ({
                        text: name,
                        link: `https://github.com/hongyukeji/openrust/blob/${encodeURIComponent(name)}/${file}`,
                    })),
                ],
            }],
            outline: { label: text.outline, level: [2, 4] },
            sidebarMenuLabel: text.menu,
            returnToTopLabel: text.top,
            docFooter: { prev: text.previous, next: text.next },
        },
    };
}

const locales = {
    root: localeConfig("zh_CN"),
    en: localeConfig("en"),
    "zh_TW": localeConfig("zh_TW"),
};
const searchLocales = {};
for (const [key, language] of [["root", "zh_CN"], ["zh_TW", "zh_TW"]]) {
    const [search, close, reset, details, select, navigate, footerClose] = labels[language].search;
    searchLocales[key] = {
        translations: {
            button: { buttonText: search, buttonAriaLabel: search },
            modal: {
                displayDetails: details,
                resetButtonTitle: reset,
                backButtonTitle: close,
                noResultsText: language === "zh_CN" ? "没有找到相关结果" : "找不到相關結果",
                footer: { selectText: select, navigateText: navigate, closeText: footerClose },
            },
        },
    };
}

export default defineConfig(({ command }) => ({
    title: labels["zh_CN"].title,
    base: command === "build" ? "./" : "/",
    srcExclude: ["README.md", "*/README.md"],
    rewrites,
    locales,
    themeConfig: {
        socialLinks: [{ icon: "github", link: "https://github.com/hongyukeji/openrust" }],
        search: { provider: "local", options: { locales: searchLocales } },
    },
    markdown: {
        config(md) {
            // Empty navigation chapters must declare their support status explicitly.
            md.core.ruler.before("normalize", "require_chapter_content", (state) => {
                const title = chapterTitles.get(state.env.relativePath);
                if (title && !state.src.trim()) throw new Error(`Empty documentation chapter: ${state.env.relativePath}`);
            });
        },
    },
    buildEnd({ outDir }) {
        copyApi(apiExport, outDir);
        writeFileSync(path.join(outDir, "index.html"), landing(false));
        writeFileSync(path.join(outDir, "404.html"), landing(true));
        writeFileSync(path.join(outDir, "docs-site.json"), siteMarker);
    },
    vite: {
        plugins: [{
            name: "docs-entry",
            configureServer(server) {
                server.middlewares.use((request, response, next) => {
                    const pathname = new URL(request.url, "http://localhost").pathname;
                    if (pathname === "/docs-site.json") {
                        response.setHeader("Content-Type", "application/json");
                        response.end(siteMarker);
                    } else if (pathname === "/" || pathname === "/index.html") {
                        response.setHeader("Content-Type", "text/html; charset=utf-8");
                        response.end(landing(false));
                    } else next();
                });
            },
        }],
    },
}));
