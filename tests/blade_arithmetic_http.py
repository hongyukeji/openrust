import json, os, pathlib, shutil, signal, socket, subprocess, tempfile, time, urllib.request, urllib.error
work=pathlib.Path(__file__).resolve().parents[1]
target=pathlib.Path(os.environ.get('CARGO_TARGET_DIR',work/'target'))
binary=target/'debug/main'
with tempfile.TemporaryDirectory(prefix='blade-main-') as directory:
 base=pathlib.Path(directory)
 for name in ['public','resources','storage/app/public']:
  target=base/name
  if (work/name).is_dir(): shutil.copytree(work/name,target,dirs_exist_ok=True)
  else: target.mkdir(parents=True,exist_ok=True)
 env=os.environ.copy(); env.update(APP_BASE_PATH=str(base),APP_ENV='production',APP_DEBUG='false',APP_DISABLED_RUNNERS='queue,schedule,reverb,grpc',SESSION_DRIVER='array',CACHE_STORE='array',LOG_DEPRECATIONS_CHANNEL='single',APP_KEY='base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=')
 for key in ['APP_CONFIG_CACHE','APP_PACKAGES_CACHE','APP_ROUTES_CACHE','APP_EVENTS_CACHE','APP_VIEW_CACHE']:env.pop(key,None)
 def artisan(*args):
  result=subprocess.run([str(binary),'artisan',*args],env=env,cwd=work,capture_output=True,timeout=30)
  assert result.returncode==0,(args,result.stdout,result.stderr)
  return result.stdout
 def template(text):
  time.sleep(.04)
  (base/'resources/views/welcome.blade.html').write_text(text)
 with socket.socket() as probe:probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
 env['APP_PORT']=str(port);template('{{ 1 + 2 * 3 }}|{{ (1+2)*3 }}|{{ 2**3**2 }}|{{ 1e309 }}|{{ -0.0 }}')
 (base/'resources/views/native-part.blade.html').write_text('{{ $value }}')
 (base/'resources/views/components').mkdir(exist_ok=True)
 (base/'resources/views/components/native-num.blade.html').write_text("@props(['value']){{ $value }}")
 log=open(base/'server.log','wb'); child=subprocess.Popen([str(binary)],cwd=work,env=env,stdout=log,stderr=subprocess.STDOUT)
 def request(path='/',headers=None,method='GET'):
  req=urllib.request.Request(f'http://127.0.0.1:{port}{path}',headers=headers or {},method=method)
  try:response=urllib.request.urlopen(req,timeout=10)
  except urllib.error.HTTPError as error:response=error
  return response.status,response.headers,response.read().decode()
 try:
  end=time.monotonic()+20
  while True:
   try: status,headers,body=request(); break
   except (OSError,urllib.error.URLError):
    assert child.poll() is None,(child.returncode,(base/'server.log').read_text())
    assert time.monotonic()<end;time.sleep(.05)
  assert status==200 and body=='7|9|512|INF|-0',(status,body)
  assert request(method='HEAD')[2]==''
  assert request('/up')[0]==200
  print('actual main: precedence, associativity, INF, signed zero, HEAD and health passed',flush=True)
  template('{{ 5.5%2 }}|{{ 0**-1 }}')
  assert request()[2]=='1|INF'
  logfiles=list((base/'storage/logs').glob('*.log')); content='\n'.join(p.read_text() for p in logfiles)
  assert 'Implicit conversion from float 5.5 to int loses precision' in content
  assert 'Power of base 0 and negative exponent is deprecated' in content
  print('actual main: deprecations logged and rendering continued',flush=True)
  for source,expected in [
   ('{{ [INF][0] }}|{{ [NAN][0] }}|{{ [-0.0][0] }}','INF|NAN|-0'),
   ("@include('native-part',['value'=>INF])",'INF'),
   ('<x-native-num :value="NAN" />','NAN'),
   ('<s>A</s>@if(true)\n<b>B</b>@endif\nC','<s>A</s><b>B</b>C'),
   ('中@if(true)<b>B</b>@endif','中<b>B</b>'),
   ('@i{{-- c --}}f(true)<b>A</b>@end{{-- c --}}if','<b>A</b>'),
   ("{{ 'a{{-- c --}}b' }}|{{ 1 {{-- c --}} + 2 }}",'ab|3'),
   ('@verbatim{{ 1/0 }}{{-- keep --}}<x-missing />@endverbatim','{{ 1/0 }}{{-- keep --}}<x-missing />'),
   ('@ver{{-- c --}}batim<b>A</b>@endverbatim','@verbatim<b>A</b>@endverbatim'),
   ("{{ '@verbatim<b>A</b>@endverbatim' }}",'&lt;b&gt;A&lt;/b&gt;'),
   ('{{-- incomplete','{{-- incomplete'),
   ('<b>p</b>{{-- {{-- nested --}}@if(true)<i>A</i>@endif','<b>p</b><i>A</i>'),
   ('@IF(true)<b>B</b>@ENDIF','<b>B</b>'),
   ('@@ @@unknown @if::custom(true)','@@ @unknown @if::custom(true)'),
   ('\0 \t\r\n<p>trimmed</p>','<p>trimmed</p>'),
   ('\f<p>kept</p>','\f<p>kept</p>'),
  ]:
   template(source);status,_,body=request();assert status==200 and body==expected,(source,status,body,expected)
  print('actual main: Blade boundaries, case, escapes, close-tag newlines and PHP leading trim passed',flush=True)
  for invalid in ['a{{-- {{-- nested --}}@if(true)<b>A</b>@endif', '@if(true)word@endif', '<b>prefix</b>@if\n(true)<b>x</b>@endif', '{{ 1/0 }}','{{ 1+$missing }}','{{ "tail"+1 }}','@js([INF])', '{{ Js::from([NAN]) }}']:
   template(invalid)
   status,headers,body=request(headers={'Accept':'application/json'})
   assert status==500 and 'application/json' in headers['Content-Type']
   json.loads(body)
   status,headers,body=request()
   assert status==500 and 'text/html' in headers['Content-Type']
  print('actual main: division, undefined value, invalid numeric input and non-finite JSON encoding use JSON/HTML 500',flush=True)
  template('{{ 1+2 }}')
  assert request()[2]=='3'
  assert artisan('view:cache')
  assert request()[2]=='3'
  assert artisan('view:clear')
  assert artisan('inspire').strip()
  result=subprocess.run(['./artisan','inspire'],cwd=work,env=env,capture_output=True,timeout=180)
  assert result.returncode==0,(result.stdout,result.stderr)
  print('actual main: recovery, view:cache, view:clear, inspire and ./artisan inspire passed',flush=True)
  child.send_signal(signal.SIGINT);assert child.wait(timeout=20)==0
  with socket.socket() as check:
   check.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);check.bind(('127.0.0.1',port))
  print('actual main: SIGINT exit 0 and port released',flush=True)
 finally:
  if child.poll() is None: child.kill();child.wait()
  log.close()
