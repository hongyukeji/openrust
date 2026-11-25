"""通过真实PTY和Artisan验收分区终端编辑器，对照固定源元数据。"""
import argparse,json,os,subprocess,sys,tempfile,time,select,re
from pathlib import Path
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--source',type=Path,default=root);p.add_argument('--report',type=Path,required=True);args=p.parse_args();source=args.source.resolve();binary=args.binary.resolve()
if sys.platform=='win32':p.error('此验收需要POSIX PTY；Windows实机验收单列。')
import pty,fcntl,termios,struct
if not binary.is_relative_to(root/'target/tmp') or not args.report.resolve().is_relative_to(root/'target/tmp'):p.error('验收二进制和报告必须位于项目target/tmp内。')
space=root/'target/tmp/pv';space.mkdir(parents=True,exist_ok=True)
cases=json.loads((source/'.agents/project/audits/artisan-tutorial/section-editor-oracle.json').read_text());results=[];failures=[]
def session():os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
for index,case in enumerate(cases):
 with tempfile.TemporaryDirectory(prefix='section-editor-',dir=space) as app_root:
  master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',24,80,0,0));env=dict(os.environ,SECTION_EDITOR_ROOT=app_root,SECTION_EDITOR_CASE=json.dumps(case,ensure_ascii=False),COLUMNS='80',TERM='xterm-256color',APP_LOCALE='en',TMPDIR=str(space));child=None;output=b''
  try:
   child=subprocess.Popen([str(binary),'--exact','section_editor_cli_fixture','--ignored','--nocapture'],stdin=slave,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,preexec_fn=session);os.close(slave);slave=None;deadline=time.monotonic()+15
   while b' > ' not in output:
    if time.monotonic()>deadline:raise AssertionError(('问题提示超时',index,output))
    ready,_,_=select.select([child.stdout],[],[],.1)
    if ready:
     data=os.read(child.stdout.fileno(),65536)
     if not data:raise AssertionError(('命令提前退出',index,output,child.stderr.read()))
     output+=data
   time.sleep(.1);os.write(master,case['keys'].encode());out,err=child.communicate(timeout=15);output+=out;assert child.returncode==0,(index,output,err)
   text=err.decode();marker='SECTION_EDITOR_META:';start=text.index(marker);meta=json.loads(text[start+len(marker):].splitlines()[0]);stdout=output.decode().removeprefix('\nrunning 1 test\n');same_meta=meta==case['metadata'];same_bytes=stdout==case['output'];passed=same_meta and (same_bytes or case['kind']=='multi');results.append(dict(index=index,kind=case['kind'],passed=passed,exact_stdout=same_bytes,metadata=meta))
   if not passed:failures.append(dict(index=index,expected=case['metadata'],actual=meta))
  except Exception as error:failures.append(dict(index=index,error=repr(error)))
  finally:
   if child is not None and child.poll() is None:child.terminate();child.wait(timeout=5)
   if slave is not None:os.close(slave)
   os.close(master)
# 原生取消还须恢复真实终端状态；这不是额外的固定源向量。
cleanup=None
with tempfile.TemporaryDirectory(prefix='section-editor-cancel-',dir=space) as app_root:
 master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',24,80,0,0));original=termios.tcgetattr(slave);child=None;output=b''
 case=dict(kind='complete',default='Guest',values=['Ada','Adam'],decorated=True)
 try:
  release=Path(app_root)/'release-probe';env=dict(os.environ,SECTION_EDITOR_ROOT=app_root,SECTION_EDITOR_CASE=json.dumps(case),SECTION_EDITOR_PROBE_RELEASE=str(release),COLUMNS='80',TERM='xterm-256color',APP_LOCALE='en',TMPDIR=str(space))
  child=subprocess.Popen([str(binary),'--exact','section_editor_cli_fixture','--ignored','--nocapture'],stdin=slave,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,preexec_fn=session);deadline=time.monotonic()+15
  while b' > ' not in output:
   if time.monotonic()>deadline:raise AssertionError('取消用例提示超时')
   ready,_,_=select.select([child.stdout],[],[],.1)
   if ready:
    data=os.read(child.stdout.fileno(),65536)
    if not data:raise AssertionError(('取消用例提前退出',output,child.stderr.read()))
    output+=data
  time.sleep(.1);assert not termios.tcgetattr(slave)[3]&termios.ICANON
  os.write(master,b'\x03');errors=b'';deadline=time.monotonic()+15
  while b'SECTION_EDITOR_STATUS:' not in errors:
   if time.monotonic()>deadline:raise AssertionError('未收到中断处理完成状态')
   ready,_,_=select.select([child.stderr],[],[],.1)
   if ready:
    data=os.read(child.stderr.fileno(),65536)
    if not data:raise AssertionError(('状态探测提前结束',errors))
    errors+=data
  assert b'SECTION_EDITOR_STATUS:130' in errors,errors
  assert termios.tcgetattr(slave)==original,'中断后未恢复终端属性'
  release.touch();out,err=child.communicate(timeout=15);assert child.returncode==130,(out,err,child.returncode)
  cleanup=dict(passed=True,exitcode=child.returncode,restored_terminal=True)
 except Exception as error:failures.append(dict(kind='native_cancel_cleanup',error=repr(error)))
 finally:
  if child is not None and child.poll() is None:child.terminate();child.wait(timeout=5)
  os.close(slave);os.close(master)
args.report.write_text(json.dumps(dict(cases=len(cases),results=results,native_cleanup=cleanup,failures=failures),ensure_ascii=False,indent=2)+'\n')
print('终端编辑器：',len(cases),'项，失败',len(failures),'项')
if failures:print(json.dumps(failures[:3],ensure_ascii=False,indent=2));raise SystemExit(1)
