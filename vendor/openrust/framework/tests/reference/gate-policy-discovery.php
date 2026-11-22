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


eval('namespace App\Models; class Article {} class Unmapped {}');
eval('namespace App\Models\Nested; class Entry {}');
eval('namespace App\Policies; class ArticlePolicy { public function view(?object $user) { return true; } }');
eval('namespace App\Alternate; class ArticlePolicy { public function view(?object $user) { return false; } }');
eval('namespace App\Policies\Nested; class EntryPolicy {}');
eval('namespace App\Models\Policies\Nested; class EntryPolicy {}');
$container=new Illuminate\Container\Container;
$default=new Illuminate\Auth\Access\Gate($container,fn()=>null);
$gate=new Illuminate\Auth\Access\Gate($container,fn()=>null);
$result=[];
$result['default']=get_class($default->getPolicyFor(new App\Models\Article))===App\Policies\ArticlePolicy::class;
$result['nested']=get_class($default->getPolicyFor(new App\Models\Nested\Entry))===App\Models\Policies\Nested\EntryPolicy::class;
$result['unmapped']=$default->getPolicyFor(new App\Models\Unmapped)===null;
$result['nonclass']=$default->getPolicyFor(42)===null;
$gate->guessPolicyNamesUsing(fn($class)=>[]);
$result['empty_callback']=$gate->getPolicyFor(new App\Models\Article)===null;
$gate->guessPolicyNamesUsing(fn($class)=>['MissingPolicy',App\Alternate\ArticlePolicy::class,App\Policies\ArticlePolicy::class]);
$result['first_existing']=get_class($gate->getPolicyFor(new App\Models\Article))===App\Alternate\ArticlePolicy::class;
$result['callback_decision']=!$gate->allows('view',[new App\Models\Article]);
$gate->policy(App\Models\Article::class,App\Policies\ArticlePolicy::class);
$result['explicit']=get_class($gate->getPolicyFor(new App\Models\Article))===App\Policies\ArticlePolicy::class;
$snapshot=$gate->policies();
$result['mapping']=$snapshot[App\Models\Article::class]===App\Policies\ArticlePolicy::class;
$snapshot=[];
$result['snapshot']=count($gate->policies())===1;
$result['other_app']=$default->policies()===[];
echo json_encode($result,JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR)."\n";
