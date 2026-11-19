#!/usr/bin/env python3
"""Download exact source archives, reject mutations, then run original PHP code."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request
import zipfile

SOURCES = [
    ('laravel/fortify', '1.43.0', '3db382a3d5d6a0d537de32a190ce3b945e621713', 'upstream', 'upstream-manifest.json'),
    ('pragmarx/google2fa', '9.0.0', 'e6bc62dd6ae83acc475f57912e27466019a1f2cf', 'google2fa', 'google2fa-manifest.json'),
    ('paragonie/constant_time_encoding', '3.1.5', 'ceed38ba49d98b85808edfc4f04750ff61eb9739', 'constant_time_encoding', 'encoding-manifest.json'),
]
PHP = 'php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9'

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    root = args.directory.resolve()
    root.mkdir(parents=True, exist_ok=True)
    for package, version, reference, name, manifest in SOURCES:
        lock = json.loads((Path(__file__).parent / manifest).read_text())
        if lock.get('ref', lock.get('source', {}).get('reference')) != reference or lock['version'].lstrip('v') != version:
            raise RuntimeError('Source pin mismatch')
        archive = root / (name+'.zip')
        if not archive.exists():
            with urllib.request.urlopen('https://repo.packagist.org/p2/'+package+'.json') as response:
                releases = json.load(response)['packages'][package]
            release = next(value for value in releases if value['version'].lstrip('v') == version)
            if release['source']['reference'] != reference:
                raise RuntimeError('Registry source mismatch')
            repository = release['source']['url'].removesuffix('.git').removeprefix('https://github.com/')
            url = 'https://codeload.github.com/'+repository+'/zip/'+reference if name == 'upstream' else release['dist']['url']
            with urllib.request.urlopen(url) as response:
                archive.write_bytes(response.read())
        if hashlib.sha256(archive.read_bytes()).hexdigest() != lock['archive_sha256']:
            raise RuntimeError('Archive hash mismatch: '+name)
        target = root/name
        if not target.exists():
            target.mkdir()
            with zipfile.ZipFile(archive) as source:
                for item in source.infolist():
                    relative = Path(*Path(item.filename).parts[1:])
                    if '..' in relative.parts or relative.is_absolute():
                        raise RuntimeError('Unsafe archive path')
                    destination = target/relative
                    if item.is_dir():
                        destination.mkdir(parents=True, exist_ok=True)
                    else:
                        destination.parent.mkdir(parents=True, exist_ok=True)
                        destination.write_bytes(source.read(item))
        for relative, digest in lock['files'].items():
            if hashlib.sha256((target/relative).read_bytes()).hexdigest() != digest:
                raise RuntimeError('Source hash mismatch: '+name+'/'+relative)
        (root/manifest).write_text(json.dumps(lock, indent=2)+'\n')
    oracle = Path(__file__).with_name('two_factor_oracle.php')
    (root/oracle.name).write_bytes(oracle.read_bytes())
    with (root/'two-factor-oracle.json').open('wb') as output:
        subprocess.run(['docker','run','--rm','-v',str(root)+':/work:ro',PHP,'php','/work/'+oracle.name,'/work'],stdout=output,check=True)
    print(root/'two-factor-oracle.json')

if __name__ == '__main__':
    main()
