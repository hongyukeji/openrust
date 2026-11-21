#!/usr/bin/env python3
"""Build and export the committed workspace's public Rust API documentation."""
import argparse
import hashlib
import html
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path, help="New output directory; never overwritten")
    parser.add_argument("--target-dir", required=True, type=Path, help="Dedicated Cargo artifact directory")
    parser.add_argument("--revision", help="Source revision, required when building an exported source archive")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    target = args.target_dir.resolve()
    if output.exists():
        parser.error("--output already exists; choose a new directory")
    if output == target or target in output.parents or output in target.parents:
        parser.error("output and Cargo artifacts must be separate directories")
    revision = args.revision
    checkout = subprocess.run(["git", "rev-parse", "--show-toplevel"], cwd=root, capture_output=True, text=True)
    in_checkout = checkout.returncode == 0 and Path(checkout.stdout.strip()).resolve() == root
    if in_checkout:
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
        if revision is not None and revision != head:
            parser.error("--revision does not match the checkout's HEAD")
        revision = head
        result = subprocess.run(["git", "status", "--porcelain", "--untracked-files=normal"], cwd=root, capture_output=True, text=True, check=True)
        if result.stdout:
            parser.error("source has uncommitted changes; commit or build a clean source archive")
    elif revision is None:
        parser.error("--revision is required for a source archive")
    env = dict(os.environ)
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        parser.error("revision must be a full Git commit SHA")
    env.update(CARGO_TARGET_DIR=str(target), RUSTUP_TOOLCHAIN="1.94.0")
    compiler = subprocess.check_output(["rustc", "+1.94.0", "-vV"], text=True)
    host = next(line.removeprefix("host: ") for line in compiler.splitlines() if line.startswith("host: "))
    env["CARGO_BUILD_TARGET"] = host
    env.setdefault("CARGO_PROFILE_DEV_DEBUG", "0")
    env["RUSTDOCFLAGS"] = " ".join(filter(None, [env.get("RUSTDOCFLAGS"), "-D rustdoc::broken_intra_doc_links -D rustdoc::private_intra_doc_links"]))
    cargo = ["cargo", "+1.94.0"]
    metadata = json.loads(subprocess.check_output(cargo + ["metadata", "--locked", "--no-deps", "--all-features", "--format-version", "1"], cwd=root, env=env))
    members = set(metadata["workspace_members"])
    crates = []
    for package in metadata["packages"]:
        if package["id"] not in members:
            continue
        for item in package["targets"]:
            if "lib" in item["kind"] and item["doc"]:
                crates.append({"package": package["name"], "crate": item["name"], "version": package["version"], "index": f'{item["name"]}/index.html'})
    crates.sort(key=lambda item: item["package"])
    inputs = {root / "Cargo.toml", root / "Cargo.lock"}
    source_roots = set()
    patched = tomllib.loads((root / "Cargo.toml").read_text()).get("patch", {}).get("crates-io", {})
    for dependency in patched.values():
        if "path" in dependency:
            directory = root / dependency["path"]
            source_roots.add(str(directory.relative_to(root)))
            inputs.add(directory / "Cargo.toml")
            for name in ["build.rs", "README.md"]:
                if (directory / name).is_file():
                    inputs.add(directory / name)
            for folder in ["src", "resources", "config", "database", "stubs"]:
                inputs.update(file for file in (directory / folder).rglob("*") if file.is_file())
    for package in metadata["packages"]:
        if package["id"] not in members:
            continue
        directory = Path(package["manifest_path"]).parent
        source_roots.add(str(directory.relative_to(root)))
        inputs.add(directory / "Cargo.toml")
        for name in ["build.rs", "README.md"]:
            if (directory / name).is_file():
                inputs.add(directory / name)
        for folder in ["src", "resources", "config", "database", "stubs"]:
            inputs.update(file for file in (directory / folder).rglob("*") if file.is_file())
    fingerprints = {str(file.relative_to(root)): hashlib.sha256(file.read_bytes()).hexdigest() for file in sorted(inputs)}
    if not crates:
        parser.error("workspace contains no documented libraries")
    # Rustdoc may leave removed items behind when reusing a target directory.
    source = target / host / "doc"
    # Cargo 1.94 clean --doc does not remove explicit-target documentation.
    if source.exists():
        shutil.rmtree(source)
    subprocess.run(cargo + ["doc", "--locked", "--workspace", "--all-features", "--no-deps", "--target", host], cwd=root, env=env, check=True)
    source = target / host / "doc"
    for crate in crates:
        if not (source / crate["index"]).is_file():
            raise RuntimeError(f'Missing Rustdoc page: {crate["index"]}')
    output.parent.mkdir(parents=True, exist_ok=True)
    # Publish the complete export only after every crate succeeded.
    with tempfile.TemporaryDirectory(prefix="openrust-api-", dir=output.parent) as temporary:
        staging = Path(temporary) / "api"
        shutil.copytree(source, staging)
        manifest = {"schema": 1, "revision": revision, "rust": "1.94.0", "target": host, "features": "all", "dependencies_documented": False, "cargo_lock_sha256": hashlib.sha256((root / "Cargo.lock").read_bytes()).hexdigest(), "source_sha256": fingerprints, "source_roots": sorted(source_roots), "crates": crates}
        (staging / "build-info.json").write_text(json.dumps(manifest, indent=2) + "\n")
        links = "\n".join(f'<li><a href="{html.escape(c["index"], quote=True)}">{html.escape(c["package"])}</a> {html.escape(c["version"])}</li>' for c in crates)
        (staging / "index.html").write_text('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>OpenRust API reference</title><main><h1>OpenRust API reference</h1><p>Source: <code>' + html.escape(revision) + '</code></p><p>Target: ' + html.escape(host) + '</p><p>Rust 1.94.0; all workspace features. Dependency APIs link to their external documentation.</p><ul>' + links + '</ul><p><a href="build-info.json">Build information</a></p></main></html>\n')
        # A new directory path prevents accidental replacement of a prior export.
        staging.rename(output)
    print(f"Exported {len(crates)} workspace libraries to {output}")


if __name__ == "__main__":
    main()
