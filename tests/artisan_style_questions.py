"""用真实终端验收样式问答便捷入口、隐藏输入与取消恢复。"""
import argparse,json,os,select,subprocess,sys,tempfile,time
if sys.platform=="win32":raise SystemExit("此验收需要POSIX终端；Windows实机验收单列")
import fcntl,pty,struct,termios
from pathlib import Path
root=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--report',type=Path,required=True);args=p.parse_args();binary=args.binary.resolve()
if not binary.is_relative_to(root/'target/tmp')or not args.report.resolve().is_relative_to(root/'target/tmp'):p.error('二进制与报告必须位于项目target/tmp中')
space=root/'target/tmp/pv';space.mkdir(parents=True,exist_ok=True);results=[]
def session():os.setsid();fcntl.ioctl(0,termios.TIOCSCTTY,0)
for cancel in [False,True]:
 with tempfile.TemporaryDirectory(prefix='style-question-tty-',dir=space)as app_root:
  master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',24,80,0,0));original=termios.tcgetattr(slave);release=Path(app_root)/'release';output=b'';errors=b'';echo=b'';child=None
  try:
   env=dict(os.environ,STYLE_QUESTIONS_ROOT=app_root,STYLE_QUESTIONS_LIVE='1',STYLE_QUESTIONS_RELEASE=str(release),COLUMNS='80',TERM='xterm-256color',APP_LOCALE='en',TMPDIR=str(space))
   child=subprocess.Popen([str(binary),'--exact','style_questions_cli_driver','--ignored','--nocapture'],stdin=slave,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,preexec_fn=session)
   def wait_for(predicate):
    global output,errors,echo
    deadline=time.monotonic()+15
    while not predicate():
     if time.monotonic()>deadline:raise AssertionError(('终端等待超时',output,errors,echo))
     ready,_,_=select.select([child.stdout,child.stderr,master],[],[],.05)
     for stream in ready:
      fd=stream if isinstance(stream,int)else stream.fileno();data=os.read(fd,65536)
      if stream is child.stdout:output+=data
      elif stream is child.stderr:errors+=data
      else:echo+=data
     if child.poll()is not None and not predicate():raise AssertionError(('命令提前退出',child.returncode,output,errors))
   wait_for(lambda:b'Name'in output and output.endswith(b' > '));time.sleep(.1)
   if not cancel:
    os.write(master,b'bad\r');wait_for(lambda:output.count(b'Name')>=2 and output.endswith(b' > '));assert b'Try again'in output
   os.write(master,b'Ada\r');wait_for(lambda:b'Password'in output and output.endswith(b' > '));time.sleep(.1)
   assert not termios.tcgetattr(slave)[3]&termios.ECHO,'隐藏输入未关闭终端回显'
   if cancel:os.write(master,b'\x03')
   else:
    os.write(master,b'sample-secret\r');wait_for(lambda:b'Continue'in output and output.endswith(b' > '));os.write(master,b'\r');wait_for(lambda:b'Color'in output and output.endswith(b' > '));os.write(master,b'\r')
   expected=130 if cancel else 0;marker=f'STYLE_QUESTIONS_STATUS:{expected}'.encode();wait_for(lambda:marker in errors)
   assert termios.tcgetattr(slave)==original,'命令完成后未恢复完整终端属性'
   assert b'sample-secret'not in output+errors+echo,'隐藏输入发生回显'
   if not cancel:
    line=next(line for line in errors.decode().splitlines()if line.startswith('STYLE_QUESTIONS_META:'));meta=json.loads(line.split(':',1)[1]);assert meta==dict(name=dict(name='ADA'),secret=dict(length=13),confirm=True,choice='green'),meta
   release.touch();out,err=child.communicate(timeout=15);output+=out;errors+=err;assert child.returncode==expected
   results.append(dict(cancel=cancel,passed=True,exitcode=child.returncode,hidden_echo_disabled=True,terminal_restored=True,password_not_echoed=True))
  except Exception as error:results.append(dict(cancel=cancel,passed=False,error=repr(error)))
  finally:
   if child is not None and child.poll()is None:
    release.touch();child.terminate()
    try:child.wait(timeout=5)
    except subprocess.TimeoutExpired:child.kill();child.wait(timeout=5)
   os.close(slave);os.close(master)
args.report.write_text(json.dumps(dict(cases=results,passed=all(case['passed']for case in results)),ensure_ascii=False,indent=2)+'\n');print(json.dumps(results,ensure_ascii=False));raise SystemExit(0 if all(case['passed']for case in results)else 1)
