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
$compiler->directive('tag',fn()=> '<?php echo "<tag>"; ?>');
$compiler->directive('emit',fn($expression)=> "<?php echo e($expression); ?>");
Illuminate\Support\Facades\Facade::setFacadeApplication($container);
$compiler->if('active',fn($value=true)=>$value);
$cases=[];
function add($template,$group='lexical') {global $cases;$cases[]=['template'=>$template,'data'=>(object)[],'group'=>$group];}
foreach(['','x','0','_','$','-','.', '>', '中', 'é', "\u{a0}","\n",'a{{-- c --}}','{{-- c --}}','a{{--a--}}{{--b--}}']as$prefix)add($prefix.'@if(true)<b>yes</b>@endif','prefix');
foreach(['x','0','_','中','é','!','>',' ']as$body)add('@if(true)'.$body.'@endif','ending');
foreach(['', ' ', "\t", "\n", "\r\n", "\n\n", " \n", "\r", "\v", "\f", "\u{a0}","\0"]as$space){
 add('<s>start</s>@if(true)'.$space.'<b>A</b>@endif<e>end</e>','opening-space');
 add('<s>start</s>@if(false)<b>A</b>@else'.$space.'<b>B</b>@endif<e>end</e>','else-space');
 add('<s>start</s>@if(true)<b>A</b>@endif'.$space.'<e>end</e>','closing-space');
 add($space.'<p>plain</p>','leading');
 add('<s>start</s>@tag'.$space.'<e>end</e>','custom-space');
 add('<s>start</s>@emit("A")'.$space.'<e>end</e>','custom-arguments');
 add('<s>start</s>@active'.$space.'<b>A</b>@endactive<e>end</e>','condition-space');
}
foreach(['@unknown(x)','@@unknown(x)','@@@unknown(x)','@@@@unknown','@@','@@@','@if::custom(true)','@unknown::x','@@if(true)','@IF(true)<b>yes</b>@ENDIF','@If(true)<b>yes</b>@EnDiF','@Tag','email@@unknown ','email@@if(true)<b>A</b>@endif','@if'."\n".'(true)<b>x</b>@endif','@if'."\t".'(true)<b>x</b>@endif','@if(true)<b>X</b>@endif(1/0)' ,"@if(true)<b>X</b>@endif()\n\nend", '@verbatim @if(true)word@endif @endverbatim', 'word@verbatim@if(true)word@endif@endverbatim', '@@verbatim text @@endverbatim', '@verbatimX@endverbatim@if(true)<b>Y</b>@endif','@verbatimX@endverbatim{{--c--}}@if(true)<b>Y</b>@endif','@active(true)<b>A</b>@elseactive(false)<b>B</b>@else<b>C</b>@endactive','@active(false)<b>A</b>@elseactive(true)<b>B</b>@else<b>C</b>@endactive', '@active(false)<b>A</b>@else<b>C</b>@endactive']as$template)add($template);
foreach(["<s>A</s>@active\n(true)<b>X</b>@endactive","<s>A</s>@tag\n(1/0)","<s>A</s>@if \t\n(true)<b>X</b>@endif","<s>A</s>{{ 'B' }}\nC","<s>A</s>{!! 'B' !!}\nC",'word@if(true)text@endif']as$template)add($template,'edge');
foreach(['a{{-- {{-- nested --}}@if(true)<b>A</b>@endif','<b>p</b>{{-- {{-- nested --}}@if(true)<i>A</i>@endif','a{{-- first --}}{{-- {{-- nested --}}@if(true)<b>A</b>@endif']as$template)add($template,'comment-span');
add("@if{{-- c --}}(true)<b>A</b>@endif",'preprocessing');
add("@i{{-- c --}}f(true)<b>A</b>@endif",'preprocessing');
add("@if(true)<b>A</b>@end{{-- c --}}if",'preprocessing');
add("{{ 1 {{-- c --}} + 2 }}",'preprocessing');
add("{!! 1 {{-- c --}} + 2 !!}",'preprocessing');
add("{{ 'a{{-- c --}}b' }}",'preprocessing');
add("{{ '{{-- missing --}}' }}",'preprocessing');
add("@if('x{{-- c --}}y' == 'xy')<b>A</b>@endif",'preprocessing');
add("a{{-- c --}}@if(true)<b>A</b>@endif",'preprocessing');
add("@verbatim{{ 1/0 }}{{-- keep --}}<x-missing />@endverbatim",'preprocessing');
add("@verbatim@if(false)<b>A</b>@endif@endverbatim",'preprocessing');
add("word@verbatim{{ 1/0 }}@endverbatim@if(true)<b>A</b>@endif",'preprocessing');
add("{{-- @verbatim {{-- keep --}} @endverbatim --}}<b>A</b>",'preprocessing');
add("@ver{{-- c --}}batim<b>A</b>@endverbatim",'preprocessing');
add("@verbatim{{-- keep --}}@endverbatim{{-- gone --}}<b>A</b>",'preprocessing');
add("@@verbatim{{-- gone --}}@@endverbatim",'preprocessing');
add("@verbatim incomplete",'preprocessing');
add("{{-- incomplete",'preprocessing');
add("{{ incomplete",'preprocessing');
add("{!! incomplete",'preprocessing');
add("@{{ incomplete",'preprocessing');
add("@{!! incomplete",'preprocessing');
add("<x-num :value=\"1 {{-- c --}} + 2\" />",'preprocessing');
add("@include{{-- c --}}('part', ['value' => 7])",'preprocessing');
add("@tag{{-- c --}}",'preprocessing');
add("@active{{-- c --}}(true)<b>A</b>@endactive",'preprocessing');
add("{{ '{{--x--}}{{--y--}}' }}",'preprocessing');
add("@verbatimA@endverbatim@verbatimB@endverbatim",'preprocessing');
add("@verbatim{{-- @endverbatim --}}<b>A</b>",'preprocessing');
add("{{ 1 + {{-- nested {{-- --}} 2 }}",'preprocessing');
foreach(["", " ", "\t", "\n", "\r\n", "\r", "\v", "\f", "\u{a0}"] as $space) {
 add("<s>A</s>@if(true)@verbatim".$space."{{ 1/0 }}{{-- keep --}}@endverbatim<b>B</b>@endif", "raw-leading");
 add("<s>A</s>@verbatim".$space."{{ x }}@endverbatim<b>B</b>", "raw-leading");
}
add("{{ '@verbatim<b>A</b>@endverbatim' }}",'raw-expressions');
add("{!! '@verbatim<b>A</b>@endverbatim' !!}",'raw-expressions');
add("@if('@verbatimA@endverbatim' == 'A')<b>A</b>@endif",'raw-expressions');
add("<x-num :value=\"'@verbatim7@endverbatim'\" />",'raw-expressions');
add("<x-num value=\"@verbatim7@endverbatim\" />",'raw-expressions');
add("@{{ '@verbatimA@endverbatim' }}",'raw-expressions');
add("@include('part', ['value' => '@verbatimA@endverbatim'])",'raw-expressions');
add('{{ 1/0 }}','errors');
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
// Compare fragment buffers separately from the engine's trimmed full-view fallback.
$fragmentSource=<<<'BLADE'
@fragment('left')@once L{{ $pass }}:{{ $sharedFragmentPass }}@endonce @endfragment{{ '' }}@fragment('right')R{{ $pass }}:{{ $sharedFragmentPass }}@endfragment
BLADE;
file_put_contents($root.'/views/fragment_composed.blade.php',$fragmentSource);
$factory->creator('fragment_composed',function($view){$view->with('pass',0);});
$factory->composer('fragment_composed',function($view)use($factory){$pass=$view->getData()['pass']+1;$view->with('pass',$pass);$factory->share('sharedFragmentPass',$pass);});
$fragmentCases=[];
foreach(['named','all','empty','missing','left']as$mode){
 $factory->share('sharedFragmentPass',0);$view=$factory->make('fragment_composed');
 $output=match($mode){'named'=>$view->fragments(['right','missing','left','left']),'all'=>$view->fragments(),'empty'=>$view->fragments([]),'missing'=>$view->fragment('missing'),'left'=>$view->fragment('left')};
 $fragmentCases[$mode]=['output'=>$output,'pass'=>$view->getData()['pass']];
}
$fragments=['source'=>$fragmentSource,'cases'=>$fragmentCases];
$overrides=[];
$compiler->directive('if',fn()=>'<override>');
foreach(['directive'=>'@if(true)']as$name=>$template){file_put_contents($root.'/views/override_'.$name.'.blade.php',$template);$overrides[$name]=['template'=>$template,'output'=>$factory->make('override_'.$name)->render()];}
$compiler->if('if',fn()=>false);
$template='@if(true)<b>A</b>@else<b>B</b>@endif';file_put_contents($root.'/views/override_condition.blade.php',$template);$overrides['condition']=['template'=>$template,'output'=>$factory->make('override_condition')->render()];
$versions=[];foreach(Composer\InstalledVersions::getInstalledPackages()as$package)if(str_starts_with($package,'illuminate/')){$versions[$package]=Composer\InstalledVersions::getPrettyVersion($package);if($versions[$package]!=='v13.40.0')throw new RuntimeException('Unpinned package');}
foreach($results as&$case){if(isset($case['message']))$case['message']=str_replace($root,'<oracle>',$case['message']);}unset($case);
if(($results[0]['output']??null)!=='<b>yes</b>')throw new RuntimeException('Broken oracle');
echo json_encode(['php'=>PHP_VERSION,'illuminate'=>$versions,'views'=>$views,'overrides'=>$overrides,'fragments'=>$fragments,'cases'=>$results],JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR),"\n";
$files->deleteDirectory($root);
