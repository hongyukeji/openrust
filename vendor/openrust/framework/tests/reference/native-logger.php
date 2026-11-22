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

$handler=new Monolog\Handler\TestHandler(Monolog\Level::Info);
$native=new Monolog\Logger('native',[$handler]);
$native->pushProcessor(fn(Monolog\LogRecord $record)=>$record->with(extra:array_merge($record->extra,['processed'=>true])));
$dispatcher=new Illuminate\Events\Dispatcher(new Illuminate\Container\Container);
$events=[];$dispatcher->listen(Illuminate\Log\Events\MessageLogged::class,function($event)use(&$events){$events[]=['level'=>$event->level,'message'=>$event->message,'context'=>(object)$event->context];});
$logger=new Illuminate\Log\Logger($native,$dispatcher);
$logger->withContext(['shared'=>'application']);
$logger->info('wrapped',['call'=>'one']);
$logger->getLogger()->info('native',['call'=>'two']);
$logger->debug('filtered',['call'=>'three']);
$logger->getLogger()->debug('native filtered',['call'=>'four']);
$records=array_map(fn($r)=>['level'=>strtolower($r->level->name),'message'=>$r->message,'context'=>(object)$r->context,'extra'=>(object)$r->extra,'channel'=>$r->channel],$handler->getRecords());
echo json_encode(['same'=>$logger->getLogger()===$native,'records'=>$records,'events'=>$events],JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR)."\n";
