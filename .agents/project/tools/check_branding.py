"""检查正式文件中的来源品牌，仅允许已逐项裁决的运行／分发标识。"""
import json
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parents[3]
audit = json.loads((root / '.agents/project/audits/branding-2026-11-23.json').read_text())
brands = re.compile('laravel|goravel', re.I)
allowed = {(entry['file'].removeprefix('./'), ' '.join(entry['text'].split())) for entry in audit['retained']}
paths = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=root).decode().split('\0')
errors = []
count = 0
for name in sorted(set(paths)):
    if not name or name.startswith('.agents/project/'):
        continue
    path = root / name
    if not path.is_file():
        continue
    if brands.search(name):
        errors.append(f'{name}：正式文件名带有来源品牌')
    try:
        text = path.read_text()
    except UnicodeError:
        continue
    for number, line in enumerate(text.splitlines(), 1):
        if not brands.search(line):
            continue
        if (name, ' '.join(line.split())) not in allowed:
            errors.append(f'{name}:{number}：未裁决的来源品牌表述')
        count += 1
if errors:
    raise SystemExit('\n'.join(errors))
print(f'正式文件名称及内容检查通过；{count}处必要运行／分发标识已有逐项说明。')
