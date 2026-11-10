"""Compile and execute all Rust container examples in the three languages."""
from pathlib import Path
import json
import re
import subprocess
import sys
from tempfile import TemporaryDirectory
source = Path(sys.argv[1]).resolve()
deps = source / 'target/debug/deps'
library = max(deps.glob('libopenrust-*.rlib'), key=lambda item: item.stat().st_mtime_ns)
tokio = max(deps.glob('libtokio-*.rlib'), key=lambda item: item.stat().st_mtime_ns)
report = []
with TemporaryDirectory(prefix='openrust-01-02c-doc-') as directory:
    root = Path(directory)
    for language in ['en', 'zh-Hans', 'zh-Hant']:
        page = source / f'docs/{language}/architecture-concepts/container.md'
        blocks = re.findall(r'```rust\n(.*?)```', page.read_text(), re.S)
        assert len(blocks) == 4
        for index, block in enumerate(blocks):
            example = root / 'example.rs'
            example.write_text(block)
            binary = root / 'example'
            subprocess.run(['rustc', '+1.94.0', '--edition=2024', '--crate-name=doc_example', '-Dwarnings', str(example), '--extern', f'openrust={library}', '--extern', f'tokio={tokio}', '-L', f'dependency={deps}', '-o', str(binary)], check=True)
            subprocess.run([binary], cwd=source, check=True)
            report.append(dict(language=language, example=index+1, result='passed'))
(Path(__file__).resolve().parent / 'example-results.json').write_text(json.dumps(report, indent=2) + '\n')
print('PASS: 12 documented Rust examples compiled and executed')
