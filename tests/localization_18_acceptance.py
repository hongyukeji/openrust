"""实际独立二进制的十八语 CLI 与语言发布保护验收，不操作开发数据。"""
import argparse,json,os,subprocess,shutil
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('--binary',required=True);parser.add_argument('--stage',required=True);args=parser.parse_args()
root=Path.cwd();stage=Path(args.stage).resolve();assert stage.is_relative_to(root/'target/tmp')
stage.mkdir(parents=True,exist_ok=True);app=stage/'application';app.mkdir(exist_ok=True)
for name in ['lang','resources','public']:shutil.copytree(root/name,app/name,dirs_exist_ok=True)
binary=Path(args.binary).resolve();env=os.environ|dict(APP_BASE_PATH=str(app),APP_ENV='production',APP_DISABLED_RUNNERS='queue,schedule,reverb,grpc',SESSION_DRIVER='array',CACHE_STORE='array',TMPDIR=str(stage),APP_KEY='base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=')
locales=['en','zh-Hans','zh-Hant','ja','ko','fr','de','es','pt','it','ru','ar','hi','id','vi','th','tr','nl']
def run(argv,code=0,extra=None):
 result=subprocess.run([str(binary),'artisan','--no-ansi',*argv],cwd=app,env=env|(extra or {}),text=True,capture_output=True,timeout=40)
 assert result.returncode==code,(argv,result.returncode,result.stdout,result.stderr)
 return result.stdout+result.stderr
for locale in locales:
 console=json.loads((root/f'vendor/openrust/framework/src/localization/lang/{locale}/console.json').read_text())
 output=run(['inspire','--help'],extra={'APP_LOCALE':locale});assert console['commands']['inspire']['description'] in output,(locale,output)
 output=run(['missing-locale-command'],1,{'APP_LOCALE':locale});assert console['command_missing'].replace(':name','missing-locale-command') in output,(locale,output)
 print(locale,'CLI localized help/error PASS',flush=True)
for relative in ['lang','resources/lang']:
 target=stage/relative.replace('/','-');target.mkdir(exist_ok=True);catalogs=target/relative;catalogs.mkdir(parents=True,exist_ok=True)
 custom=catalogs/'ja/validation.json';custom.parent.mkdir(exist_ok=True);custom.write_text('{"custom":"preserve"}\n')
 outside=catalogs/'custom-extra.json';outside.write_text('{"keep":true}\n')
 extra={'APP_BASE_PATH':str(target),'APP_LOCALE':'en'}
 run(['lang:publish'],extra=extra);assert custom.read_text()=='{"custom":"preserve"}\n'
 for locale in locales:
  for group in ['auth','passwords','pagination','validation','welcome','languages','console','schema']:
   p=catalogs/locale/f'{group}.json';assert p.is_file(),p
   if p!=custom:assert p.read_bytes()==(root/f'vendor/openrust/framework/src/localization/lang/{locale}/{group}.json').read_bytes(),p
  assert (catalogs/f'{locale}.json').read_bytes()==(root/f'vendor/openrust/framework/src/localization/lang/{locale}.json').read_bytes()
 run(['lang:publish','--force'],extra=extra);assert custom.read_bytes()==(root/'vendor/openrust/framework/src/localization/lang/ja/validation.json').read_bytes()
 deleted=catalogs/'de/auth.json';deleted.unlink();custom.write_text('{"custom":"replace"}\n')
 run(['lang:publish','--existing','--force'],extra=extra);assert not deleted.exists();assert custom.read_bytes()==(root/'vendor/openrust/framework/src/localization/lang/ja/validation.json').read_bytes();assert outside.read_text()=='{"keep":true}\n'
 assert (catalogs/'NOTICE').read_bytes()==(root/'vendor/openrust/framework/src/localization/stubs/NOTICE').read_bytes()
 print(relative,'publish/force/existing/user preservation/license PASS',flush=True)

# POSIX终端实际触发缺少参数的交互路径，不能只通过帮助页证明生成器已消费词条。
import pty, select, time
for index, locale in enumerate(locales):
 console=json.loads((root/f'vendor/openrust/framework/src/localization/lang/{locale}/console.json').read_text())
 for command, group, answer in [('make:mail','mail','3'),('make:notification','notification','n')]:
  master,slave=pty.openpty()
  process=subprocess.Popen([str(binary),'artisan',command],cwd=app,env=env|{'APP_LOCALE':locale},stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
  os.close(slave);data=b'';deadline=time.monotonic()+30
  try:
   os.write(master,f'Locale{group.title()}{index}\n{answer}\n'.encode())
   while time.monotonic()<deadline:
    if select.select([master],[],[],.1)[0]:
     try:chunk=os.read(master,65536)
     except OSError:break
     if not chunk:break
     data+=chunk
    elif process.poll() is not None:break
   process.wait(timeout=2)
   output=data.decode().replace('\r\n','\n')
   assert process.returncode==0,(locale,command,output)
   assert console['make'][group]['name'] in output,(locale,command,output)
   key='view' if group=='mail' else 'markdown'
   assert console['make'][group][key] in output,(locale,command,output)
   print(locale,command,'actual localized prompt and generation PASS',flush=True)
  finally:
   if process.poll() is None:process.kill();process.wait()
   os.close(master)
