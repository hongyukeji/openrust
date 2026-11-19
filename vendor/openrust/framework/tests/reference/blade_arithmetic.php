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
 'components.bad-prop'=>"@props(['value' => 1 / 0]){{ \$value }}",
 'components.bad-aware'=>"@aware(['value' => 1 / 0]){{ \$value }}",
];
foreach($views as $name=>$source)file_put_contents($root.'/views/'.str_replace('.','/',$name).'.blade.php',$source);
$cases=[];
function add($template,$data=[],$group='scalar') {global $cases;$cases[]=['template'=>$template,'data'=>(object)$data,'group'=>$group];}
$expressions=[
 '1 + 2','2*-3','2--3','2++3','2- -3','2**!false','2**+2','1+2*3','(1+2)*3','20-3-2','20/2/2','2**3**2','-2**2','(-2)**2','2**-2','2**-2**2','!2**2','- -2','+false','-null','+" 12 "',
 '.5+1.','1e-2+2e+1','1+2e-2','1_000+2','0xff+0b11','010+0o10','-0.0','0.0/-1','4/2','4.0/2','7/2','7%3','-7%3','7%-3',
 '9223372036854775807+1','-9223372036854775808-1','-(-9223372036854775808)','3037000500*3037000500','-9223372036854775808/-1','-9223372036854775808%-1',
 '(-9223372036854775808)===(-9223372036854775807-1)','2**63','(-1)**9223372036854775807','1**9223372036854775807','0**0','0**-1','(-1)**0.5','1e309','-1e309','1e309*0','1e309-1e309','1e309/1e309',
 'INF+1','NAN+1','NAN==NAN','NAN===NAN','NAN!=NAN','NAN==true','INF>null','INF=="INF"','NAN=="NAN"',
 'false && (1/0)','true || (1/0)','null ?? 2+3*4','3 ?? (1/0)','(null ?? 2)+3','1+2===3','1.0+2===3','1.0+2===3.0',
 '1/0','1%0' ,'5.5%2','"5.5"%2','"foo"+1','"12foo"+1','""+1','[1]-1','[1]+1','[1]+[2] === [1]',"['x'=>1]+['x'=>2,'y'=>3] === ['x'=>1,'y'=>3]",
 '1 + $missing','$missing ?? 7','$data[\'missing\'] ?? 8','$data[1+1]',"(1e309 > 1) && (1e309 === INF)",
];
foreach($expressions as $e)add('{{ '.$e.' }}',['data'=>[10,20,30]]);
add("{{ [\$key => 'bad'] ?? 'invalid-key' }}",['key'=>[1]],'invalid-array');
add("{{ [\$last => 'full', 'bad'] ?? 'overflow' }}",['last'=>PHP_INT_MAX],'invalid-array');
add('@if ($price * $quantity >= 100)Bulk order @endif',['price'=>12.5,'quantity'=>8],'documentation');
$values=[null,false,true,0,1,-2,5.5,2.0,'0','2',' -3 ','2.5','1e2','9223372036854775808','1e309','12tail','tail',[],[1],['k'=>1]];
foreach(['+','-','*','/','%','**']as$op)foreach($values as$a)foreach([0,1,-2,2.5,'3','tail',[]]as$b)add('{{ $a '.$op.' $b }}',['a'=>$a,'b'=>$b],'coercion');
foreach([0,1,2,10,31,52,53,54,62,63,64,100,127,128,200,1000]as$bits){
 $hex='0x1'.str_repeat('0',$bits);add('{{ '.$hex.' + 0 }}',[],'radix');
}
foreach([
 '@if(1+2*3===7)<b>yes</b>@else<b>no</b>@endif',
 '@if(INF)<b>yes</b>@else<b>no</b>@endif',
 '@if(false){{ 1/0 }}@else<b>safe</b>@endif',
 '@isset($missing)<b>bad</b>@else<b>safe</b>@endisset',
 '@empty($missing)<b>safe</b>@endempty',
 '@foreach(null as $v)<b>bad</b>@endforeach',
 '@foreach(1+2 as $v)<b>bad</b>@endforeach',
 '@foreach([1+2,3*4] as $v){{ $v }};@endforeach',
 '@for($i=1+1;$i<2*3;$i+=1+1){{ $i }}@endfor',
 '<input @checked(2+2===4) @disabled(1/0)>',
 '@class(["ok"=>1+2===3])',
 '@include("part",["value"=>1+2])',
 '@includeWhen(false,"part",["value"=>1/0])',
 '@includeIf("absent",["value"=>1/0])',
 '<x-num :value="1+2" />',
 '<x-bad-prop :value="3" />',
 '<x-bad-aware :value="3" />',
]as$template)add($template,[],'contexts');
$results=[];
error_reporting(E_ALL);
// Mirrors fixed Laravel HandleExceptions: default deprecations log to null and continue.
set_error_handler(function($level,$message,$file,$line){if(in_array($level,[E_DEPRECATED,E_USER_DEPRECATED],true))return true;if(error_reporting()&$level)throw new ErrorException($message,0,$level,$file,$line);return false;});
foreach($cases as$i=>$case){
 $name='case_'.$i;file_put_contents($root.'/views/'.$name.'.blade.php',$case['template']);
 try{$result=$factory->make($name,(array)$case['data'])->render();$case['output']=$result;}
 catch(Throwable$e){$case['error']=get_class($e);$case['message']=$e->getMessage();}
 $results[]=$case;
}
restore_error_handler();
// Do not freeze a broken oracle's infrastructure errors into expected failures.
if(($results[0]['output']??null)!=='3')throw new RuntimeException('Basic oracle failed:'.json_encode($results[0]));
foreach($results as$r)if($r['template']==='<x-num :value="1+2" />' && ($r['output']??null)!=='3')throw new RuntimeException('Component oracle failed:'.json_encode($r));
$versions=[];foreach(Composer\InstalledVersions::getInstalledPackages()as$package)if(str_starts_with($package,'illuminate/')){$versions[$package]=Composer\InstalledVersions::getPrettyVersion($package);if($versions[$package]!=='v13.40.0')throw new RuntimeException('Unpinned Illuminate package');}
foreach($results as&$result){if(isset($result['message']))$result['message']=str_replace($root,'<oracle>',$result['message']);}unset($result);
echo json_encode(['php'=>PHP_VERSION,'illuminate'=>$versions,'views'=>$views,'cases'=>$results],JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR),"\n";
$files->deleteDirectory($root);
