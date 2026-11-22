"""Exercise Artisan message generators using a POSIX pseudo-terminal."""

import errno
import os
from pathlib import Path
import pty
import select
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
