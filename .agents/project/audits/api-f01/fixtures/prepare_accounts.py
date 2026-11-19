#!/usr/bin/env python3
"""Verify pinned Fortify account controller/contract/response bytes, then reproduce account lifecycle results."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

PHP = 'php@sha256:50956f094e90a716d43bea28087d447555ad80510e386ab8c9c10d8c70d781a9'
FILES = ['src/Contracts/'+name+'.php' for name in ['CreatesNewUsers','UpdatesUserProfileInformation','UpdatesUserPasswords']] + ['src/Http/Controllers/'+name+'.php' for name in ['RegisteredUserController','ProfileInformationController','PasswordController']] + ['src/Http/Responses/'+name+'.php' for name in ['RegisterResponse','ProfileInformationUpdatedResponse','PasswordUpdateResponse']] + ['src/Events/PasswordUpdatedViaController.php']


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('upstream', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    fixtures = Path(__file__).resolve().parent
    upstream = args.upstream.resolve()
    manifest = json.loads((fixtures / 'upstream-manifest.json').read_text())
    if manifest['ref'] != '3db382a3d5d6a0d537de32a190ce3b945e621713':
        raise RuntimeError('Source pin mismatch')
    for source in FILES:
        if hashlib.sha256((upstream / source).read_bytes()).hexdigest() != manifest['files'][source]:
            raise RuntimeError('Source hash mismatch: '+source)
    result = subprocess.run(['docker', 'run', '--rm', '--network', 'none',
                             '-v', str(fixtures) + ':/fixtures:ro',
                             '-v', str(upstream) + ':/upstream:ro', PHP,
                             'php', '/fixtures/accounts.php', '/upstream'],
                            check=True, stdout=subprocess.PIPE)
    cases = json.loads(result.stdout)
    if len(cases) != 4:
        raise RuntimeError('Unexpected oracle case count')
    args.output.write_bytes(result.stdout)
    print(hashlib.sha256(result.stdout).hexdigest())


if __name__ == '__main__':
    main()
