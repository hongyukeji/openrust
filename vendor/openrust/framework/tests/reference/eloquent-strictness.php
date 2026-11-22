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
Illuminate\Database\Eloquent\Model::setEventDispatcher(new Illuminate\Events\Dispatcher(new Illuminate\Container\Container));
enum StrictStatus:string {case ON='on';}
class StrictItem extends Illuminate\Database\Eloquent\Model {
 protected $table='parity_strict_items'; public $timestamps=false; protected $fillable=['name'];
 protected function casts(){return ['count'=>'integer','status'=>StrictStatus::class,'plain'=>'array','active'=>'boolean'];}
 public function getAliasAttribute($value){return $value??'fallback';}
}
class StrictGuarded extends StrictItem {protected $fillable=[];}
class StrictHandled extends StrictItem {}
class StrictBroken extends StrictItem {
 public function setLabelAttribute($value){if(!is_string($value))throw new InvalidArgumentException('label');$this->attributes['label']=strtoupper($value);}
 public function setSecretAttribute($value){if($value==='bad')throw new InvalidArgumentException('setter');$this->attributes['secret']=$value;}
}
class StrictOwner extends Illuminate\Database\Eloquent\Model {
 protected $table='parity_strict_owners';const CREATED_AT=null;protected $guarded=[];
}
class StrictChild extends Illuminate\Database\Eloquent\Model {
 protected $table='parity_strict_children';public $timestamps=false;protected $guarded=[];protected $touches=['owner'];
 public function owner(){return $this->belongsTo(StrictOwner::class,'owner_id');}
}
$connection->statement('create table parity_strict_items (id integer primary key, name text, count integer, nullable text)');
$connection->statement("insert into parity_strict_items values (1,'Ada',7,null)");
$connection->statement('create table parity_strict_owners (id integer primary key, updated_at text)');
$connection->statement("insert into parity_strict_owners values (1,'2000-01-01 00:00:00')");
$connection->statement('create table parity_strict_children (id integer primary key, owner_id integer, name text)');
$connection->statement("insert into parity_strict_children values (1,1,'first')");
$results=[];
foreach([false,true] as $strict)foreach([false,true] as $guarded)foreach([false,true] as $force) {
 $class=$guarded?StrictGuarded::class:StrictItem::class;$class::preventSilentlyDiscardingAttributes($strict);
 $model=new $class;$error=null;
 try { $model->{$force?'forceFill':'fill'}(['name'=>'New','secret'=>'x']); }catch(Illuminate\Database\Eloquent\MassAssignmentException $e){$error='mass';}
 $results['fill_'.(int)$strict.'_'.(int)$guarded.'_'.(int)$force]=['error'=>$error,'attributes'=>(object)$model->getAttributes()];
}
StrictItem::preventSilentlyDiscardingAttributes(true);
StrictItem::handleDiscardedAttributeViolationUsing(fn($model,$keys)=>$model->setAttribute('discarded',implode(', ',$keys)));
$m=new StrictHandled;$m->fill(['name'=>'New','secret'=>'x']);$results['fill_handler']=$m->getAttributes();
StrictItem::handleDiscardedAttributeViolationUsing(null);StrictItem::preventSilentlyDiscardingAttributes(false);
foreach([false,true] as $strict)foreach(['new','existing','recent'] as $state)foreach(['unknown','count','alias','nullable','status'] as $key) {
 StrictItem::preventAccessingMissingAttributes($strict);
 $m=match($state){'new'=>new StrictItem,'existing'=>StrictItem::select('id','nullable')->findOrFail(1),'recent'=>StrictItem::create(['name'=>'recent'])};
 try{$result=['value'=>$m->$key];}catch(Illuminate\Database\Eloquent\MissingAttributeException $e){$result=['error'=>'missing'];}
 $results['read_'.(int)$strict.'_'.$state.'_'.$key]=$result;
}
StrictItem::preventAccessingMissingAttributes(true);
StrictItem::handleMissingAttributeViolationUsing(fn($model,$key)=>'provided');
$m=StrictHandled::select('id','nullable')->findOrFail(1);
foreach(['unknown','count','status'] as $key)$results['read_handler_'.$key]=['value'=>$m->$key];
StrictItem::handleMissingAttributeViolationUsing(null);StrictItem::shouldBeStrict(false);
$events=0;StrictOwner::saved(function()use(&$events){$events++;});
foreach(['normal','ignore_owner','ignore_child','ignore_on'] as $name){
 $connection->statement("update parity_strict_owners set updated_at='2000-01-01 00:00:00'");$events=0;
 $m=StrictChild::findOrFail(1);$m->name=$name;
 $save=fn()=>$m->save();
 match($name){'normal'=>$save(),'ignore_owner'=>StrictOwner::withoutTouching($save),'ignore_child'=>StrictChild::withoutTouching($save),'ignore_on'=>StrictChild::withoutTouchingOn([StrictOwner::class],$save)};
 $results['touch_'.$name]=['changed'=>StrictOwner::findOrFail(1)->updated_at->format('Y-m-d H:i:s')!=='2000-01-01 00:00:00','saved'=>$events];
}
$broken=new StrictBroken;$broken->forceFill(['secret'=>'old']);
try{$broken->forceFill(['label'=>'changed','secret'=>'bad']);}catch(InvalidArgumentException $e){}
$results['fill_setter_failure']=$broken->getAttributes();
$broken=new StrictBroken;$broken->forceFill(['secret'=>'old']);
try{$broken->forceFill(['name'=>'changed','secret'=>'bad']);}catch(InvalidArgumentException $e){}
$results['fill_plain_setter_failure']=$broken->getAttributes();
$broken=new StrictBroken;$broken->forceFill(['plain'=>null,'secret'=>'old']);
try{$broken->forceFill(['plain->a'=>1,'secret'=>'bad']);}catch(InvalidArgumentException $e){}
$results['fill_json_setter_failure']=$broken->getAttributes();
$broken=new StrictBroken;
try{$broken->forceFill(['active'=>'0','label'=>1]);}catch(InvalidArgumentException $e){}
$results['fill_cast_setter_failure']=$broken->getAttributes();
$flags=[];StrictOwner::withoutTouching(function()use(&$flags){$flags[]=StrictOwner::isIgnoringTouch();StrictOwner::withoutTouching(function()use(&$flags){$flags[]=StrictOwner::isIgnoringTouch();});$flags[]=StrictOwner::isIgnoringTouch();});$flags[]=StrictOwner::isIgnoringTouch();$results['upstream_nested_same']=$flags;
echo json_encode($results,JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR)."\n";
