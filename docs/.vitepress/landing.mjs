export const siteMarker = '{"site":"openrust-docs"}\n';
export const languages = {
    en: { label: "English", lang: "en", path: "en" },
    zh_CN: { label: "简体中文", lang: "zh-CN", path: "zh_CN" },
    "zh_TW": { label: "繁體中文", lang: "zh-TW", path: "zh_TW" },
};
const supported = Object.fromEntries(Object.values(languages).map(({ path, lang }) => [path, lang]));

export function landing(notFound) {
    const languageLinks = Object.values(languages).map(({ lang, path, label }) =>
        `<a lang="${lang}" data-path="${path}" href="./${path}/">${label}</a>`
    ).join("\n        ");
    return `<!doctype html>
<html lang="${languages.zh_CN.lang}">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${notFound ? "404 · OpenRust" : "OpenRust"}</title>
<style>
    :root { color-scheme: light dark; font-family: system-ui, sans-serif; }
    main { max-width: 34rem; margin: 12vh auto; padding: 1.5rem; }
    nav { display: flex; flex-wrap: wrap; gap: 1rem; }
</style>
<main>
    <h1>${notFound ? "404" : "OpenRust"}</h1>
    <p id="message"></p>
    <nav aria-label="Language">
        ${languageLinks}
    </nav>
</main>
<script>
    const supported = ${JSON.stringify(supported)};
    const preferences = navigator.languages?.length ? navigator.languages : [navigator.language];
    const preferred = preferences.map((tag) => {
        const [language, ...parts] = tag.toLowerCase().split("-");
        if (language === "en") return "en";
        if (language !== "zh") return null;
        if (parts.includes("hant")) return "zh_TW";
        if (parts.includes("hans")) return "zh_CN";
        return parts.some((part) => ["tw", "hk", "mo"].includes(part)) ? "zh_TW" : "zh_CN";
    }).find(Boolean) ?? "zh_CN";
    const errorPage = ${notFound};
    if (errorPage) {
        const locale = location.pathname.split("/").findLast((part) => Object.hasOwn(supported, part)) ?? preferred;
        document.documentElement.lang = supported[locale];
        document.getElementById("message").textContent = {
            en: "Page not found. Return to the documentation:",
            "zh_CN": "找不到页面。返回文档首页：",
            "zh_TW": "找不到頁面。返回文件首頁：",
        }[locale];
        const links = [...document.querySelectorAll("nav a")];
        links.forEach((link) => link.removeAttribute("href"));
        // A static host may serve this file at any missing URL under its mount.
        (async () => {
            let base = new URL("./", location.href);
            while (true) {
                try {
                    const response = await fetch(new URL("docs-site.json", base));
                    if (response.ok && (await response.json()).site === "openrust-docs") {
                        for (const link of links) link.href = new URL(link.dataset.path + "/", base);
                        return;
                    }
                } catch {}
                const parent = new URL("../", base);
                if (parent.href === base.href) return;
                base = parent;
            }
        })();
    } else {
        const target = new URL("./" + preferred + "/", location.href);
        target.search = location.search;
        target.hash = location.hash;
        location.replace(target);
    }
</script>
</html>
`;
}
