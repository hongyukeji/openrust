export const siteMarker = '{"site":"openrust-docs"}\n';
export const languages = {
    en: { label: "English", lang: "en", path: "en" },
    "zh-Hans": { label: "简体中文", lang: "zh-Hans", path: "zh-Hans" },
    "zh-Hant": { label: "繁體中文", lang: "zh-Hant", path: "zh-Hant" },
};
const supported = Object.fromEntries(Object.values(languages).map(({ path, lang }) => [path, lang]));

export function landing(notFound) {
    const languageLinks = Object.values(languages).map(({ lang, path, label }) =>
        `<a lang="${lang}" data-path="${path}" href="./${path}/">${label}</a>`
    ).join("\n        ");
    return `<!doctype html>
<html lang="${languages["zh-Hans"].lang}">
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
        try {
            const locale = new Intl.Locale(tag);
            if (locale.language === "en") return "en";
            if (locale.language !== "zh") return null;
            if (locale.script === "Hant") return "zh-Hant";
            if (locale.script === "Hans") return "zh-Hans";
            if (locale.script) return null;
            return ["TW", "HK", "MO"].includes(locale.region) ? "zh-Hant" : "zh-Hans";
        } catch { return null; }
    }).find(Boolean) ?? "zh-Hans";
    const errorPage = ${notFound};
    if (errorPage) {
        const locale = location.pathname.split("/").findLast((part) => Object.hasOwn(supported, part)) ?? preferred;
        document.documentElement.lang = supported[locale];
        document.getElementById("message").textContent = {
            en: "Page not found. Return to the documentation:",
            "zh-Hans": "找不到页面。返回文档首页：",
            "zh-Hant": "找不到頁面。返回文件首頁：",
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
