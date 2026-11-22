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
$resolver = new Illuminate\Database\ConnectionResolver(['default'=>$connection]);$resolver->setDefaultConnection('default');
Illuminate\Database\Eloquent\Model::setConnectionResolver($resolver);
Illuminate\Database\Eloquent\Relations\Relation::morphMap(['post'=>ParityPost::class,'other'=>ParityOther::class]);
class ParityActivity extends Illuminate\Database\Eloquent\Model {
 protected $table='parity_morph_activities';public $timestamps=false;
 public function subject(){return $this->morphTo();}
}
class ParityPost extends Illuminate\Database\Eloquent\Model {
 protected $table='parity_morph_posts';public $timestamps=false;
 public function ratings(){return $this->hasMany(ParityRating::class,'post_id');}
}
class ParityOther extends Illuminate\Database\Eloquent\Model {protected $table='parity_morph_others';public $timestamps=false;}
class ParityRating extends Illuminate\Database\Eloquent\Model {
 protected $table='parity_morph_ratings';public $timestamps=false;
 protected static function booted(){static::addGlobalScope('positive',fn($q)=>$q->where('score','>=',0));}
}
foreach([
 'create table parity_morph_posts (id integer primary key)',
 'create table parity_morph_others (id integer primary key)',
 'create table parity_morph_ratings (id integer primary key, post_id integer, score integer)',
 'create table parity_morph_activities (id integer primary key, subject_type text, subject_id integer)',
 'insert into parity_morph_posts (id) values (1),(2)',
 'insert into parity_morph_others (id) values (1)',
 'insert into parity_morph_ratings (id,post_id,score) values (1,1,10),(2,1,20),(3,1,-100)',
 "insert into parity_morph_activities (id,subject_type,subject_id) values (1,'post',1),(2,'post',2),(3,'other',1),(4,null,null)",
] as $sql)$connection->statement($sql);
$cases=[];
foreach(['max','min','sum','avg'] as $aggregate)foreach([1,2,3,4] as $id){
 $activity=ParityActivity::with('subject')->findOrFail($id);
 $method='loadMorph'.ucfirst($aggregate);$activity->$method('subject',[ParityPost::class=>['ratings']],'score');
 $target=$activity->getRelation('subject');$attribute='ratings_'.$aggregate.'_score';$attributes=$target?->getAttributes()??[];
 $cases[]=['method'=>$aggregate,'id'=>$id,'attribute'=>$attribute,'present'=>array_key_exists($attribute,$attributes),'value'=>$attributes[$attribute]??null];
}
echo json_encode($cases,JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR),"\n";
