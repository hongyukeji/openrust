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
class ParityWriteItem extends Illuminate\Database\Eloquent\Model {
 protected $table='parity_write_items';public $timestamps=false;protected $fillable=['label','alt'];
}
$connection->statement('create table parity_write_items (id integer primary key, label text unique, alt text unique)');
$events=[];
foreach(['saving','creating','created','saved','updating','updated','deleting','deleted'] as $event) {
 ParityWriteItem::$event(function($model)use(&$events,$event){
  $events[]=$event;
  if(($event==='saving'&&$model->label==='cancel_saving')||($event==='creating'&&$model->label==='cancel_creating')||($event==='deleting'&&$model->label==='cancel_delete')||($event==='updating'&&$model->label==='cancel_updating'))return false;
  if(($event==='saved'&&$model->label==='post_error')||($event==='deleted'&&$model->label==='delete_error'))throw new RuntimeException('listener');
  if($event==='saved'&&$model->label==='mutate_saved')$model->label='listener_label';
 });
}
$cases=[];
$record=function($name,$model,$action)use(&$cases,&$events,$connection){
 $events=[];
 try{$result=['ok'=>$action()];}catch(Throwable $e){$result=['error'=>$e instanceof Illuminate\Database\QueryException?'query':($e instanceof LogicException?'logic':'listener')];}
 $cases[$name]=['result'=>$result,'events'=>$events,'exists'=>$model->exists,'recent'=>$model->wasRecentlyCreated,'dirty'=>$model->isDirty(),'rows'=>$connection->table('parity_write_items')->count()];
};
$item=new ParityWriteItem(['label'=>'first','alt'=>'first_alt']);
$record('save_success',$item,fn()=>$item->saveOrFail());
$record('update_success',$item,fn()=>$item->updateOrFail(['label'=>'updated']));
$record('update_cancel',$item,fn()=>$item->updateOrFail(['label'=>'cancel_updating']));
$record('update_quiet',$item,fn()=>$item->updateQuietly(['label'=>'quiet']));
$record('update_listener_error',$item,fn()=>$item->updateOrFail(['label'=>'post_error']));
$item->updateQuietly(['label'=>'quiet']);
$occupied=new ParityWriteItem(['label'=>'occupied','alt'=>'occupied']);$occupied->saveOrFail();
$record('update_sql_error',$item,fn()=>$item->updateOrFail(['label'=>'occupied']));
$item->updateQuietly(['label'=>'quiet']);
foreach(['cancel_saving','cancel_creating','post_error','mutate_saved'] as $label){$model=new ParityWriteItem(['label'=>$label,'alt'=>$label]);$record($label,$model,fn()=>$model->saveOrFail());}
$duplicate=new ParityWriteItem(['label'=>'quiet','alt'=>'duplicate_alt']);$record('duplicate',$duplicate,fn()=>$duplicate->saveOrFail());
$cancel=new ParityWriteItem(['label'=>'cancel_delete','alt'=>'cancel_delete']);$cancel->save();$record('delete_cancel',$cancel,fn()=>$cancel->deleteOrFail());
$failed=new ParityWriteItem(['label'=>'delete_error','alt'=>'delete_error']);$failed->save();$record('delete_error',$failed,fn()=>$failed->deleteOrFail());
$record('delete_success',$item,fn()=>$item->deleteOrFail());
$empty=new ParityWriteItem;$record('ignore_empty',$empty,fn()=>$empty->saveOrIgnore());
foreach(['cancel_saving','cancel_creating','post_error'] as $label){$model=new ParityWriteItem(['label'=>$label,'alt'=>'ignore_'.$label]);$record('ignore_'.$label,$model,fn()=>$model->saveOrIgnore());}
$ignored=new ParityWriteItem(['label'=>'ignored','alt'=>'ignored']);$record('ignore_success',$ignored,fn()=>$ignored->saveOrIgnore());
$record('ignore_existing',$ignored,fn()=>$ignored->saveOrIgnore());
$conflict=new ParityWriteItem(['label'=>'ignored','alt'=>'different']);$record('ignore_conflict',$conflict,fn()=>$conflict->saveOrIgnore(uniqueBy:['label']));
$other=new ParityWriteItem(['label'=>'new_label','alt'=>'ignored']);$record('ignore_other_constraint',$other,fn()=>$other->saveOrIgnore(uniqueBy:['label']));
echo json_encode($cases,JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR),"\n";
