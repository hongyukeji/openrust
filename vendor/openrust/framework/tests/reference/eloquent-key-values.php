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


$connection = new Illuminate\Database\SQLiteConnection(new PDO('sqlite::memory:'), ':memory:');
$resolver = new Illuminate\Database\ConnectionResolver(['default' => $connection]);
$resolver->setDefaultConnection('default');
Illuminate\Database\Eloquent\Model::setConnectionResolver($resolver);
class ParityItem extends Illuminate\Database\Eloquent\Model {
 protected $table = 'parity_model_keys'; public $timestamps = false;
 protected static function booted() { static::addGlobalScope('visible', fn($q) => $q->where('visible',1)); }
 public function getLabelAttribute($value) { return $value === null ? null : strtoupper($value); }
}
class ParityTextItem extends Illuminate\Database\Eloquent\Model {
 protected $table = 'parity_model_text_keys'; public $timestamps = false; protected $keyType = 'string'; public $incrementing = false;
}
$connection->statement('create table parity_model_keys (id integer primary key, label text, visible integer, nullable_value text)');
foreach([[1,'ada',1],[2,'grace',1],[3,'hidden',0]] as $row) $connection->insert('insert into parity_model_keys(id,label,visible) values (?,?,?)',$row);
$connection->statement('create table parity_model_text_keys (id text primary key)');
foreach(['','1','7','007',"x' OR 1=1 --"] as $key) $connection->insert('insert into parity_model_text_keys(id) values (?)',[$key]);
$cases=[];
foreach(['integer'=>ParityItem::class,'string'=>ParityTextItem::class] as $kind=>$class) {
 foreach([null,false,true,0,1,2,3,7,'','1','007','2suffix',"x' OR 1=1 --",[],[1,2,3],['007',"x' OR 1=1 --"],['2suffix',null]] as $input) {
  foreach(['whereKey','whereKeyNot','orWhereKey','orWhereKeyNot'] as $method) {
   $q=$class::query(); if(str_starts_with($method,'or')) $q->where('id','=',$kind==='integer'?1:'1');
   $q->$method($input);
   $cases[]=['kind'=>$kind,'method'=>$method,'input'=>$input,'ids'=>$q->orderBy('id')->get()->modelKeys()];
  }
 }
}
$values=[];
foreach(['value','soleValue','valueOrFail'] as $method) {
 foreach([null,1,2,3,99] as $id) foreach(['label','parity_model_keys.label','nullable_value'] as $column) {
  $q=ParityItem::query(); if($id!==null) $q->whereKey($id); $q->orderBy('id');
  try { $result=['ok'=>$q->$method($column)]; }
  catch(Illuminate\Database\Eloquent\ModelNotFoundException $e) { $result=['error'=>'ModelNotFound']; }
  catch(Illuminate\Database\MultipleRecordsFoundException $e) { $result=['error'=>'MultipleModelsFound','count'=>$e->count]; }
  $values[]=['method'=>$method,'id'=>$id,'column'=>$column,'result'=>$result];
 }
}
echo json_encode(['keys'=>$cases,'values'=>$values], JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR), "\n";
