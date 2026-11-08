#!/usr/bin/env python3
"""按实际分配字节核对 OpenRust 构建产物和磁盘预算。"""

import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess


LIMITS = {
    "artifact_soft_bytes": 40_000_000_000,
    "artifact_hard_bytes": 50_000_000_000,
    "disk_soft_percent": 5,
    "disk_hard_percent": 3,
}


def fail_scan(error):
    raise error


def cargo_targets(roots):
    targets = set()
    for root in roots:
        for directory, children, files in os.walk(root, onerror=fail_scan):
            path = Path(directory)
            has_build_lock = any(
                (path / profile / ".cargo-build-lock").is_file()
                for profile in children
            ) if "CACHEDIR.TAG" in files else False
            if "CACHEDIR.TAG" in files and (".rustc_info.json" in files or has_build_lock):
                targets.add(path)
                children.clear()
            elif ".git" in children:
                children.remove(".git")
    return targets


def allocated(path, seen):
    def count(info):
        key = (info.st_dev, info.st_ino)
        if key in seen or stat.S_ISLNK(info.st_mode):
            return 0
        seen.add(key)
        return info.st_blocks * 512

    if path.is_file():
        return count(path.stat())
    size = 0
    for directory, children, files in os.walk(path, onerror=fail_scan):
        parent = Path(directory)
        size += count(parent.lstat())
        for name in files:
            size += count((parent / name).lstat())
    return size


def disk_samples(paths, build_on, growth):
    samples = {}
    build_device = build_on.stat().st_dev
    for path in paths:
        device = path.stat().st_dev
        if device not in samples:
            usage = shutil.disk_usage(path)
            projected = max(0, usage.free - growth) if device == build_device else usage.free
            samples[device] = {
                "path": str(path),
                "total_bytes": usage.total,
                "available_bytes": usage.free,
                "projected_available_bytes": projected,
            }
    return list(samples.values())


def decision(total, disks):
    def below(percent):
        return any(
            disk["projected_available_bytes"] * 100 < disk["total_bytes"] * percent
            for disk in disks
        )

    if total >= LIMITS["artifact_hard_bytes"] or below(LIMITS["disk_hard_percent"]):
        return "hard", 3
    if total >= LIMITS["artifact_soft_bytes"] or below(LIMITS["disk_soft_percent"]):
        return "soft", 2
    return "ok", 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scan-root", action="append", type=Path, default=[],
                        help="扫描已确认归属的外部目录")
    parser.add_argument("--artifact", action="append", type=Path, default=[],
                        help="计入已登记的外置 target、二进制或归档")
    parser.add_argument("--build-on", type=Path, help="预计新增产物所在的现有目录")
    parser.add_argument("--planned-bytes", type=int, default=0, help="预计新增字节数")
    args = parser.parse_args()
    if args.planned_bytes < 0:
        parser.error("--planned-bytes 不能为负数")

    project = Path(__file__).resolve().parents[3]
    worktrees = subprocess.check_output(
        ["git", "-C", str(project), "worktree", "list", "--porcelain", "-z"]
    )
    roots = {Path(os.fsdecode(entry[9:])).resolve(strict=True)
             for entry in worktrees.split(b"\0") if entry.startswith(b"worktree ")}
    roots.update(path.resolve(strict=True) for path in args.scan_root)
    build_on = (args.build_on or project).resolve(strict=True)
    targets = cargo_targets(roots)
    targets.update(path.resolve(strict=True) for path in args.artifact)

    seen = set()
    allocations = {str(path): allocated(path, seen) for path in sorted(targets)}
    total = sum(allocations.values())
    projected = total + args.planned_bytes
    disks = disk_samples([project, build_on, *sorted(targets)], build_on, args.planned_bytes)
    status, code = decision(projected, disks)
    print(json.dumps({
        "status": status,
        "limits": LIMITS,
        "scan_roots": sorted(map(str, roots)),
        "allocated_bytes_by_path": allocations,
        "allocated_bytes": total,
        "planned_bytes": args.planned_bytes,
        "projected_bytes": projected,
        "disk": disks,
    }, ensure_ascii=False, indent=2))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
