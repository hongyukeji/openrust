"""验证已构建的当前Linux运行时镜像及18语资源发布；先构建docker/Dockerfile。"""
import io, json, os, shutil, subprocess, tarfile, time, urllib.request
from pathlib import Path
root = Path.cwd()
stage = root / 'target/tmp/localization-18-locales/docker'
stage.mkdir(parents=True, exist_ok=True)
base = os.environ.get('LOCALE_DOCKER_BASE', 'openrust:localization-18-final')
tag = 'openrust:localization-18-resources'
for name in ['resources', 'lang', 'public']:
    if (stage / name).exists():
        shutil.rmtree(stage / name)
    shutil.copytree(root / name, stage / name, dirs_exist_ok=True)
for name in ['LICENSE', 'THIRD_PARTY_NOTICES.md']:
    shutil.copy2(root / name, stage / name)
(stage / 'Dockerfile').write_text(f'FROM {base}\nCOPY resources/ /app/resources/\nCOPY lang/ /app/lang/\nCOPY public/ /app/public/\nCOPY LICENSE THIRD_PARTY_NOTICES.md /app/\n')
subprocess.run(['docker', 'build', '--pull=false', '-t', tag, str(stage)], check=True)
args = ['docker', 'run', '-d', '--rm', '-p', '127.0.0.1::8000']
for key, value in dict(APP_BASE_PATH='/app', APP_DISABLED_RUNNERS='queue,schedule,reverb,grpc', APP_KEY='base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=', SESSION_DRIVER='array', CACHE_STORE='array', LOG_CHANNEL='stderr').items():
    args.extend(['-e', f'{key}={value}'])
container = subprocess.check_output([*args, tag], text=True).strip()
try:
    address = subprocess.check_output(['docker', 'port', container, '8000'], text=True).strip()
    base_url = 'http://' + address
    for attempt in range(100):
        try:
            urllib.request.urlopen(base_url + '/up', timeout=2).close()
            break
        except OSError:
            time.sleep(.1)
    else:
        raise AssertionError('容器HTTP启动超时')
    # 容器没有应用日语词典；日语HTTP必须实际消费编入产物的默认值。
    subprocess.run(['docker', 'exec', container, 'test', '!', '-e', '/app/lang/ja/welcome.json'], check=True)
    locales = json.loads((root / '.agents/project/audits/03-07/localization-18-source.json').read_text())['standard_source']['locales']
    for locale in locales:
        request = urllib.request.Request(base_url, headers={'Accept-Language': locale})
        with urllib.request.urlopen(request, timeout=10) as response:
            html = response.read().decode()
            assert response.headers['Content-Language'] == locale
            assert json.loads((root / f'vendor/openrust/framework/src/translation/lang/{locale}/welcome.json').read_text())['heading'] in html
            docs = locale if locale in ['en', 'zh-Hans', 'zh-Hant'] else 'en'
            assert f'/docs/{docs}/README.md' in html
        print(locale, 'Linux runtime resource delivery PASS', flush=True)
    published = [f'{locale}/{group}.json' for locale in locales for group in ['auth', 'passwords', 'pagination', 'validation', 'welcome', 'languages', 'console', 'schema']] + [f'{locale}.json' for locale in locales] + ['NOTICE']
    # 发布写入独立可写应用根，不改变镜像内只读应用资源的所有权。
    subprocess.run(['docker', 'exec', container, 'mkdir', '-p', '/app/storage/language-publish'], check=True)
    subprocess.run(['docker', 'exec', '-e', 'APP_BASE_PATH=/app/storage/language-publish', container, '/app/main', 'artisan', 'lang:publish'], check=True)
    archive = subprocess.check_output(['docker', 'exec', container, 'tar', '-cf', '-', '-C', '/app/storage/language-publish/lang', '.'])
    with tarfile.open(fileobj=io.BytesIO(archive)) as files:
        for name in published:
            expected = root / 'vendor/openrust/framework/src/translation' / ('stubs/NOTICE' if name == 'NOTICE' else 'lang/' + name)
            assert files.extractfile('./' + name).read() == expected.read_bytes(), name
    print('Linux compiled publisher: 162 JSON catalogs and license recreated exactly PASS', flush=True)
    result = {name: subprocess.check_output(['docker', 'image', 'inspect', '--format', '{{.Id}}', image], text=True).strip() for name, image in [('base_image', base), ('resource_image', tag)]}
    (stage / 'images.json').write_text(json.dumps(result, indent=2) + '\n')
finally:
    subprocess.run(['docker', 'stop', container], check=True)
