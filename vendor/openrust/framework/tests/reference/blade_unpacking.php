<?php
// Run with the pinned Illuminate View 13.40.0 Composer fixture, PHP 8.4.
require getenv('BLADE_ORACLE_VENDOR') ?: '/app/vendor/autoload.php';
use Illuminate\Container\Container;
use Illuminate\Events\Dispatcher;
use Illuminate\Filesystem\Filesystem;
use Illuminate\View\Compilers\BladeCompiler;
use Illuminate\View\Engines\CompilerEngine;
use Illuminate\View\Engines\EngineResolver;
use Illuminate\View\Factory;
use Illuminate\View\FileViewFinder;

class_alias(Illuminate\Support\Js::class, 'Js');

$root=sys_get_temp_dir().'/blade-unpack-'.bin2hex(random_bytes(8));
mkdir($root.'/views/components',0777,true);mkdir($root.'/cache');
$files=new Filesystem;
$container=new Container;Container::setInstance($container);
$compiler=new BladeCompiler($files,$root.'/cache');
$resolver=new EngineResolver;$resolver->register('blade',fn()=>new CompilerEngine($compiler,$files));
$factory=new Factory($resolver,new FileViewFinder($files,[$root.'/views']),new Dispatcher($container));
$factory->setContainer($container);
$container->instance('view',$factory);
$container->instance(Illuminate\Contracts\View\Factory::class,$factory);
$container->instance(Illuminate\View\Compilers\BladeCompiler::class,$compiler);
$container->instance('blade.compiler',$compiler);
$container->instance(Illuminate\Contracts\Foundation\Application::class,new class {public function getNamespace(){return 'App\\';}});
$views=[
 'part'=>'{{ $title }}',
 'components.spread'=>"@props(['items'])@foreach(\$items as \$key => \$value){{ \$key }}:{{ \$value }};@endforeach",
];
foreach($views as$name=>$source)file_put_contents($root.'/views/'.str_replace('.','/',$name).'.blade.php',$source);
$templates=[
 '@foreach([...$items] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@foreach([99,...$items,100] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@foreach([-5=>99,...$items] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@foreach(["name"=>1,...$items,"name"=>2] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@foreach(array(...$items,...$items) as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@js([...$items])',
 '<x-spread :items="[...$items]" />',
 '{{ [...$items][0] ?? "absent" }}',
 '{{ ["nested"=>[...$items]]["nested"][1] ?? "absent" }}',
];
$inputs=[[],[10,20],[-7=>10,9=>20],['name'=>10,'08'=>20,'1'=>30],['title'=>'<A>','name'=>0],null,7,'text',false];
$cases=[];
foreach($inputs as$items)foreach($templates as$template)$cases[]=['data'=>['items'=>$items],'template'=>$template];
foreach([
 '@foreach([...[],10,...[8=>20,-5=>30],40] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@foreach(["a"=>1,...["a"=>2,"b"=>3],"a"=>4] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@foreach([...[-5=>1],...[-8=>2],...[]] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@foreach([-5=>1,...[100=>2],3] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '@foreach(["08"=>1,...["8"=>2,"-0"=>3,"+1"=>4]] as $key => $value){{ $key }}:{{ $value }};@endforeach',
 '{{ [...[INF,NAN,-0.0]][0] }}|{{ [...[INF,NAN,-0.0]][1] }}|{{ [...[INF,NAN,-0.0]][2] }}',
 '@js([...[["a"=>1]],[...[[2,3]]]])',
 '@include("part",[...["title"=>"<A>"]])',
 '<b @class([...["active"=>true,"hidden"=>false],"wide"])>x</b>',
 '{{ true || [...$invalid] }}|{{ false && [...$invalid] }}|{{ 7 ?? [...$invalid] }}',
 '{{ [9223372036854775807=>1,...[]][9223372036854775807] }}',
 '{{ [9223372036854775807=>1,...[2]][0] }}',

]as$template)$cases[]=['data'=>new stdClass,'template'=>$template];
foreach([null,true,'abc']as$invalid)$cases[]=['data'=>['invalid'=>$invalid],'template'=>'{{ [...$invalid] }}'];
$results=[];
foreach($cases as$i=>$case){
 $name='case_'.$i;file_put_contents($root.'/views/'.$name.'.blade.php',$case['template']);
 try{$case['output']=$factory->make($name,(array)$case['data'])->render();}
 catch(Throwable$e){$case['error']=get_class($e);$case['message']=str_replace($root,'<oracle>',$e->getMessage());}
 $results[]=$case;
}
$versions=[];foreach(Composer\InstalledVersions::getInstalledPackages()as$package)if(str_starts_with($package,'illuminate/')){$versions[$package]=Composer\InstalledVersions::getPrettyVersion($package);if($versions[$package]!=='v13.40.0')throw new RuntimeException('Unpinned Illuminate package');}
echo json_encode(['php'=>PHP_VERSION,'illuminate'=>$versions,'views'=>$views,'cases'=>$results],JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR),"\n";
$files->deleteDirectory($root);
