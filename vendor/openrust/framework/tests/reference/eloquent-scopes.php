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



$connection=new Illuminate\Database\SQLiteConnection(new PDO('sqlite::memory:'),':memory:');
$resolver=new Illuminate\Database\ConnectionResolver(['default'=>$connection]);$resolver->setDefaultConnection('default');
Illuminate\Database\Eloquent\Model::setConnectionResolver($resolver);
class ScopeItem extends Illuminate\Database\Eloquent\Model {
 use Illuminate\Database\Eloquent\SoftDeletes;
 protected $table='parity_scope_items';public $timestamps=false;protected $guarded=[];
 protected static function booted(){static::addGlobalScope('visible',fn($q)=>$q->where('visible',1));static::addGlobalScope('tenant',fn($q)=>$q->where('tenant',1));}
 public function children(){return $this->hasMany(ScopeChild::class,'parent_id');}
}
class ScopeChild extends Illuminate\Database\Eloquent\Model {
 protected $table='parity_scope_children';public $timestamps=false;
 protected static function booted(){static::addGlobalScope('visible',fn($q)=>$q->where('visible',1));}
}
$connection->statement('create table parity_scope_items (id integer primary key, visible integer, tenant integer, deleted_at text)');
$connection->statement('create table parity_scope_children (id integer primary key, parent_id integer, visible integer)');
$connection->statement("insert into parity_scope_items values (1,1,1,null),(2,1,2,null),(3,0,1,null),(4,1,1,'2026-11-23 00:00:00')");
$connection->statement('insert into parity_scope_children values (10,1,1),(11,1,0),(12,2,1)');
$results=[];
$run=function($name,$q)use(&$results){
 $removed=array_map(fn($n)=>$n===Illuminate\Database\Eloquent\SoftDeletingScope::class?'soft':$n,$q->removedScopes());
 $results[$name]=['ids'=>$q->orderBy('id')->pluck('id')->all(),'removed'=>$removed];
};
$run('default',ScopeItem::query());
$run('override',ScopeItem::query()->withGlobalScope('visible',fn($q)=>$q->where('visible',0)));
$run('except_tenant',ScopeItem::query()->withoutGlobalScopesExcept(['tenant']));
$run('except_empty',ScopeItem::query()->withoutGlobalScopesExcept([]));
$run('except_all',ScopeItem::query()->withoutGlobalScopesExcept(['visible','tenant',Illuminate\Database\Eloquent\SoftDeletingScope::class]));
$run('remove_repeat',ScopeItem::query()->withoutGlobalScope('visible')->withoutGlobalScope('visible')->withoutGlobalScope('missing'));
$run('remove_reregister',ScopeItem::query()->withoutGlobalScope('visible')->withGlobalScope('visible',fn($q)=>$q->where('visible',0)));
$run('clear_add',ScopeItem::query()->withoutGlobalScopes()->withGlobalScope('single',fn($q)=>$q->where('id',2)));
$run('or_safe',ScopeItem::query()->where('id',2)->orWhere('id',1));
$run('scope_or_safe',ScopeItem::query()->withGlobalScope('visible',fn($q)=>$q->where('id',1)->orWhere('id',2)));
$run('with_trashed',ScopeItem::query()->withTrashed());
$run('without_trashed',ScopeItem::query()->withoutTrashed());
$run('only_trashed',ScopeItem::query()->onlyTrashed());
$run('only_except_empty',ScopeItem::query()->onlyTrashed()->withoutGlobalScopesExcept([]));
$run('replace_query',ScopeItem::query()->where('id',999)->setQuery($connection->table('parity_scope_items')->where('id',1)));
$results['base_unscoped']=ScopeItem::query()->getQuery()->orderBy('id')->pluck('id')->all();
$results['base_predicates']=ScopeItem::query()->where('id',2)->getQuery()->pluck('id')->all();
$results['applied_once']=ScopeItem::query()->applyScopes()->getQuery()->orderBy('id')->pluck('id')->all();
$results['applied_twice']=ScopeItem::query()->applyScopes()->applyScopes()->getQuery()->orderBy('id')->pluck('id')->all();
$q=ScopeItem::query()->with('children');$results['eager_names']=array_keys($q->getEagerLoads());
$results['eager_children']=$q->firstOrFail()->children->pluck('id')->all();
$q=ScopeItem::query()->with('children')->setEagerLoads([]);$results['eager_reset']=array_keys($q->getEagerLoads());
$results['model_isolation']=ScopeChild::query()->orderBy('id')->pluck('id')->all();
echo json_encode($results,JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR)."\n";
