import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createMarkdownRenderer } from "vitepress";

// 用实际VitePress解析器验证三语引用及改版本后的输出，不另写TOML解析器。
test("release pages read the workspace version through native snippets", async () => {
    const root = fileURLToPath(new URL("../../", import.meta.url));
    const temporary = path.join(root, "target/tmp");
    mkdirSync(temporary, { recursive: true });
    const fixture = mkdtempSync(path.join(temporary, "docs-version-"));
    try {
        const docs = path.join(fixture, "docs");
        const renderer = await createMarkdownRenderer(docs);
        for (const version of ["0.1.2", "9.8.7-rc.1+test"]) {
            writeFileSync(path.join(fixture, "Cargo.toml"), `#region framework-version\n[workspace.package]\nversion = "${version}"\n#endregion framework-version\n\n[dependencies]\nwrong = "1"\n`);
            for (const language of ["en", "zh-Hans", "zh-Hant"]) {
                const page = path.join(docs, language, "prologue/releases.md");
                mkdirSync(path.dirname(page), { recursive: true });
                const text = readFileSync(path.join(root, "docs", language, "prologue/releases.md"), "utf8");
                const html = await renderer.renderAsync(text, { path: page });
                const code = [...html.matchAll(/<code[^>]*>([\s\S]*?)<\/code>/g)]
                    .map((match) => match[1].replace(/<[^>]*>/g, ""))
                    .find((value) => value.includes("[workspace.package]"));
                assert(code, `${language}: missing workspace snippet`);
                assert(code.includes(version), `${language}: stale version`);
                assert(!code.includes("wrong"), `${language}: region boundary lost`);
                assert(!code.includes("#region"), `${language}: region markers leaked`);
            }
        }
    } finally {
        rmSync(fixture, { recursive: true, force: true });
    }
});
