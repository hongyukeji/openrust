import assert from 'node:assert/strict';
import {readFileSync,existsSync,readdirSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=fileURLToPath(new URL('../',import.meta.url));
const languages=['en','zh_CN','zh_TW'];
let canonical;
for(const language of languages){
 const base=path.join(root,language);
 const navigation=readFileSync(path.join(base,'README.md'),'utf8');
 const files=[...navigation.matchAll(/^(?:- )?\[[^\]]+\]\(([^)]+\.md)\)$/gm)].map(m=>m[1]);
 assert(files.length>100,`${language}: incomplete navigation`);
 if(canonical)assert.deepEqual(files,canonical,`${language}: navigation order differs`);else canonical=files;
 for(const file of new Set(files)){
  const target=path.resolve(base,file);assert(existsSync(target),`${language}/${file}: missing`);
  const text=readFileSync(target,'utf8');
  assert(/^# .+/m.test(text),`${language}/${file}: missing title`);
  assert(text.trim().split('\n').length>3,`${language}/${file}: title-only chapter`);
  for(const match of text.replace(/```[\s\S]*?```/g,'').matchAll(/\]\(([^)\s]+\.md)(?:#[^)\s]*)?\)/g)){
   if(/^[a-z][a-z0-9+.-]*:/i.test(match[1])||match[1].startsWith('/'))continue;
   assert(existsSync(path.resolve(path.dirname(target),match[1])),`${language}/${file}: missing reference ${match[1]}`);
  }
 }
 const filesOnDisk=[];
 function walk(directory){for(const entry of readdirSync(directory,{withFileTypes:true})){const target=path.join(directory,entry.name);if(entry.isDirectory())walk(target);else if(entry.name.endsWith('.md'))filesOnDisk.push(target);}}
 walk(base);for(const target of filesOnDisk)assert(readFileSync(target,'utf8').trim(),`Empty Markdown: ${target}`);
 console.log(`${language}: ${new Set(files).size} navigation chapters and ${filesOnDisk.length} Markdown files have content PASS`);
}
