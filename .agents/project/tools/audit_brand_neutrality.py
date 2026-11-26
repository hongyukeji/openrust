"""列出品牌线索及保留类别；分类用于复核，不据此自动修改文件或关闭审计。"""
import argparse
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[3]
BRANDS = re.compile(r'open[ _-]*rust|lara[ _-]*vel|gora[ _-]*vel|\borust\b', re.I)


def classify(path, text):
    if path.startswith('.agents/project/'):
        return '内部规则与历史', '内部参考和历史证据保持原貌；现行规则单独更新。'
    if re.search(r'license|notice|copyright|sources', path + ' ' + text, re.I):
        return '来源与许可', '保持分发许可、来源及版权准确，不能机械替换。'
    if re.search(r'OPENRUST_\w+|OpenRust\\|openrust[:.]|laravel[_:]|laravel-(?:mail|pagination|notifications)|openrust/model', text):
        return '既有契约', '导入、事件、配置、缓存或其他既有标识；更名先核对消费者与迁移。'
    if any(term in text for term in ('listening on', 'OpenRust Foundation', 'OpenRust Horizon')):
        return '可观察输出与默认值', '已有消费者或默认展示值，本轮保留；迁移须另行确认。'
    if re.search(r'github\.com|gitee\.com|vendor/openrust-|openrust[-_]|\[package.metadata|name\s*=|APP_NAME|DB_DATABASE', text):
        return '项目与依赖身份', '保留实际坐标、导入、路径、仓库及默认配置。'
    if path.startswith(('tests/', 'docker/')) or '/tests/' in path:
        return '验收与部署标识', '保留测试隔离、数据语义及部署默认值，不为去品牌重写样例数据。'
    return '上下文保留', '结合上下文复核项目身份、功能归属或兼容边界；不是自动清理授权。'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True)
    args = parser.parse_args()
    paths = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=ROOT).decode().split('\0')
    rows = []
    names = []
    for name in sorted(set(paths)):
        if not name:
            continue
        if BRANDS.search(name):
            names.append(name)
        path = ROOT / name
        if not path.is_file():
            continue
        try:
            contents = path.read_text()
        except UnicodeError:
            continue
        for number, line in enumerate(contents.splitlines(), 1):
            if BRANDS.search(line):
                category, reason = classify(name, line)
                rows.append(dict(file=name, line=number, text=line, category=category, reason=reason))
    output = Path(args.output).resolve()
    if not output.is_relative_to(ROOT / 'target/tmp'):
        parser.error('扫描产物须放在项目 target/tmp/ 下')
    output.write_text(json.dumps(dict(filenames=names, matches=rows), ensure_ascii=False, indent=2) + '\n')
    print(f'{len(rows)} 行线索，{len(names)} 个含品牌的路径；结果须结合实际用途裁决。')


if __name__ == '__main__':
    main()
