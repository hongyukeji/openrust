"""Exercise Artisan message generators using a POSIX pseudo-terminal."""

import errno
import json
import os
from pathlib import Path
import pty
import select
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory
from time import monotonic

binary = Path(sys.argv[1]).resolve()

def artisan(root, args, steps=(), expected=0, locale='en'):
    master, slave = pty.openpty()
    env = dict(os.environ, APP_BASE_PATH=str(root), APP_LOCALE=locale, TERM='xterm', NO_COLOR='1')
    process = subprocess.Popen([binary, 'artisan', *args], env=env, stdin=slave, stdout=slave, stderr=slave)
    os.close(slave)
    consumed, pending = bytearray(), bytearray()
    deadline = monotonic() + 8
    def read():
        timeout = deadline - monotonic()
        assert timeout > 0 and select.select([master], [], [], timeout)[0], (args, consumed)
        try:
            chunk = os.read(master, 65536)
        except OSError as ex:
            if ex.errno != errno.EIO:
                raise
            return False
        consumed.extend(chunk)
        pending.extend(chunk)
        return bool(chunk)
    try:
        for marker, answer in steps:
            marker = marker.encode()
            while marker not in pending:
                assert read(), (args, consumed)
            del pending[:pending.index(marker) + len(marker)]
            os.write(master, answer.encode())
        while read():
            pass
        assert process.wait(timeout=2) == expected, (args, consumed.decode())
        print('terminal', args, 'exit', expected, 'PASS', flush=True)
        return consumed.decode()
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=2)
        os.close(master)

mail = 'What is the name of the mailable?'
notification = 'What is the name of the notification?'
choose = 'Choose [1]'
confirm = 'Would you like to create a markdown view? [Y/n]'
with TemporaryDirectory() as folder:
    root = Path(folder)
    # 独立应用使用正常语言资源，语言选择与回退不再由命令写死。
    languages = Path(__file__).resolve().parents[1] / 'lang'
    for resource in languages.glob('*/console.json'):
        destination = root / 'lang' / resource.parent.name / resource.name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(resource, destination)
    artisan(root, ['make:mail'], [(mail, '\n'), (mail, 'Orders/Ready\n'), (choose, '\n')])
    assert '<x-mail::message>' in (root/'resources/views/mail/orders/ready.blade.html').read_text()
    artisan(root, ['make:mail'], [(mail, 'Billing/InvoiceReady\n'), (choose, '8\n'), (choose, '2\n')])
    assert '<p>' in (root/'resources/views/mail/billing/invoice-ready.blade.html').read_text()
    artisan(root, ['make:mail', '--'], [(mail, 'PlainMessage\n'), (choose, '3\n')])
    assert (root/'app/mail/plain_message.rs').exists() and not (root/'resources/views/mail/plain-message.blade.html').exists()
    out = artisan(root, ['make:mail', '--view'], [(mail, 'ManualView\n')])
    assert choose not in out and (root/'resources/views/mail/manual-view.blade.html').exists()
    out = artisan(root, ['make:mail', 'NamedMessage'])
    assert mail not in out and choose not in out
    artisan(root, ['make:notification'], [(notification, 'Billing/Completed\n'), (confirm, '\n'), ('[mail.billing.completed]', '\n')])
    assert (root/'resources/views/mail/billing/completed.blade.html').exists()
    artisan(root, ['make:notification'], [(notification, 'CustomNotice\n'), (confirm, 'yes\n'), ('[mail.custom-notice]', 'mail.custom.test\n')])
    assert (root/'resources/views/mail/custom/test.blade.html').exists()
    artisan(root, ['make:notification'], [(notification, 'PlainNotice\n'), (confirm, 'no\n')])
    assert not (root/'resources/views/mail/plain-notice.blade.html').exists()
    output = artisan(root, ['make:notification', '--markdown=mail.ordered'], [(notification, 'AlreadyConfigured\n')])
    assert confirm not in output and (root/'resources/views/mail/ordered.blade.html').exists()
    for command, prompt in [('make:mail', mail), ('make:notification', notification)]:
        for flags in [['-n', command], [command, '--no-interaction'], ['-qn', command], [command, '--unknown']]:
            output = artisan(root, flags, expected=1)
            assert prompt not in output and 'Choose [1]' not in output, output
        assert '<name>' in artisan(root, [command, '--help'])
    out = artisan(root, ['make:mail'], [(mail, 'Cancelled\n'), (choose, '\x04')], expected=1)
    assert 'Command cancelled.' in out and not (root/'app/mail/cancelled.rs').exists()
    artisan(root, ['make:mail','--test'], [('邮件类叫什么？', 'SimplifiedMail\n')], locale='zh-Hans')
    artisan(root, ['make:notification','--test'], [('通知類別叫什麼？', 'TraditionalNotice\n')], locale='zh-Hant')
    # 相同命令由统一资源选择文案；新增语言、命令级覆盖及缺词回退无需改Rust。
    for locale, heading, option in [('en', 'Usage:', 'Print help'), ('zh-Hans', '用法：', '显示帮助'), ('zh-Hant', '用法：', '顯示說明')]:
        output = artisan(root, ['help', 'list'], locale=locale)
        assert heading in output and option in output, output
    resource = root / 'lang/fr/console.json'
    resource.parent.mkdir(parents=True)
    resource.write_text(json.dumps({'usage': 'Utilisation:', 'commands': {'list': {'description': 'Liste des commandes', 'arguments': {'help': 'Aide locale'}}}}))
    output = artisan(root, ['help', 'list'], locale='fr')
    assert 'Utilisation:' in output and 'Liste des commandes' in output and 'Aide locale' in output, output
    assert 'Options:' in output, output
