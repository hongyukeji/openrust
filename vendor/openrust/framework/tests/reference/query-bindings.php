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
$kinds=['select','from','join','where','groupBy','having','order','union','unionOrder'];
foreach(['sqlite','pgsql','mysql'] as $driver) {
 $connection=new Illuminate\Database\Connection(fn()=>throw new RuntimeException('仅编译'),'', '', ['driver'=>$driver]);
 $grammar=match($driver){'sqlite'=>new Illuminate\Database\Query\Grammars\SQLiteGrammar($connection),'pgsql'=>new Illuminate\Database\Query\Grammars\PostgresGrammar($connection),'mysql'=>new Illuminate\Database\Query\Grammars\MySqlGrammar($connection)};
 $connection->setQueryGrammar($grammar);
 $new=fn()=>$connection->table('binding_items');
 $record=function($name,$query)use(&$cases,$driver){$cases[$driver.'_'.$name]=['sql'=>$query->toSql(),'raw'=>$query->getRawBindings(),'flat'=>$query->getBindings()];};
 $record('empty',$new());
 foreach($kinds as $kind) {
  $record('set_'.$kind,$new()->setBindings([11,12],$kind));
  $record('add_'.$kind,$new()->addBinding(11,$kind)->addBinding([12,13],$kind));
 }
 $record('nested_having',$new()->groupBy('id')->having('id','>',1)->havingNested(fn($q)=>$q->having('id','<',2)->setBindings([9],'having')));
 $record('append',$new()->where('id','>',1)->setBindings([2])->where('id','<',9));
 $record('add_append',$new()->whereRaw('id > ?',[])->addBinding(1)->where('id','<',9));
 $record('clear_select',$new()->selectRaw('? as marker',[1])->setBindings([2],'select')->select(['id'])->selectRaw('? as marker',[3]));
 $record('clear_order',$new()->orderByRaw('?',[1])->setBindings([2],'order')->reorder()->orderByRaw('?',[3]));
 $record('source_replace',$new()->fromRaw('(select ? as id) as one',[1])->fromRaw('(select ? as id) as two',[2])->from('binding_items'));
 $other=$new()->where('id','<',9)->havingRaw('? = 1',[4]);
 $record('merge',$new()->whereRaw('id > ? and id < ?',[1])->mergeBindings($other)->orderByRaw('?',[5]));
 $query=$new()->selectRaw('? as marker',[1])->fromRaw('(select ? as id) as source',[2])->joinWhere('other','other.id','=',3)->where('source.id','>',4)->groupByRaw('?',[5])->havingRaw('? = 1',[6])->orderByRaw('?',[7]);
 $part=$new()->selectRaw('? as marker',[8])->where('id','<',9);
 $query->unionAll($part)->orderByRaw('?',[10]);
 $record('all',$query);
 foreach($kinds as $i=>$kind)$query->setBindings([$i+101],$kind);
 $query->where('source.id','<',200)->selectRaw('? as extra',[201])->orderByRaw('?',[202]);
 $record('all_changed',$query);
 $copy=clone $query;$copy->addBinding(303,'where');$record('copy',$copy);$record('original',$query);
 try{$new()->setBindings([1],'invalid');$cases[$driver.'_invalid']=false;}catch(InvalidArgumentException $e){$cases[$driver.'_invalid']=true;}
}
echo json_encode($cases,JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR)."\n";
