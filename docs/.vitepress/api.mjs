import { readFileSync, existsSync, cpSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import path from "node:path";

export function loadApi(directory, root) {
    if (!directory) return null;
    directory = path.resolve(directory);
    const manifest = JSON.parse(readFileSync(path.join(directory, "build-info.json"), "utf8"));
    const fail = (reason) => { throw new Error(`Invalid API export: ${reason}`); };
    if (manifest.schema !== 1 || !/^[0-9a-f]{40}$/.test(manifest.revision) ||
        manifest.rust !== "1.95.0" || typeof manifest.target !== "string" || !/^[a-zA-Z0-9_-]+$/.test(manifest.target) || manifest.features !== "all" ||
        manifest.dependencies_documented !== false) fail("unsupported build information");
    const hash = (file) => createHash("sha256").update(readFileSync(file)).digest("hex");
    if (manifest.cargo_lock_sha256 !== hash(path.join(root, "Cargo.lock"))) fail("Cargo.lock differs from the documented source");
    if (!manifest.source_sha256 || !manifest.source_sha256["Cargo.toml"] ||
        !manifest.source_sha256["Cargo.lock"]) fail("missing source fingerprints");
    if (!Array.isArray(manifest.source_roots) || !manifest.source_roots.length) fail("missing source roots");
    const inputs = new Set(["Cargo.toml", "Cargo.lock"]);
    const walk = directory => {
        if (!existsSync(directory)) return;
        for (const entry of readdirSync(directory, { withFileTypes: true })) {
            const file = path.join(directory, entry.name);
            if (entry.isDirectory()) walk(file);
            else if (entry.isFile()) inputs.add(path.relative(root, file));
        }
    };
    for (const relative of manifest.source_roots) {
        const directory = path.resolve(root, relative);
        if (directory !== path.resolve(root) && !directory.startsWith(path.resolve(root) + path.sep)) fail("source root escapes repository");
        inputs.add(path.relative(root, path.join(directory, "Cargo.toml")));
        for (const name of ["build.rs", "README.md"]) {
            if (existsSync(path.join(directory, name))) inputs.add(path.relative(root, path.join(directory, name)));
        }
        for (const folder of ["src", "resources", "config", "database", "stubs"]) walk(path.join(directory, folder));
    }
    if (JSON.stringify([...inputs].sort()) !== JSON.stringify(Object.keys(manifest.source_sha256).sort())) fail("source file set differs");
    for (const [file, expected] of Object.entries(manifest.source_sha256)) {
        const resolved = path.resolve(root, file);
        if (!resolved.startsWith(path.resolve(root) + path.sep) ||
            !/^[0-9a-f]{64}$/.test(expected) || hash(resolved) !== expected) fail(`source differs: ${file}`);
    }
    if (!Array.isArray(manifest.crates) || !manifest.crates.length) fail("no workspace libraries");
    for (const crate of manifest.crates) {
        if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(crate.crate) ||
            crate.index !== `${crate.crate}/index.html` ||
            !existsSync(path.join(directory, crate.index))) fail("missing crate index");
    }
    if (!existsSync(path.join(directory, "index.html"))) fail("missing landing page");
    return { directory, manifest };
}

export function copyApi(api, outDir) {
    if (api) cpSync(api.directory, path.join(outDir, "api"), { recursive: true, errorOnExist: true, force: false });
}
