#!/usr/bin/env python3
"""以指定 PHP CLI 和校验后的固定官方源码重新生成请求 URL 查询串参考。"""
import argparse
import hashlib
import json
import pathlib
import os
import tempfile
import subprocess
import urllib.request

HERE = pathlib.Path(__file__).resolve().parent


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--php', required=True, type=pathlib.Path)
    parser.add_argument('--source-dir', required=True, type=pathlib.Path)
    parser.add_argument('--output-dir', type=pathlib.Path, default=HERE)
    parser.add_argument('--php-sha256', help='可信 CLI 校验值；默认使用本次已验收二进制的固定值')
    args = parser.parse_args()
    provenance = json.loads((HERE / 'provenance.json').read_text())
    binary = args.php.resolve(strict=True)
    actual_binary_sha = sha256(binary)
    expected_binary_sha = args.php_sha256 or provenance['php_runtime']['binary_sha256']
    if actual_binary_sha != expected_binary_sha:
        raise SystemExit('PHP CLI SHA256 与明确指定的可信校验值不符')
    version = subprocess.check_output([str(binary), '-n', '-r', 'echo PHP_VERSION;'], text=True)
    if version != '8.5.0':
        raise SystemExit('参考版本必须是 PHP 8.5.0，实际为 ' + version)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    for source in provenance['sources']:
        path = args.source_dir / source['local_path']
        path.parent.mkdir(parents=True, exist_ok=True)
        if not path.exists():
            with urllib.request.urlopen(source['url']) as response:
                data = response.read()
            if hashlib.sha256(data).hexdigest() != source['sha256']:
                raise SystemExit('官方源码下载 SHA256 不符：' + source['url'])
            path.write_bytes(data)
        if sha256(path) != source['sha256']:
            raise SystemExit('缓存源码 SHA256 不符：' + str(path))
    outputs = {}
    generated_bytes = {}
    command_prefix = [str(binary), '-n', '-d', 'max_input_vars=4096', '-d', 'max_input_nesting_level=64']
    for tag in ['v7.4.0', 'v8.0.0']:
        command = command_prefix + [str(HERE / 'reference.php'), str(args.source_dir.resolve()), tag, str(HERE / 'cases.json')]
        result = subprocess.run(command, check=True, capture_output=True)
        if result.stderr:
            raise SystemExit('PHP stderr 非空：' + result.stderr.decode())
        output = json.loads(result.stdout)
        if output['warnings']:
            raise SystemExit('PHP 参考产生警告：' + repr(output['warnings']))
        outputs[tag] = output
        generated_bytes[tag] = result.stdout
    if outputs['v7.4.0']['canonical'] != outputs['v8.0.0']['canonical'] or outputs['v7.4.0']['operations'] != outputs['v8.0.0']['operations']:
        raise SystemExit('固定 Symfony 两线参考结果不一致')
    # 两线均成功并通过全部校验后，才以临时文件原子替换各结果。
    for tag, data in generated_bytes.items():
        with tempfile.NamedTemporaryFile(dir=args.output_dir, prefix='.url-golden-', delete=False) as staged:
            staged.write(data)
            temporary = pathlib.Path(staged.name)
        try:
            os.replace(temporary, args.output_dir / ('golden-' + tag + '.json'))
        finally:
            temporary.unlink(missing_ok=True)
    report = {
        'php_version': version,
        'php_binary_sha256': actual_binary_sha,
        'cases_sha256': sha256(HERE / 'cases.json'),
        'reference_script_sha256': sha256(HERE / 'reference.php'),
        'generator_script_sha256': sha256(HERE / 'generate.py'),
        'canonical_per_line': len(outputs['v7.4.0']['canonical']),
        'operations_per_line': len(outputs['v7.4.0']['operations']),
        'symfony_results_equal': True,
        'warning_count': 0,
        'runtime_metadata': [outputs[tag]['metadata'] for tag in outputs],
        'output_sha256': {tag: sha256(args.output_dir / ('golden-' + tag + '.json')) for tag in outputs},
    }
    (args.output_dir / 'reference-run.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps(report, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
