"""真实独立HTTP：内置默认值、应用键覆盖、新语言与星号配置。"""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time
import urllib.error
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument('--binary', required=True)
parser.add_argument('--stage', required=True)
args = parser.parse_args()
root = Path.cwd()
stage = Path(args.stage).resolve()
assert stage.is_relative_to(root / 'target/tmp')
stage.mkdir(parents=True, exist_ok=True)
for name in ['resources', 'public']:
    shutil.copytree(root / name, stage / name, dirs_exist_ok=True)
for locale, heading in [('fr', 'Application French heading'), ('sv', 'Application Swedish heading')]:
    path = stage / f'lang/{locale}/welcome.json'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps({'heading': heading}))
binary = Path(args.binary).resolve()
base_env = os.environ | dict(APP_BASE_PATH=str(stage), APP_ENV='production', APP_DEBUG='false', APP_KEY='base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=', SESSION_DRIVER='array', CACHE_STORE='array', APP_DISABLED_RUNNERS='queue,schedule,reverb,grpc', APP_HOST='127.0.0.1', TMPDIR=str(stage))
for configured in [None, '*', 'en,fr', '']:
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 0))
        port = probe.getsockname()[1]
    env = base_env | {'APP_PORT': str(port)}
    env.pop('APP_SUPPORTED_LOCALES', None)
    if configured is not None:
        env['APP_SUPPORTED_LOCALES'] = configured
    with (stage / ('server-' + str(port) + '.log')).open('w') as log:
        process = subprocess.Popen([str(binary)], cwd=stage, env=env, stdout=log, stderr=log)
        try:
            url = f'http://127.0.0.1:{port}/'
            for _ in range(150):
                try:
                    urllib.request.urlopen(url + 'up', timeout=2).close()
                    break
                except OSError:
                    if process.poll() is not None:
                        raise AssertionError('HTTP启动提前退出')
                    time.sleep(.1)
            for locale in ['fr', 'sv', 'ja']:
                request = urllib.request.Request(url, headers={'Accept-Language': locale})
                with urllib.request.urlopen(request, timeout=10) as response:
                    body = response.read().decode()
                    selected = locale if configured in [None, '*'] or configured == 'en,fr' and locale == 'fr' else 'en'
                    assert f'lang="{selected}"' in body, (configured, locale, selected)
                    # 欢迎页本身也声明内容语言；停用协商不移除此下游响应头。
                    assert response.headers['Content-Language'] == selected
                    if configured == '':
                        assert 'class="language-switcher"' not in body
                    if selected == 'fr':
                        assert 'Application French heading' in body
                        defaults = json.loads((root / 'vendor/openrust/framework/src/localization/lang/fr/welcome.json').read_text())
                        assert defaults['documentation'] in body
                    if selected == 'sv':
                        assert 'Application Swedish heading' in body
                    assert '/docs/en/README.md' in body
            print(repr(configured), 'HTTP default/override/candidates PASS', flush=True)
        finally:
            process.terminate()
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
