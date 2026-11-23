"""Settings 协同验收：独占 Redis 8，可组合已有三库隔离入口，取消后回收容器。"""
import argparse
import json
import os
import secrets
import signal
import subprocess
import sys
import time


def docker(*args):
    result = subprocess.run(['docker', *args], capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise RuntimeError(f'Docker {args[0]} 失败，退出码 {result.returncode}')
    return result.stdout.strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', choices=['sqlite', 'mysql', 'pgsql'], default='sqlite')
    parser.add_argument('--redis', action='store_true')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('需要验收命令')
    run_id = secrets.token_hex(12)
    env = os.environ.copy() | {'SETTINGS_TEST_DRIVER': args.driver}
    for key in ['SETTINGS_TEST_REDIS_URL', 'SETTINGS_TEST_REDIS_CONTAINER', 'SETTINGS_TEST_RUN_ID']:
        env.pop(key, None)
    received = None

    def cancel(signum, _frame):
        nonlocal received
        received = received or signum

    for signum in [signal.SIGINT, signal.SIGTERM, signal.SIGHUP]:
        signal.signal(signum, cancel)
    try:
        if args.redis:
            image, = json.loads(docker('image', 'inspect', 'redis:8'))
            if set(image['Config'].get('Volumes') or {}) - {'/data'}:
                raise RuntimeError('Redis 镜像数据目录与验收挂载不符')
            password = secrets.token_hex(24)
            name = f'settings-sync-redis-{run_id}'
            container = docker('run', '-d', '--rm', '--name', name, '--label',
                               f'org.openrust.settings-sync={run_id}', '--tmpfs', '/data:rw,size=128m',
                               '-p', '127.0.0.1::6379', image['Id'], 'redis-server', '--dir', '/data', '--save', '',
                               '--appendonly', 'no', '--requirepass', password)
            info, = json.loads(docker('inspect', container))
            port = info['NetworkSettings']['Ports']['6379/tcp'][0]['HostPort']
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                try:
                    if docker('exec', '-e', f'REDISCLI_AUTH={password}', container, 'redis-cli', 'PING') == 'PONG':
                        break
                except RuntimeError:
                    pass
                time.sleep(.1)
            else:
                raise RuntimeError('Redis 未就绪')
            env.update(SETTINGS_TEST_REDIS_URL=f'redis://:{password}@127.0.0.1:{port}/0',
                       SETTINGS_TEST_REDIS_CONTAINER=name, SETTINGS_TEST_RUN_ID=run_id)
            print('独占 Redis 8 已就绪（凭据不输出）', flush=True)
        if args.driver != 'sqlite':
            wrapper = 'mysql_acceptance.py' if args.driver == 'mysql' else 'postgres_acceptance.py'
            command = [sys.executable, '-B', os.path.join(os.path.dirname(__file__), wrapper), '--', *command]
        with subprocess.Popen(command, env=env, start_new_session=True) as process:
            while process.poll() is None:
                if received:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=20)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait()
                    break
                time.sleep(.1)
            code = process.returncode
    finally:
        # 标签限定本批资源，保留所有其他容器、缓存及数据卷。
        if args.redis:
            containers = docker('ps', '-aq', '--filter', f'label=org.openrust.settings-sync={run_id}').splitlines()
            for container in containers:
                docker('rm', '-f', container)
            if docker('ps', '-aq', '--filter', f'label=org.openrust.settings-sync={run_id}'):
                raise RuntimeError('本批 Redis 容器未回收')
    raise SystemExit(128 + received if received else code)


if __name__ == '__main__':
    main()
