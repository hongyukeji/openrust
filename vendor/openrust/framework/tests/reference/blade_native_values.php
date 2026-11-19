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

$root=sys_get_temp_dir().'/blade-arithmetic-'.bin2hex(random_bytes(8));
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
 'part'=>'{{ $value }}',
 'components.num'=>"@props(['value' => 0]){{ \$value }}",
 'components.default'=>"@props(['value' => NAN]){{ \$value }}",
 'components.outer'=>"@props(['value'])<x-inner />",
 'components.inner'=>"@aware(['value']){{ \$value }}",
 'layout'=>"{{ \$number }}|@yield('body')",
];
foreach($views as$name=>$source)file_put_contents($root.'/views/'.str_replace('.','/',$name).'.blade.php',$source);
$compiler->directive('nativeKind',fn($expression)=>'<?php echo gettype('.$expression.'); ?>');
$templates=[
 '{{ $number }}',
 '{{ [$number][0] }}',
 '{{ $items[0] }}|{{ $nested["value"] }}',
 '@include("part",["value"=>$number])',
 '<x-num :value="$number" />',
 '<x-default />',
 '<x-outer :value="$number" />',
 '@each("part",$items,"value")',
 '@extends("layout",["number"=>$number])@section("body"){{ $number }}@endsection',
 '@foreach($items as $item){{ $item }}:{{ $loop->index }};@endforeach',
 '@nativeKind($number)|@nativeKind([$number])',
 '@if($number)yes @else no @endif|@isset($number)present @endisset|@empty($number)empty @endempty',
 '{{ $number === INF }}|{{ $number === -INF }}|{{ $number === NAN }}|{{ $number == "INF" }}|{{ $number == true }}',
 '{{ [$number] === [$number] }}|{{ [$number] == [$number] }}',
 '{{ $number + 1 }}|{{ $number * 0 }}',
 '@js($number)',
 '@js([$number])',
 '@js($nested)',
 '{{ $number ?? 7 }}|{{ $missing ?? $number }}|{{ $nested["absent"] ?? $number }}',
 '{{ [$number] == ["INF"] }}|{{ [$number] == ["-INF"] }}|{{ [$number] == ["NAN"] }}',
 '{{ Js::from($items) }}',
];
$results=[];$numbers=['INF'=>INF,'-INF'=>-INF,'NAN'=>NAN,'-0'=>-0.0,'finite'=>2.0];
error_reporting(E_ALL);
set_error_handler(function($level,$message,$file,$line){if(in_array($level,[E_DEPRECATED,E_USER_DEPRECATED],true))return true;if(error_reporting()&$level)throw new ErrorException($message,0,$level,$file,$line);return false;});
foreach($numbers as$kind=>$number)foreach($templates as$template){
 $i=count($results);$name='case_'.$i;file_put_contents($root.'/views/'.$name.'.blade.php',$template);
 $case=['kind'=>$kind,'template'=>$template];
 try{$case['output']=$factory->make($name,['number'=>$number,'items'=>[$number,-0.0],'nested'=>['value'=>$number]])->render();}
 catch(Throwable$e){$case['error']=get_class($e);$case['message']=str_replace($root,'<oracle>',$e->getMessage());}
 $results[]=$case;
}
foreach(['{{ [INF][0] }}',"@include('part',['value'=>NAN])",'<x-num :value="INF" />']as$template){
 $i=count($results);$name='case_'.$i;file_put_contents($root.'/views/'.$name.'.blade.php',$template);
 $case=['kind'=>'INF','template'=>$template];
 try{$case['output']=$factory->make($name)->render();}catch(Throwable$e){$case['error']=get_class($e);$case['message']=str_replace($root,'<oracle>',$e->getMessage());}
 $results[]=$case;
}
restore_error_handler();
if(($results[0]['output']??null)!=='INF')throw new RuntimeException('Basic native oracle failed');
$versions=[];foreach(Composer\InstalledVersions::getInstalledPackages()as$package)if(str_starts_with($package,'illuminate/')){$versions[$package]=Composer\InstalledVersions::getPrettyVersion($package);if($versions[$package]!=='v13.40.0')throw new RuntimeException('Unpinned Illuminate package');}
echo json_encode(['php'=>PHP_VERSION,'illuminate'=>$versions,'views'=>$views,'cases'=>$results],JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR),"\n";
$files->deleteDirectory($root);
