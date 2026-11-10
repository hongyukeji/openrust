from pathlib import Path
from tempfile import TemporaryDirectory
import contextlib, http.client, json, os, re, select, shutil, socket, subprocess, sys

workspace = Path(__file__).resolve().parents[4]
source = Path(sys.argv[1]).resolve()
report = []
keys = set(re.findall(r'"([A-Z][A-Z0-9_]+)"', '\n'.join(p.read_text() for p in (source/'config').glob('*.rs'))))
keys.update(('APP_BASE_PATH','APP_ENV','GRPC_HOST','GRPC_PORT'))
env = {key:value for key,value in os.environ.items() if key not in keys}
env.update(CARGO_NET_OFFLINE='true', RUSTUP_TOOLCHAIN='1.94.0')

def record(name, **details):
    report.append(dict(name=name, **details))
    print('PASS', name, flush=True)

def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1',0))
        return listener.getsockname()[1]

def command(args, cwd, settings, code=0):
    result = subprocess.run(args, cwd=cwd, env=settings, text=True, capture_output=True, timeout=30)
    assert result.returncode == code, (args, result.returncode, result.stderr[:250])
    return result

with TemporaryDirectory(prefix='openrust-00-01-runtime-') as temp:
    root = Path(temp)
    application = root/'deployment'
    welcome = application/'resources/views/welcome.html'
    welcome.parent.mkdir(parents=True)
    welcome.write_text('startup acceptance')
    binary = application/'main'
    shutil.copy2(source/'target/release/main', binary)
    working = root/'foreign-cwd'
    working.mkdir()
    (working/'.env').write_text('APP_DEBUG=not-a-boolean\n')

    def serve(name, settings, expected_port, paths=('/', '/up')):
        with subprocess.Popen([binary], cwd=working, env=settings, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True) as server:
            try:
                assert select.select([server.stdout], [], [], 10)[0], 'listener startup timeout'
                line=server.stdout.readline()
                assert f'listening on http://127.0.0.1:{expected_port}' in line, (name,line,server.poll())
                with contextlib.closing(http.client.HTTPConnection('127.0.0.1',expected_port,timeout=5)) as client:
                    for path in paths:
                        client.request('GET',path)
                        response=client.getresponse()
                        body=response.read().decode()
                        assert response.status==200,(name,path,response.status,body[:80])
                        if path=='/': assert body=='startup acceptance'
                for arguments in [('inspire',), ('list',), ('help','list')]:
                    result=command([binary,'artisan',*arguments],working,settings)
                    assert result.stdout
                record(name, http_requests=len(paths), artisan_invocations=3)
            finally:
                server.terminate()
                server.wait(timeout=10)
                assert server.returncode==0,server.stderr.read()

    p=port()
    serve('standalone without .env, foreign cwd',dict(env,APP_PORT=str(p)),p)
    template=subprocess.check_output(['git','show','HEAD:.env.example'],cwd=workspace,text=True)
    (application/'.env').write_text(template)
    p=port()
    serve('HEAD .env.example plus process APP_PORT override',dict(env,APP_PORT=str(p)),p)

    # Absolute same-origin disk URL forces the effective APP_URL port to match.
    disk=application/'storage/app/public/probe.txt'
    disk.parent.mkdir(parents=True);disk.write_text('disk')
    p=port()
    (application/'.env').write_text(f'APP_PORT={p}\nPUBLIC_STORAGE_URL=http://localhost:{p}/storage/tmp/\n')
    serve('file APP_PORT supplies default app.url',env,p,('/', '/storage/tmp/probe.txt'))
    actual=port();public=port()
    (application/'.env').write_text(f'APP_PORT=8000\nAPP_URL=http://localhost:{public}\nPUBLIC_STORAGE_URL=http://localhost:{public}/storage/tmp/\n')
    serve('explicit APP_URL independent of process listener port',dict(env,APP_PORT=str(actual)),actual,('/', '/storage/tmp/probe.txt'))
    actual=port();public=port()
    (application/'.env').write_text(f'APP_URL=http://localhost:1\nPUBLIC_STORAGE_URL=http://localhost:{public}/storage/tmp/\n')
    serve('process APP_URL overrides file URL',dict(env,APP_PORT=str(actual),APP_URL=f'http://localhost:{public}'),actual,('/', '/storage/tmp/probe.txt'))

    p=port()
    (application/'.env').write_text('APP_DEBUG=not-a-boolean\n')
    (application/'.env.acceptance').write_text(f'APP_PORT={p}\nAPP_DEBUG=false\n')
    serve('APP_ENV selects .env.acceptance',dict(env,APP_ENV='acceptance'),p)
    (application/'.env.acceptance').unlink()
    p=port()
    (application/'.env').write_text(f'APP_PORT={p}\n')
    serve('missing selected file falls back to .env',dict(env,APP_ENV='missing'),p)

    moved=root/'moved-app'
    shutil.move(application,moved)
    application=moved;binary=moved/'main'
    p=port()
    (moved/'.env').write_text(f'APP_PORT={p}\n')
    serve('relocated standalone directory',env,p)
    # Explicit root works when the executable is outside that root.
    outside=root/'outside-main';shutil.copy2(binary,outside)
    binary=outside
    p=port()
    (moved/'.env').write_text(f'APP_PORT={p}\n')
    serve('APP_BASE_PATH absolute override',dict(env,APP_BASE_PATH=str(moved)),p)

    for args in ([],['artisan','inspire']):
        result=command([binary,*args],working,dict(env,APP_BASE_PATH='relative-path'),1)
        assert 'absolute application root' in result.stderr
    record('relative APP_BASE_PATH rejected by both entries')
    for bad in ('invalid','-1','65536'):
        settings=dict(env,APP_BASE_PATH=str(moved),APP_PORT=bad)
        # URL generation and the listener are separate configuration concerns.
        settings['APP_URL']='http://localhost'
        result=command([binary],working,settings,1)
        assert 'app.port must be an integer from 0 to 65535' in result.stderr
        result=command([binary,'artisan','inspire'],working,settings)
        assert '—' in result.stdout
    record('invalid HTTP port rejected by listener, Artisan remains available')

    # Source binary uses the application crate directory, even from a foreign cwd.
    result=command(['cargo','+1.94.0','build','--locked','--offline'],source,env)
    debug=source/'target/debug/main'
    result=command([debug,'artisan','inspire'],working,env)
    assert '—' in result.stdout
    record('source binary from foreign cwd')
    result=command(['./artisan','inspire'],source,env)
    assert '—' in result.stdout
    record('documented source Artisan script')
    result=command(['cargo','+1.94.0','run','--locked','--offline','--','artisan','inspire'],source,env)
    assert '—' in result.stdout
    record('documented cargo run entry')

out=workspace/'.agents/project/audits/00-01/runtime-results.json'
out.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
