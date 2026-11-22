<?php
ini_set('display_errors', 'stderr');
require rtrim($argv[2] ?? throw new RuntimeException('请传入固定 PHP 依赖的 vendor 目录'),'/').'/autoload.php';
$root=rtrim($argv[1],'/');
$maps=[];
foreach(glob($root.'/src/Illuminate/*/composer.json') as $file) {
 $component=json_decode(file_get_contents($file),true);
 foreach($component['autoload']['psr-4']??[] as $prefix=>$path) $maps[$prefix][]=dirname($file).'/'.$path;
}
uksort($maps,fn($a,$b)=>strlen($b)-strlen($a));
spl_autoload_register(function($class)use($maps){foreach($maps as $prefix=>$paths)if(str_starts_with($class,$prefix))foreach($paths as $path){
 $file=$path.str_replace('\\','/',substr($class,strlen($prefix))).'.php';
 if(is_file($file)){require $file;return;}
}});
require $root.'/src/Illuminate/Collections/functions.php';
require $root.'/src/Illuminate/Collections/helpers.php';
require $root.'/src/Illuminate/Support/helpers.php';

$cases=[];
$operations=[
 [['fromRaw',['(select ? as id) as items',[7]]]],
 [['selectRaw',['? as marker',[11]]],['fromRaw',['(select ? as id) as items',[7]]],['joinWhere',['other','other.id','=',2]],['where',['items.id','>',3]]],
 [['joinWhere',['other','other.label','=',"x' OR 1=1 --"]]],
 [['leftJoinWhere',['other','other.id','=',null]]],
 [['rightJoinWhere',['other','other.id','=',4]]],
 [['groupBy',['id']],['havingNull',[['id','label']]]],
 [['groupBy',['id']],['havingNotNull',[['id','label']]]],
 [['groupBy',['id']],['havingNotBetween',['id',[2,4]]]],
 [['groupBy',['id']],['having',['id','>',1]],['orHavingNull',['id']]],
 [['groupBy',['id']],['having',['id','>',1]],['orHavingNotNull',['label']]],
 [['groupBy',['id']],['having',['id','>',1]],['orHavingBetween',['id',[2,4]]]],
 [['groupBy',['id']],['having',['id','>',1]],['orHavingNotBetween',['id',[2,4]]]],
 [['groupBy',['id']],['havingNested',[[['having',['id','>',1]],['orHaving',['id','<',0]]]]],['havingNotNull',['label']]],
 [['groupBy',['id']],['having',['id','=',7]],['havingNested',[[],'or']]],
 [['groupBy',['id']],['having',['id','=',7]],['havingNested',[[['having',['id','>',1]],['orHaving',['id','<',0]]],'or']]],
];
foreach(['whereBinary','orWhereBinary','whereNotBinary','orWhereNotBinary'] as $method) $operations[]=[['where',['id','=',1]],[$method,['label',"x' OR 1=1 --"]]];
foreach(['whereIntegerInRaw','orWhereIntegerInRaw','whereIntegerNotInRaw','orWhereIntegerNotInRaw'] as $method) foreach([[],[0,-1,1,9223372036854775807]] as $values) $operations[]=[['where',['id','=',1]],[$method,['id',$values]]];
foreach(['whereRowValues','orWhereRowValues'] as $method) foreach([[1,2],[null,2],[],[1]] as $values) $operations[]=[['where',['id','=',1]],[$method,[['id','group_id'],'>=',$values]]];
foreach(['orWhereAll','orWhereAny','orWhereNone'] as $method) foreach([[],['id','group_id']] as $columns) $operations[]=[['where',['id','=',1]],[$method,[$columns,'=',2]]];
foreach(['forPageBeforeId','forPageAfterId'] as $method) foreach([null,0,7] as $last) $operations[]=[['orderBy',['id','desc']],['orderBy',['label','asc']],[$method,[3,$last,'id']]];
foreach([null,0,-1,42] as $seed)$operations[]=[['inRandomOrder',[$seed]]];
$operations[]=[['orderByRaw',['?',[8]]],['reorderDesc',['id']]];
$apply=function($query,$operations) use (&$apply) {
 foreach($operations as [$method,$arguments]) {
  if($method==='havingNested') {
   $query->havingNested(fn($nested)=>$apply($nested,$arguments[0]),$arguments[1]??'and');
  } else $query->$method(...$arguments);
 }
 return $query;
};
foreach(['sqlite','mysql','pgsql'] as $driver){
 $class=match($driver){'sqlite'=>Illuminate\Database\SQLiteConnection::class,'mysql'=>Illuminate\Database\MySqlConnection::class,'pgsql'=>Illuminate\Database\PostgresConnection::class};
 $connection=new $class(fn()=>throw new RuntimeException('此参考只编译 SQL'));
 foreach($operations as $ops){try{$query=$apply($connection->table('items'),$ops);$cases[]=['driver'=>$driver,'operations'=>$ops,'sql'=>$query->toSql(),'bindings'=>$query->getBindings()];}catch(TypeError|RuntimeException|InvalidArgumentException $error){$cases[]=['driver'=>$driver,'operations'=>$ops,'error'=>$error::class];}}
}
echo json_encode(['framework'=>'1449bf241017087fbd38c118ecaabef3f6797fdc','cases'=>$cases],JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR);
