<?php
require $argv[1].'/vendor/autoload.php';
require $argv[2].'/AbstractHasher.php';
require $argv[2].'/BcryptHasher.php';
require $argv[2].'/CreatesUserProviders.php';
require $argv[2].'/AuthManager.php';
require $argv[2].'/TransformsRequest.php';
require $argv[2].'/ConvertEmptyStringsToNull.php';
class InputNormalizer extends Illuminate\Foundation\Http\Middleware\ConvertEmptyStringsToNull {function normalize($value){return $this->transform('current_password',$value);}}
$normalizer=new InputNormalizer;
use Illuminate\Translation\{Translator,ArrayLoader};
use Illuminate\Validation\Factory;
use Illuminate\Container\Container;
class TestUser { function __construct(private $hash) {} function getAuthPassword(){return $this->hash;} }
class TestGuard { function __construct(private $user){} function guest(){return $this->user===null;} function user(){return $this->user;} }
class TestAuth extends Illuminate\Auth\AuthManager {
 function __construct(private $default,private $users){}
 function getDefaultDriver(){return $this->default;}
 protected function resolve($name){if(!array_key_exists($name,$this->users))throw new RuntimeException('Unknown guard');return new TestGuard($this->users[$name]);}
}
$loader=new ArrayLoader;$loader->addMessages('en','validation',require $argv[1].'/validation.php');
$container=new Container;$factory=new Factory(new Translator($loader,'en'),$container);
$hasher=new Illuminate\Hashing\BcryptHasher(['rounds'=>4]);
$hashes=['normal'=>$hasher->make('correct-password'),'empty'=>$hasher->make(''),'number'=>$hasher->make('123')];
$warnings=[];set_error_handler(function($severity,$message)use(&$warnings){if($severity===E_DEPRECATED){$warnings[]=$message;return true;}return false;});
$cases=[];
foreach($hashes as $profile=>$hash)foreach(['web','admin'] as $default)foreach(['current_password','current_password:web','current_password:admin','current_password:missing','current_password:','current_password:0'] as $rule)foreach(['correct-password','wrong-password',null,false,true,123,[],['a'],'',' ',"\0",1.25,12345678] as $value){
 $warnings=[];
 $container->instance('auth',new TestAuth($default,['web'=>new TestUser($hash),'admin'=>null]));$container->instance('hash',new Illuminate\Hashing\BcryptHasher);
 try{$v=$factory->make(['current_password'=>$normalizer->normalize($value)],['current_password'=>$rule]);$passes=$v->passes();$cases[]=['profile'=>$profile,'default'=>$default,'rule'=>$rule,'value'=>$value,'passes'=>$passes,'messages'=>$v->errors()->toArray(),'php_deprecations'=>$warnings];}
 catch(Throwable $e){$cases[]=['profile'=>$profile,'default'=>$default,'rule'=>$rule,'value'=>$value,'exception'=>get_class($e)];}
}
echo json_encode(['php'=>PHP_VERSION,'cases'=>$cases],JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR)."\n";
