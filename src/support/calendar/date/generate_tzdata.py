#!/usr/bin/env python3
"""从固定官方 PHP 8.5.0 timezonedb.h 提取 POSIX 尾规则/历史起点。

不运行PHP、不读取系统TZ，不使用Cargo/build.rs；仅标准库。输入SHA固定，
升级基线时显式更新SHA并重新差分。许可证见同目录LICENSE.timelib及SOURCES.md。
用法：python3 generate_tzdata.py /path/to/php-8.5.0/ext/date/lib/timezonedb.h /path/to/tail_rules.rs
"""
import pathlib,re,struct,hashlib,sys
EXPECTED_SHA256='01d9a2097154f96ae320204d1dad3157652216bd4a81cda2e2ad9100f076dbc1'
def generate(source,output):
 raw=source.read_bytes();digest=hashlib.sha256(raw).hexdigest()
 if digest!=EXPECTED_SHA256:raise SystemExit('固定官方数据SHA256不匹配: '+digest)
 text=raw.decode();head=text[:text.index('const unsigned char timelib_timezone_db_data_builtin')]
 block=text[text.index('const unsigned char timelib_timezone_db_data_builtin'):];block=block[:block.index('\n};')]
 blob=bytes(int(n,16) for n in re.findall(r'0x([0-9A-Fa-f]{2})',block))
 index=re.findall(r'\{ \(char\*\) "([^"]+)"\s*, 0x([0-9A-Fa-f]+) \}',head)
 if len(index)!=598:raise SystemExit('固定zone数量不匹配')
 rows=[]
 for name,offset in index:
  pos=int(offset,16)
  if blob[pos:pos+3]!=b'PHP':raise SystemExit('无效PHP TZif header')
  gmt,std,leap,times,types,chars=struct.unpack_from('>6I',blob,pos+20)
  pos+=44+times*5+types*6+chars+leap*8+std+gmt
  if blob[pos:pos+4]!=b'TZif':raise SystemExit('无效64位TZif header')
  gmt,std,leap,times,types,chars=struct.unpack_from('>6I',blob,pos+20);pos+=44
  transitions=list(struct.unpack_from('>'+str(times)+'q',blob,pos)) if times else []
  type_start=pos+times*9;first_offset,first_dst,abbr_index=struct.unpack_from('>iBB',blob,type_start)
  abbr_start=type_start+types*6;end=blob.index(0,abbr_start+abbr_index)
  first_abbr=blob[abbr_start+abbr_index:end].decode('ascii')
  pos+=times*9+types*6+chars+leap*12+std+gmt
  if blob[pos]!=10:raise SystemExit('无效POSIX footer起点')
  end=blob.index(10,pos+1);footer=blob[pos+1:end].decode('ascii')
  if not footer:raise SystemExit('无POSIX footer')
  rows.append((name,transitions[-1] if times else -9223372036854775808,transitions[0] if times else None,first_offset,first_abbr,first_dst,footer))
 rules=sorted(set(row[-1] for row in rows))
 out=['//! 由 `generate_tzdata.py` 从固定 PHP 8.5.0 / IANA 2025b 数据生成。',
      '//! 完整来源、SHA256 和许可见 SOURCES.md / LICENSE.timelib。','',
      'use super::posix::TailRecord;','',
      'pub(super) const RULES: &[&str] = &[']
 for rule in rules:out.append('    '+repr(rule).replace("'",'"')+',')
 out.extend(['];','','pub(super) const ZONES: &[TailRecord] = &['])
 for name,begin,first,offset,abbr,dst,rule in rows:
  first='None' if first is None else f'Some({first:_})'
  out.append('    TailRecord { name: "'+name+'", from_epoch: '+f'{begin:_}'+', first_epoch: '+first+', first_offset: '+f'{offset:_}'+', first_abbreviation: "'+abbr+'", first_is_dst: '+str(bool(dst)).lower()+', rule: RULES['+str(rules.index(rule))+'] },')
 out.extend(['];','']);output.write_text('\n'.join(out))
 print(f'{len(rows)} zones, {len(rules)} rules; source SHA256 {digest}')
def abbreviations(source,output):
 raw=source.read_bytes()
 if hashlib.sha256(raw).hexdigest()!='b841e465ee76bd428c4297a82e320c982392d61ca1185a4f2a3a391eee537b7e':raise SystemExit('固定缩写数据SHA256不匹配')
 rows=re.findall(r'\{\s*"([^"]+)",\s*(\d+),\s*(-?\d+),',raw.decode())
 first={}
 for name,dst,offset in rows:first.setdefault(name,(int(offset),bool(int(dst))))
 out=['//! 固定 PHP 8.5.0 / tzdb 2025.2 时区缩写；来源与许可见 SOURCES.md。','',
      'pub(super) const ABBREVIATIONS: &[(&str, i32)] = &[']
 for name,(offset,dst) in first.items():out.append(f'    ("{name}", {offset:_}),')
 out.extend(['];','','pub(super) const DAYLIGHT_ABBREVIATIONS: &[&str] = &['])
 for name,(offset,dst) in first.items():
  if dst:out.append(f'    "{name}",')
 out.extend(['];','']);output.write_text('\n'.join(out))
 print(f'{len(first)} abbreviations')
if __name__=='__main__':
 generate(pathlib.Path(sys.argv[1]),pathlib.Path(sys.argv[2]))
 if len(sys.argv)==5:abbreviations(pathlib.Path(sys.argv[3]),pathlib.Path(sys.argv[4]))
