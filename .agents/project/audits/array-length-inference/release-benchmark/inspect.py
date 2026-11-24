"""macOS补充检查：只比较Faker::new本体，不据此证明完整调用链等价。"""
import json
import os
from pathlib import Path
import re
import subprocess
out = Path(os.environ.get('ARRAY_PERF_DIR', Path.cwd() / 'target/tmp/array-release'))
functions = {}
for version in ['before', 'after']:
    process = subprocess.Popen(['otool', '-tvV', str(out / 'bin' / f'{version}_time')], stdout=subprocess.PIPE, text=True)
    active = False
    body = []
    for line in process.stdout:
        if line.rstrip().endswith(':') and '__ZN' in line:
            if active:
                active = False
            if '5Faker3new17h' in line:
                active = True
        elif active:
            body.append(line.rstrip())
    assert process.wait() == 0
    assert body
    functions[version] = body
    (out / f'{version}-faker_new.asm').write_text('\n'.join(body) + '\n')

def opcodes(lines):
    return [re.sub(r'^\s*[0-9a-f]+\s+', '', line).split('\t', 1)[0] for line in lines if line.strip()]
a, b = opcodes(functions['before']), opcodes(functions['after'])
result = {'before_instructions': len(a), 'after_instructions': len(b), 'same_opcode_sequence': a == b,
          'scope': 'Faker::new body only; operands/addresses and callees are not proven equivalent'}
(out / 'assembly-summary.json').write_text(json.dumps(result, indent=2))
print(result)
