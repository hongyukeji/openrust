"""Actual Passport-01 binary / publish / rebuild / MySQL acceptance in an isolated source copy."""
import argparse, hashlib, json, os, pathlib, signal, socket, subprocess, time, urllib.error, urllib.request
parser=argparse.ArgumentParser();parser.add_argument('--isolated-source',action='store_true',required=True);parser.parse_args()
work=pathlib.Path(__file__).resolve().parents[1]
assert not (work/'.git').exists(), 'Run in an exported source snapshot, never the shared checkout'
assert os.environ.get('OPENRUST_REQUIRE_MYSQL')=='1' and os.environ.get('OPENRUST_TEST_MYSQL_URL')
container=os.environ['OPENRUST_ACCEPTANCE_MYSQL_CONTAINER']
inspection=subprocess.run(['docker','inspect',container],capture_output=True,text=True,check=True)
owner=json.loads(inspection.stdout)[0]
assert owner['Name'].startswith('/openrust-acceptance-') and owner['Config']['Labels'].get('org.openrust.acceptance.run'), 'Requires the dedicated mysql_acceptance.py instance'
binary=pathlib.Path(os.environ.get('CARGO_TARGET_DIR',work/'target'))/'debug/main'
env=os.environ.copy();env.update(APP_BASE_PATH=str(work),APP_ENV='production',APP_DEBUG='false',APP_DISABLED_RUNNERS='queue,schedule,reverb,grpc',APP_KEY='base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=',DB_CONNECTION='mysql',DB_URL=os.environ['OPENRUST_TEST_MYSQL_URL'],DB_PREFIX='',PASSPORT_CONNECTION='mysql',SESSION_DRIVER='array',CACHE_STORE='array')
for key in ['APP_CONFIG_CACHE','APP_PACKAGES_CACHE','APP_ROUTES_CACHE','APP_EVENTS_CACHE','APP_VIEW_CACHE','APP_STORAGE_PATH','PASSPORT_PRIVATE_KEY','PASSPORT_PUBLIC_KEY']:env.pop(key,None)
(work/'storage').mkdir(exist_ok=True)
def run(args,expected=0,timeout=60):
 result=subprocess.run(args,cwd=work,env=env,capture_output=True,text=True,timeout=timeout)
 assert result.returncode==expected,(args,result.returncode,result.stdout,result.stderr)
 assert 'BEGIN PRIVATE KEY' not in result.stdout+result.stderr
 return result

def artisan(*args,expected=0):return run([str(binary),'artisan',*args],expected).stdout

def sql(query):
 command=['docker','exec',container,'sh','-ec','MYSQL_PWD="$MYSQL_ROOT_PASSWORD" mysql --protocol=TCP -h 127.0.0.1 -uroot --batch --skip-column-names "$MYSQL_DATABASE" -e "$1"','passport-acceptance',query]
 result=subprocess.run(command,capture_output=True,text=True,timeout=20);assert result.returncode==0,result.stderr
 return result.stdout

def build(features=True):
 args=['cargo','build','--locked','--bin','main']
 if features:args+=['--features','passport']
 run(args,timeout=900)

def pair(directory):return tuple(hashlib.sha256((directory/name).read_bytes()).hexdigest()for name in ['oauth-public.key','oauth-private.key'])

assert '4096' in artisan('passport:keys','--help')
assert 'Encryption keys generated successfully.' in artisan('passport:keys')
storage=work/'storage';before=pair(storage)
assert 'Encryption keys already exist' in artisan('passport:keys',expected=1);assert pair(storage)==before
assert 'Encryption keys generated successfully.' in artisan('passport:keys','--force','--length=2048');assert pair(storage)!=before
assert (storage/'oauth-private.key').read_text().startswith('-----BEGIN PRIVATE KEY-----')
if os.name!='nt':
 assert (storage/'oauth-private.key').stat().st_mode&0o777==0o600
 assert (storage/'oauth-public.key').stat().st_mode&0o777==0o660
artisan('passport:keys','--force','--length=invalid',expected=1)
print('actual binary: keys default 4096, overwrite protection, force 2048, errors and Unix permissions PASS',flush=True)

artisan('migrate','--force')
assert 'oauth_clients' not in sql('SHOW TABLES'), 'Provider must not auto-install OAuth tables'
artisan('vendor:publish','--tag=passport-migrations')
files=list((work/'database/migrations').glob('*_create_oauth_*.rs'));assert len(files)==5
artisan('vendor:publish','--tag=passport-migrations');assert len(list((work/'database/migrations').glob('*_create_oauth_*.rs')))==5
artisan('vendor:publish','--tag=passport-config')
config=work/'config/passport.rs';assert config.exists()
source=config.read_text().replace('"guard": "web"','"guard": "staff"');assert '"staff"' in source;config.write_text(source)
artisan('vendor:publish','--tag=passport-config');assert config.read_text()==source
# Rust configuration and migrations are compiled. Apply the documented module wiring in this isolated copy.
module=work/'config/mod.rs';text=module.read_text();assert 'mod passport;' not in text
text=text.replace('mod view;','mod view;\nmod passport;').replace('("auth".to_owned(), auth::load(env)?),','("auth".to_owned(), auth::load(env)?),\n        ("passport".to_owned(), passport::load(env)?),')
module.write_text(text);build()
assert 'staff' in artisan('config:show','passport')
artisan('migrate','--force');tables=sql('SHOW TABLES');assert all(name in tables for name in ['oauth_auth_codes','oauth_access_tokens','oauth_refresh_tokens','oauth_clients','oauth_device_codes'])
assert 'Nothing to migrate' in artisan('migrate','--force')
artisan('migrate:status')
artisan('config:cache');env['PASSPORT_CONNECTION']='not-a-connection';artisan('migrate:status');artisan('config:clear');env['PASSPORT_CONNECTION']='mysql'
artisan('migrate:rollback','--force');assert 'oauth_clients' not in sql('SHOW TABLES')
artisan('migrate','--force');artisan('migrate:fresh','--force');assert 'oauth_device_codes' in sql('SHOW TABLES')
print('actual binary: publish, preserve overrides, recompile, migrate/repeat/status/cache/rollback/rebuild/fresh on MySQL 8.4 PASS',flush=True)

custom=work/'storage-custom';custom.mkdir();old=pair(storage);env['APP_STORAGE_PATH']=str(custom);artisan('passport:keys','--length=2048');assert all((custom/name).exists()for name in ['oauth-private.key','oauth-public.key']);assert pair(storage)==old;env.pop('APP_STORAGE_PATH')
with socket.socket()as probe:probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
env['APP_PORT']=str(port)
log=open(work/'passport-acceptance-server.log','wb');server=subprocess.Popen([str(binary)],cwd=work,env=env,stdout=log,stderr=subprocess.STDOUT)
def request(path,headers=None):
 try:response=urllib.request.urlopen(urllib.request.Request(f'http://127.0.0.1:{port}{path}',headers=headers or {}),timeout=10)
 except urllib.error.HTTPError as error:response=error
 with response:return response.status,response.headers,response.read()
try:
 deadline=time.monotonic()+30
 while True:
  try:result=request('/up');break
  except (OSError,urllib.error.URLError):
   assert server.poll()is None,'HTTP process exited';assert time.monotonic()<deadline;time.sleep(.05)
 assert result[0]==200;assert request('/')[0]==200
 status,headers,body=request('/oauth/token',{'Accept':'application/json'});assert status==404 and 'application/json' in headers['Content-Type'];json.loads(body)
 status,headers,_=request('/missing');assert status==404 and 'text/html' in headers['Content-Type']
 server.send_signal(signal.SIGINT);assert server.wait(timeout=20)==0
 with socket.socket()as probe:probe.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);probe.bind(('127.0.0.1',port))
finally:
 if server.poll()is None:server.kill();server.wait()
 log.close()
print('actual binary: custom storage, HTTP root/health, JSON/HTML 404, absent OAuth placeholders, SIGINT and port release PASS',flush=True)
run(['./artisan','inspire'],timeout=900)
# The normal framework independent entry still builds without enabling the optional Passport provider.
artisan('passport:keys','--help',expected=1)
print('actual ./artisan inspire and default feature opt-in boundary PASS',flush=True)

# Enable the package in the default features exactly as documented; preserve other feature definitions.
manifest=work/'Cargo.toml';text=manifest.read_text();assert '\ndefault = ' not in text
manifest.write_text(text.replace('[features]\n','[features]\ndefault = ["passport"]\n',1))
run(['./artisan','passport:keys','--help'],timeout=900)
assert 'Encryption keys already exist' in run(['./artisan','passport:keys'],expected=1,timeout=900).stdout
print('documented Cargo default feature and ./artisan passport:keys entry PASS',flush=True)
