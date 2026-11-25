"""通过真实终端运行树输出命令，核对所选输出、物理字节和终端状态。"""
import argparse,errno,json,os,pty,select,struct,subprocess,termios,time,fcntl
from pathlib import Path
root=Path(__file__).resolve().parents[1]
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary',type=Path,required=True)
parser.add_argument('--report',type=Path,required=True)
args=parser.parse_args()
report=args.report.resolve()
if not report.is_relative_to(root/'target/tmp'):parser.error('报告须位于项目target/tmp内')
report.parent.mkdir(parents=True,exist_ok=True)
cases=json.loads((root/'.agents/project/audits/artisan-tutorial/tree-pty-oracle.json').read_text())
results=[]
for case in cases:
 master,slave=pty.openpty();fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',24,80,0,0));original=termios.tcgetattr(slave)
 env=dict(os.environ,TREE_CASE=json.dumps(case),COLUMNS='80',LINES='24',TERM='xterm-256color',COLORTERM='',TMPDIR=str(root/'target/tmp/pv'));env.pop('NO_COLOR',None)
 child=None
 try:
  child=subprocess.Popen([str(args.binary.resolve()),'--exact','tree_console_cli_driver','--ignored','--nocapture'],stdin=slave,stdout=slave,stderr=slave,env=env,close_fds=True)
  data=bytearray();deadline=time.monotonic()+12
  while True:
   if time.monotonic()>=deadline:raise AssertionError('树输出子进程超时')
   ready,_,_=select.select([master],[],[],.05)
   if ready:
    try:chunk=os.read(master,65536)
    except OSError as error:
     if error.errno!=errno.EIO:raise
     break
    if chunk:data.extend(chunk)
   if child.poll()is not None and not ready:break
  code=child.wait(timeout=3);assert code==0,(case,code,bytes(data));text=data.decode();payload=text.split('TREE_BEGIN',1)[1];payload,meta=payload.rsplit('TREE_META',1);assert payload==case['payload'],(case,payload);assert json.loads(meta)==case['meta'],(case,meta);restored=termios.tcgetattr(slave)==original;assert restored,'终端状态未恢复';results.append(dict(mode=case['mode'],shape=case['shape'],error=case['error'],exitcode=code,termios_restored=restored,passed=True))
 finally:
  if child is not None and child.poll()is None:child.kill();child.wait(timeout=3)
  os.close(master);os.close(slave)
report.write_text(json.dumps(dict(passed=True,cases=results),ensure_ascii=False,indent=2)+'\n')
print('tree PTY:',len(results),'source/native cases passed; termios preserved')
