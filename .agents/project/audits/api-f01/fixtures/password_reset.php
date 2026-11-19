<?php
namespace Symfony\Component\HttpFoundation {class Response {function __construct(public $status=302,public $body=null,public $location=null,public $flash=[],public $errors=[],public $old=[]) {}}}
namespace Illuminate\Contracts\Support {interface Responsable {function toResponse($request);}}
namespace Illuminate\Contracts\Auth {interface StatefulGuard {} interface PasswordBroker {}}
namespace Illuminate\Routing {class Controller {}}
namespace Illuminate\Http {class JsonResponse extends \Symfony\Component\HttpFoundation\Response {function __construct($body,$status=200){parent::__construct($status,$body);}} class Request {
 function __construct(public $input){}
 function has($key){return array_key_exists($key,$this->input);}
 function merge($value){$this->input=array_merge($this->input,$value);}
 function __get($key){return $this->input[$key]??null;}
 function only(...$keys){return array_intersect_key($this->input,array_flip($keys));}
 function all(){return $this->input;}
 function validate($rules){\Harness::$log[]='validate';\Harness::$validated=$this->input;}
 function wantsJson(){return \Harness::$json;}
}}
namespace Illuminate\Foundation\Http {class FormRequest extends \Illuminate\Http\Request {}}
namespace Illuminate\Support {class Str {static function lower($v){return strtolower($v);}static function random($n){return str_repeat('x',$n);}}}
namespace Illuminate\Support\Facades {class Password {
 const RESET_LINK_SENT='passwords.sent';const PASSWORD_RESET='passwords.reset';
 static function broker($name){\Harness::$log[]='broker:'.$name;return new \Broker;}
}}
namespace Illuminate\Auth\Events {class PasswordReset {function __construct($user){}}}
namespace Illuminate\Validation {class ValidationException extends \RuntimeException {
 function __construct(public $errors){parent::__construct('validation');}
 static function withMessages($errors){return new self($errors);}
}}
namespace Laravel\Fortify {class Fortify {
 static function email(){return \Harness::$email;}
 static function redirects($name,$default){return \Harness::$redirect??$default??'/home';}
}}
namespace {
class Harness {static $json,$email,$status,$lower,$redirect,$actionError;static $log=[],$credentials=[],$validated=null;}
class Guard implements \Illuminate\Contracts\Auth\StatefulGuard {}
class User {public $remember=null;function setRememberToken($value){$this->remember=$value;Harness::$log[]='remember:'.strlen($value);}function save(){Harness::$log[]='save';}}
class Broker implements \Illuminate\Contracts\Auth\PasswordBroker {
 function sendResetLink($input){Harness::$credentials=$input;Harness::$log[]='send';return Harness::$status;}
 function reset($input,$callback){Harness::$credentials=$input;Harness::$log[]='reset';if(Harness::$status==='passwords.reset')$callback(new User);return Harness::$status;}
}
class Action {function reset($user,$input){Harness::$log[]='action';if(Harness::$actionError)throw \Illuminate\Validation\ValidationException::withMessages(['password'=>['Application rejected password.']]);}}
class RedirectResponse extends \Symfony\Component\HttpFoundation\Response {
 function with($key,$value){$this->flash[$key]=$value;return $this;}
 function withInput($input){$this->old=$input;return $this;}
 function withErrors($errors){$this->errors=$errors;return $this;}
}
function config($key,$default=null){return match($key){'fortify.lowercase_usernames'=>Harness::$lower,'fortify.passwords'=>'accounts','fortify.views'=>true,default=>$default};}
function route($name){return 'http://localhost/login';}
function redirect($target){return new RedirectResponse(location:$target);}
function back(){return new RedirectResponse(location:'/forgot-password');}
function trans($key){return match($key){'passwords.sent'=>'We have emailed your password reset link.','passwords.reset'=>'Your password has been reset.','passwords.user'=>"We can't find a user with that email address.",'passwords.token'=>'This password reset token is invalid.','passwords.throttled'=>'Please wait before retrying.',default=>$key};}
function event($event){Harness::$log[]='PasswordReset';}
function app($name,$params=[]){
 if($name===\Laravel\Fortify\Contracts\ResetsUserPasswords::class)return new Action;
 if($name===\Laravel\Fortify\Actions\CompletePasswordReset::class)return new \Laravel\Fortify\Actions\CompletePasswordReset;
 $short=substr($name,strrpos($name,'\\')+1);$class='Laravel\\Fortify\\Http\\Responses\\'.$short;return new $class(...$params);
}
$root=$argv[1];
$names=['PasswordResetResponse','FailedPasswordResetResponse','SuccessfulPasswordResetLinkRequestResponse','FailedPasswordResetLinkRequestResponse','ResetPasswordViewResponse','RequestPasswordResetLinkViewResponse','ResetsUserPasswords'];
foreach($names as $name)require $root.'/src/Contracts/'.$name.'.php';
foreach(array_slice($names,0,4) as $name)require $root.'/src/Http/Responses/'.$name.'.php';
foreach(['PasswordResetLinkController','NewPasswordController'] as $name)require $root.'/src/Http/Controllers/'.$name.'.php';
require $root.'/src/Actions/CompletePasswordReset.php';
require $root.'/src/Http/Requests/SendPasswordResetLinkRequest.php';
$cases=[];
foreach([true,false] as $json)foreach(['send','reset'] as $operation)foreach($operation==='send'?['passwords.sent','passwords.user','passwords.throttled']:['passwords.reset','passwords.user','passwords.token'] as $status){
 $cases[]=compact('json','operation','status')+['email_field'=>'email','lower'=>true,'action_error'=>false,'redirect'=>null];
}
foreach([true,false] as $json)foreach(['send','reset'] as $operation)$cases[]=compact('json','operation')+['status'=>$operation==='send'?'passwords.user':'passwords.token','email_field'=>'contact','lower'=>true,'action_error'=>false,'redirect'=>null];
foreach([true,false] as $json)$cases[]=compact('json')+['operation'=>'reset','status'=>'passwords.reset','email_field'=>'email','lower'=>true,'action_error'=>true,'redirect'=>null];
$cases[]=['json'=>false,'operation'=>'reset','status'=>'passwords.reset','email_field'=>'email','lower'=>true,'action_error'=>false,'redirect'=>'/done'];
$output=[];
foreach($cases as $case){
 Harness::$json=$case['json'];Harness::$email=$case['email_field'];Harness::$status=$case['status'];Harness::$lower=$case['lower'];Harness::$actionError=$case['action_error'];Harness::$redirect=$case['redirect'];Harness::$log=[];Harness::$credentials=[];Harness::$validated=null;
 $input=[Harness::$email=>'A@EXAMPLE.TEST','token'=>'token123','password'=>'changed:secret','password_confirmation'=>'changed:secret','extra'=>'kept'];
 $request=$case['operation']==='send'?new \Laravel\Fortify\Http\Requests\SendPasswordResetLinkRequest($input):new \Illuminate\Http\Request($input);
 if($case['operation']==='send'){Harness::$log[]='validate';Harness::$validated=$input;}
 try {
  $response=$case['operation']==='send'?(new \Laravel\Fortify\Http\Controllers\PasswordResetLinkController)->store($request):(new \Laravel\Fortify\Http\Controllers\NewPasswordController(new Guard))->store($request);
  $response=$response->toResponse($request);$result=['http'=>$response->status,'body'=>$response->body,'location'=>$response->location,'flash'=>$response->flash,'errors'=>$response->errors,'old'=>$response->old];
 }catch(\Illuminate\Validation\ValidationException $error){$result=['http'=>422,'errors'=>$error->errors];}
 $output[]=$case+$result+['log'=>Harness::$log,'credentials'=>Harness::$credentials,'validated'=>Harness::$validated,'request'=>$request->all()];
}
echo json_encode($output,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR)."\n";
}
