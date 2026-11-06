#!/usr/bin/env python3
"""校验核心功能验收清单并按等权已验项汇总，不给部分实现折算分数。"""

import argparse
import collections
import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
RECORD = ROOT / ".agents/project/memory/core-functional-acceptance.md"
STATES = {"已验收", "部分完成", "待开发"}


def slug(value):
    """生成本记录使用的 Markdown 标题锚点。"""
    return re.sub(r"[^\w\- ]", "", value.lower()).replace(" ", "-")


def summarize():
    """读取唯一功能表，核对证据链接和范围，再计算统计。"""
    text = RECORD.read_text()
    rows = []
    seen = set()
    groups = collections.OrderedDict()
    for line in text.splitlines():
        if not re.match(r"\| C\d{3} \|", line):
            continue
        cells = [cell.strip() for cell in line.strip("|").split("|")]
        if len(cells) != 7:
            raise ValueError(f"功能行列数错误：{line}")
        ident, group, upstream, name, state, evidence, gap = cells
        if ident in seen or state not in STATES or not all(cells):
            raise ValueError(f"功能ID、状态或必填字段错误：{ident}")
        seen.add(ident)
        links = re.findall(r"\]\(([^)]+)\)", evidence)
        if not links:
            raise ValueError(f"缺少证据入口：{ident}")
        for link in links:
            path, _, anchor = link.partition("#")
            target = (RECORD.parent / path).resolve()
            if not target.is_relative_to(ROOT) or not target.is_file():
                raise ValueError(f"证据路径不存在或越界：{ident} {link}")
            if anchor:
                headings = {
                    slug(value.lstrip("#").strip())
                    for value in target.read_text().splitlines()
                    if value.startswith("#")
                }
                if anchor not in headings:
                    raise ValueError(f"证据章节不存在：{ident} {link}")
        if not re.findall(r"L\d{3}", upstream):
            raise ValueError(f"缺少固定上游映射：{ident}")
        counts = groups.setdefault(group, collections.Counter())
        counts[state] += 1
        rows.append((ident, upstream, state))
    if not rows:
        raise ValueError("核心功能表为空")
    required = {
        f"L{index:03}" for index in range(1, 71) if index not in (3, 6, 9, 63)
    } | {"L074", "L075", "L076", "L078", "L079"}
    actual = set().union(*(set(re.findall(r"L\d{3}", row[1])) for row in rows))
    if required - actual or actual - required:
        raise ValueError(f"核心章节映射有差集：缺{required - actual}，多{actual - required}")
    result = []
    total = collections.Counter()
    for group, counts in groups.items():
        size = sum(counts.values())
        result.append({
            "group": group, "accepted": counts["已验收"],
            "partial": counts["部分完成"], "open": counts["待开发"],
            "total": size, "percent": round(100 * counts["已验收"] / size, 1),
        })
        total.update(counts)
    size = sum(total.values())
    return {
        "scope_version": 1, "chapter_count": len(required), "total": size,
        "accepted": total["已验收"], "partial": total["部分完成"],
        "open": total["待开发"],
        "percent": round(100 * total["已验收"] / size, 1), "groups": result,
    }


def main():
    """输出可复算统计；默认同时核对正文中的总数声明。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="输出结构化统计")
    args = parser.parse_args()
    result = summarize()
    expected = (
        f"统计校验：总项={result['total']}；已验收={result['accepted']}；"
        f"部分完成={result['partial']}；待开发={result['open']}；"
        f"完成率={result['percent']:.1f}%。"
    )
    if expected not in RECORD.read_text():
        raise ValueError("正文统计与功能表不一致，请按清单重算后更新")
    for group in result["groups"]:
        expected_group = (
            f"| {group['group']} | {group['accepted']} | {group['partial']} | "
            f"{group['open']} | {group['total']} | {group['percent']:.1f}% |"
        )
        if expected_group not in RECORD.read_text():
            raise ValueError(f"领域汇总与功能表不一致：{group['group']}")
    if args.json:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        print(expected)
        for group in result["groups"]:
            print(f"{group['group']}：{group['accepted']}/{group['total']}，{group['percent']:.1f}%")


if __name__ == "__main__":
    main()
