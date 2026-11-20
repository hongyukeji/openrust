import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, symlinkSync, rmSync, mkdirSync, copyFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { loadApi } from "../.vitepress/api.mjs";
const root = fileURLToPath(new URL("../../", import.meta.url));
const directory = process.env.DOCS_API_DIR;
assert(directory, "DOCS_API_DIR is required");
const accepted = loadApi(directory, root);
assert.equal(loadApi(undefined, root), null);
const temporary = mkdtempSync(path.join(tmpdir(), "openrust-api-manifest-"));
try {
    for (const crate of accepted.manifest.crates) symlinkSync(path.join(directory, crate.crate), path.join(temporary, crate.crate), "dir");
    symlinkSync(path.join(directory, "index.html"), path.join(temporary, "index.html"));
    const save = (manifest) => writeFileSync(path.join(temporary, "build-info.json"), JSON.stringify(manifest));
    const original = JSON.parse(readFileSync(path.join(directory, "build-info.json"), "utf8"));
    for (const [reason, mutate, error] of [
        ["missing target", manifest => { delete manifest.target; }, /unsupported build information/],
        ["stale lockfile", manifest => { manifest.cargo_lock_sha256 = "0".repeat(64); }, /Cargo.lock differs/],
        ["stale library", manifest => { manifest.source_sha256["vendor/openrust/framework/src/lib.rs"] = "0".repeat(64); }, /source differs/],
        ["escaping source", manifest => { manifest.source_sha256["../outside"] = "0".repeat(64); }, /source file set differs|source differs/],
        ["missing crate", manifest => { manifest.crates[0].index = "missing/index.html"; }, /missing crate index/],
        ["unsupported features", manifest => { manifest.features = "default"; }, /unsupported build information/],
    ]) {
        const manifest = structuredClone(original); mutate(manifest); save(manifest);
        assert.throws(() => loadApi(temporary, root), error, reason);
        console.log(`${reason}: rejected PASS`);
    }
    save(original);
    assert.equal(loadApi(temporary, root).manifest.crates.length, original.crates.length);
    console.log("Matching export: accepted PASS");
    const snapshot = path.join(temporary, "source");
    for (const file of Object.keys(original.source_sha256)) {
        const target = path.join(snapshot, file);
        mkdirSync(path.dirname(target), { recursive: true });
        copyFileSync(path.join(root, file), target);
    }
    assert.equal(loadApi(temporary, snapshot).manifest.revision, original.revision);
    writeFileSync(path.join(snapshot, "vendor/openrust/framework/src/new_api_input.rs"), "// stale export must not accept a new source file\n");
    assert.throws(() => loadApi(temporary, snapshot), /source file set differs/);
    console.log("New source file: rejected PASS");
} finally { rmSync(temporary, { recursive: true }); }
