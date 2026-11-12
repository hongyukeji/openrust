import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";
import { serve } from "vitepress";

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.DOCS_PLAYWRIGHT_MODULE || "playwright");
const root = fileURLToPath(new URL("../", import.meta.url));

const languages = [
    ["en", "en-US", "en", "English", "Routing"],
    ["zh-hans", "zh-CN", "zh-Hans", "简体中文", "路由"],
    ["zh-hant", "zh-TW", "zh-Hant", "繁體中文", "路由"],
];

const browser = await chromium.launch({ executablePath: process.env.DOCS_BROWSER_PATH });
try {
    for (const mount of ["/", "/docs/"]) {
        const { server } = await serve({ root, base: mount, port: 0 });
        try {
            const base = `http://127.0.0.1:${server.address().port}${mount}`;
            const fallback = await browser.newPage({ locale: "fr-FR" });
            await fallback.goto(base);
            await fallback.waitForURL(base + "zh-hans/");
            await fallback.close();

            for (const [index, [language, locale, htmlLang, , query]] of languages.entries()) {
                const [next, , , nextLabel] = languages[(index + 1) % languages.length];
                for (const mobile of [false, true]) {
                    const page = await browser.newPage({
                        locale,
                        viewport: mobile ? { width: 375, height: 812 } : { width: 1440, height: 900 },
                        isMobile: mobile,
                        hasTouch: mobile,
                    });
                    page.setDefaultTimeout(7000);
                    const errors = [];
                    page.on("pageerror", (error) => errors.push(error.message));

                    await page.goto(base + "?keep=1#intro");
                    await page.waitForURL(base + language + "/?keep=1#intro");
                    assert.equal(await page.locator("html").getAttribute("lang"), htmlLang);

                    const chapter = "/getting-started/installation.html?keep=1#intro";
                    await page.goto(base + language + chapter);
                    if (mobile) await page.locator(".VPNavBarHamburger").click();
                    const nav = page.locator(mobile ? ".VPNavScreen" : ".VPNavBar");
                    const version = nav.getByRole("button", { name: "master", exact: true });
                    if (mobile) await version.click();
                    else await version.hover();
                    const branch = nav.locator('a[href*="/blob/v1.x/"]');
                    await branch.waitFor({ state: "visible" });
                    assert.equal(await branch.textContent(), "v1.x");
                    assert.equal(
                        await branch.getAttribute("href"),
                        `https://github.com/hongyukeji/openrust/blob/v1.x/docs/${htmlLang}/README.md`,
                    );

                    if (mobile) await nav.locator(".VPNavScreenTranslations button").click();
                    else await nav.locator(".VPNavTranslations button:visible").first().hover();
                    await Promise.all([
                        page.waitForURL(base + next + chapter),
                        nav.getByRole("link", { name: nextLabel, exact: true }).click(),
                    ]);
                    await Promise.all([
                        page.waitForURL(base + next + "/getting-started/configuration.html"),
                        page.locator(".VPDocFooter a[href*='getting-started/configuration']").click(),
                    ]);

                    if (!mobile) {
                        const section = base + language + "/digging-deeper/scheduling.html#maintenance-mode";
                        assert.equal((await page.goto(section)).status(), 200);
                        await page.locator("#maintenance-mode").waitFor();
                        await page.locator("#pausing-scheduled-tasks").waitFor();
                        assert.match(await page.locator("main").innerText(), /evenWhenPaused/);

                        assert.match(await page.locator("main").innerText(), /evenInMaintenanceMode/);
                        const maintenance = page.locator('main a[href$="configuration.html#maintenance-mode"]');
                        await Promise.all([
                            page.waitForURL(base + language + "/getting-started/configuration.html#maintenance-mode"),
                            maintenance.click(),
                        ]);
                    }

                    await page.goto(base + language + "/");
                    await page.locator(".VPNavBarSearchButton").click();
                    await page.locator("#localsearch-input").fill(query);
                    await Promise.all([
                        page.waitForURL((url) => url.pathname === mount + language + "/the-basics/routing.html"),
                        page.locator('.VPLocalSearchBox a[href*="the-basics/routing.html"]').first().click(),
                    ]);

                    for (const missing of [language + "/nested/missing.html", "unknown/deep/missing.html"]) {
                        const response = await page.goto(base + missing);
                        assert.equal(response.status(), 404);
                        await Promise.all([
                            page.waitForURL(base + language + "/"),
                            page.locator(`a[lang="${htmlLang}"][href]`).click(),
                        ]);
                    }
                    assert.deepEqual(errors, []);
                    console.log(mount, language, mobile ? "mobile" : "desktop", "PASS");
                    await page.close();
                }
            }
        } finally {
            await promisify(server.close).call(server);
        }
    }
} finally {
    await browser.close();
}
