"""构建真实应用并验收原生 Blade；所有副本与产物由调用方放入项目 target/tmp/。"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time
import urllib.request
import urllib.error


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--work',type=Path,required=True)
    parser.add_argument('--source',type=Path,required=True)
    args=parser.parse_args();work=args.work.resolve();source=args.source.resolve()
    project=Path(__file__).resolve().parents[1]
    assert work.is_relative_to(project/'target/tmp')
    assert source.is_relative_to(project/'target/tmp')
    work.mkdir(parents=True,exist_ok=True)
    consumer=work/'consumer'
    if consumer.exists():shutil.rmtree(consumer)
    shutil.copytree(source/'tests/fixtures/blade-native',consumer)
    for manifest in [consumer/'Cargo.toml',consumer/'package/Cargo.toml']:
        text=manifest.read_text()
        for prefix in ['../../../../','../../../']:
            text=text.replace('"'+prefix+'vendor/openrust-', '"'+str(source/'vendor')+'/openrust-')
        if manifest.parent==consumer:
            patches=source.joinpath('Cargo.toml').read_text().split('[patch.crates-io]\n',1)[1].split('\n[',1)[0]
            patches=patches.replace('path = "vendor/','path = "'+str(source/'vendor')+'/')
            text+='\n[patch.crates-io]\n'+patches+'\n'
        manifest.write_text(text)
    shutil.copyfile(source/'Cargo.lock',consumer/'Cargo.lock')
    target=Path(os.environ['CARGO_TARGET_DIR']);env=dict(os.environ)
    marker=work/'business-effects';marker.unlink(missing_ok=True);env['NATIVE_SIDE_EFFECT']=str(marker)
    count=0
    def run(name,cmd,expected=0,cwd=consumer,extra=None):
        nonlocal count
        start=time.monotonic();result=subprocess.run(cmd,cwd=cwd,env=env if extra is None else extra,capture_output=True,text=True)
        (work/(name+'.log')).write_text(result.stdout+result.stderr)
        if expected==0:assert result.returncode==0,(name,result.returncode,(result.stdout+result.stderr)[-6000:])
        else:assert result.returncode!=0,name+' unexpectedly passed'
        print(name,'PASS',round(time.monotonic()-start,2),'s',flush=True);count+=1
        return result.stdout+result.stderr
    manifest=str(consumer/'Cargo.toml')
    run('lock',['cargo','+1.95.0','metadata','--format-version','1','--offline','--manifest-path',manifest])
    cargo=['cargo','+1.95.0','build','--locked','--offline','--features','standalone','--manifest-path',manifest]
    run('build',cargo)
    assert not marker.exists(),'构建执行了业务代码'
    binary=target/'debug/blade-native-consumer'
    deploy=work/'deploy';deploy.mkdir(exist_ok=True)
    shutil.copyfile(binary,deploy/'main');(deploy/'main').chmod(0o755)
    old=deploy/'main-before-rebuild';shutil.copyfile(binary,old);old.chmod(0o755)
    def resources():
        shutil.copytree(consumer/'resources',deploy/'resources',dirs_exist_ok=True)
        shutil.copytree(consumer/'templates',deploy/'templates',dirs_exist_ok=True)
        shutil.copytree(consumer/'package/resources/views',deploy/'packages/native/views',dirs_exist_ok=True)
    resources()
    for locale,hello in [('en','Hello'),('zh-Hans','你好'),('zh-Hant','您好')]:
        directory=deploy/'lang'/locale;directory.mkdir(parents=True,exist_ok=True)
        (directory/'messages.json').write_text(json.dumps({'hello':hello}))
        (deploy/'lang'/(locale+'.json')).write_text(json.dumps({'hello':hello}))
    runtime=dict(env,PATH='/unavailable-build-tools')
    runtime.pop('APP_BASE_PATH',None)
    run('cache-no-effects',[str(deploy/'main'),'artisan','view:cache'],cwd=deploy,extra=runtime)
    assert not marker.exists(),'view:cache 执行了业务代码'
    run('self-test',[str(deploy/'main'),'self-test'],cwd=deploy,extra=runtime)
    override=deploy/'resources/views/vendor/native/overridden.blade.html'
    override_source=override.read_text();override.unlink()
    try:run('package-fallback',[str(deploy/'main'),'self-test'],cwd=deploy,extra=dict(runtime,NATIVE_PACKAGE_DEFAULT='1'))
    finally:override.write_text(override_source)
    assert marker.exists(),'原生函数没有执行实际业务副作用'
    before=marker.read_bytes()
    run('clear-no-effects',[str(deploy/'main'),'artisan','view:clear'],cwd=deploy,extra=runtime)
    assert marker.read_bytes()==before
    run('self-test-after-clear',[str(deploy/'main'),'self-test'],cwd=deploy,extra=runtime)
    path=deploy/'resources/views/proof.blade.html';original=path.read_text();path.write_text(original)
    # 普通 HTML 更新无需重新构建，真实函数仍按原文校验。
    path.write_text(original+'<!--html-change-->')
    run('html-only-cache',[str(deploy/'main'),'artisan','view:cache'],cwd=deploy,extra=runtime)
    path.write_text(original)
    for name,changed in [('changed',original.replace('let total: i64','let total: u64')),('added',original+'@rust context.set("new",1); @endrust')]:
        path.write_text(changed)
        out=run('reject-'+name,[str(deploy/'main'),'artisan','view:cache'],1,cwd=deploy,extra=runtime)
        assert '重新构建' in out
    path.write_text(original)
    branch=deploy/'resources/views/branch.blade.html';branch_original=branch.read_text()
    first=branch_original.find('@rust');end=branch_original.find('@endrust',first)+8
    branch.write_text(branch_original[:first]+branch_original[end:]+branch_original[first:end])
    run('reject-reordered',[str(deploy/'main'),'artisan','view:cache'],1,cwd=deploy,extra=runtime);branch.write_text(branch_original)
    # 真实类型检查、所有权和包私有性失败，不以解析器关键词检查代替 rustc。
    invalid=consumer/'resources/views/invalid.blade.html'
    for name,body,diagnostic in [
        ('type','let value: u32 = "invalid";','error[E0308]'),
        ('borrow','let s=String::from("x"); let moved=s; context.set("v",s);','error[E0382]'),
        ('private','::native_package::private_function();','error[E0603]'),
        ('await','async {}.await;','error[E0728]'),
        ('return','return String::from("bad");','error[E0308]'),
        ('syntax','let = ;','原生块 1')]:
        invalid.write_text('@rust '+body+' @endrust')
        out=run('compile-fail-'+name,cargo,1)
        assert diagnostic in out,(name,out[-2000:])
        assert 'invalid.blade.html' in out
    invalid.write_text('@rust let value=1;')
    assert '缺少 @endrust' in run('compile-fail-unclosed',cargo,1)
    invalid.unlink()
    run('rebuild-after-delete',cargo)
    # 新增、修改、删除模板都必须触发 Cargo 发现；旧二进制读取新模板必须失败。
    dynamic=consumer/'resources/views/new.blade.html';dynamic.write_text('@rust context.set("x",7); @endrust{{ $x }}')
    run('rebuild-added',cargo);resources()
    assert '重新构建' in run('old-binary-new-template',[str(old),'artisan','view:cache'],1,cwd=deploy,extra=runtime)
    shutil.copyfile(binary,deploy/'main')
    run('new-binary-new-template',[str(deploy/'main'),'artisan','view:cache'],cwd=deploy,extra=runtime)
    dynamic.write_text('@rust context.set("x",8); @endrust{{ $x }}');run('rebuild-modified',cargo);resources()
    assert '重新构建' in run('old-binary-changed-template',[str(deploy/'main'),'artisan','view:cache'],1,cwd=deploy,extra=runtime)
    shutil.copyfile(binary,deploy/'main');run('new-binary-changed-template',[str(deploy/'main'),'artisan','view:cache'],cwd=deploy,extra=runtime)
    dynamic.unlink();run('rebuild-deleted',cargo);shutil.copyfile(binary,deploy/'main')
    assert '重新构建' in run('deleted-source-old-deploy-rejected',[str(deploy/'main'),'artisan','view:cache'],1,cwd=deploy,extra=runtime)
    (deploy/'resources/views/new.blade.html').unlink()
    run('cache-restored',[str(deploy/'main'),'artisan','view:cache'],cwd=deploy,extra=runtime)
    # 独立部署运行时移走构建模板，PATH 无 Cargo/rustc。部署只保留二进制与运行资源。
    hidden=work/'consumer-hidden';consumer.rename(hidden)
    with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
    runtime['APP_PORT']=str(port)
    server_log=(work/'http-server.log').open('w')
    server=subprocess.Popen([str(deploy/'main')],cwd=deploy,env=runtime,stdout=server_log,stderr=subprocess.STDOUT)
    try:
        url=f'http://127.0.0.1:{port}/native'
        for _ in range(150):
            try:urllib.request.urlopen(url,timeout=.3);break
            except Exception:
                assert server.poll() is None,(work/'http-server.log').read_text();time.sleep(.1)
        else:raise AssertionError('独立 HTTP 服务未就绪')
        def request(pair):
            n,locale=pair;req=urllib.request.Request(url,headers={'X-Name':f'<user{n}>','Accept-Language':locale})
            text=urllib.request.urlopen(req,timeout=10).read().decode()
            hello={'en':'Hello','zh-Hans':'你好','zh-Hant':'您好'}[locale]
            assert text.strip()==f'&lt;&lt;user{n}&gt;:{locale}&gt;|{hello}|nested-local',(pair,text)
        with ThreadPoolExecutor(max_workers=8) as pool:list(pool.map(request,[(n,l) for n in range(8) for l in ['en','zh-Hans','zh-Hant']]))
        http_view=deploy/'resources/views/http.blade.html';http_original=http_view.read_text()
        for changed in [http_original.replace('context.set("message",','context.set("changed_message",'),http_original+'@rust let new_code=1; @endrust']:
            http_view.write_text(changed)
            try:urllib.request.urlopen(url,timeout=10)
            except urllib.error.HTTPError as error:assert error.code==500
            else:raise AssertionError('运行期执行了不匹配的旧原生函数')
        http_view.write_text(http_original+'<b>HTML only</b>')
        assert urllib.request.urlopen(url,timeout=10).read().decode().endswith('<b>HTML only</b>')
        http_view.write_text(http_original)
        request((99,'en'))
        print('HTTP isolated deployment 24 concurrent language/data consumers, runtime mismatch rejection and HTML refresh PASS',flush=True);count+=1
    finally:
        server.terminate();server.wait(timeout=15);server_log.close();hidden.rename(consumer)
    print('PASS',count,'actual consumer stages; no toolchain at runtime; no build/cache business execution',flush=True)

if __name__=='__main__':main()
