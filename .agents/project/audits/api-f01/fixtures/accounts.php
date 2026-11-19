<?php
namespace Illuminate\Foundation\Events {trait Dispatchable {}}
namespace Illuminate\Contracts\Auth {interface StatefulGuard {} interface PasswordBroker {}}
namespace Illuminate\Routing {class Controller {}}
namespace Illuminate\Support {class Str {public static function lower($value) {return strtolower($value);}}}
namespace Illuminate\Support\Facades {class Password {public static function broker($name) {if($name!=='accounts')throw new \LogicException('wrong broker');return new \Broker;}}}
namespace Illuminate\Auth\Events {class Registered {public function __construct(public $user) {}}}
namespace Illuminate\Http {class JsonResponse {public function __construct(public $body,public $status=200) {}}}
namespace Laravel\Fortify\Contracts {
    interface RegisterResponse {} interface RegisterViewResponse {} interface ProfileInformationUpdatedResponse {} interface PasswordUpdateResponse {}
}
namespace Laravel\Fortify {
    class Fortify {const PROFILE_INFORMATION_UPDATED='profile-information-updated';const PASSWORD_UPDATED='password-updated';public static function username(){return 'email';}}
}
namespace {
    class Harness {public static $user;public static $tokens=0;public static $events=[];public static $fail=false;public static $regenerations=0;}
    class User {public $name;public $email;public $password;}
    class Session {public function regenerate(){Harness::$regenerations++;}}
    class Request {
        public function __construct(private $input) {}
        public function has($key){return array_key_exists($key,$this->input);}
        public function __get($key){return $this->input[$key]??null;}
        public function merge($values){$this->input=array_replace($this->input,$values);}
        public function all(){return $this->input;}
        public function user(){return Harness::$user;}
        public function wantsJson(){return true;}
        public function boolean($key){return filter_var($this->input[$key]??false,FILTER_VALIDATE_BOOLEAN);}
        public function hasSession(){return true;}
        public function session(){return new Session;}
    }
    class_alias(Request::class, 'Illuminate\\Http\\Request');
    function config($key,$default=null){return match($key){'fortify.lowercase_usernames'=>true,'fortify.passwords'=>'accounts',default=>$default};}
    function event($event){Harness::$events[]=substr(strrchr($event::class,'\\'),1);}
    function app($name){return match($name){
        \Laravel\Fortify\Contracts\RegisterResponse::class=>new \Laravel\Fortify\Http\Responses\RegisterResponse,
        \Laravel\Fortify\Contracts\ProfileInformationUpdatedResponse::class=>new \Laravel\Fortify\Http\Responses\ProfileInformationUpdatedResponse,
        \Laravel\Fortify\Contracts\PasswordUpdateResponse::class=>new \Laravel\Fortify\Http\Responses\PasswordUpdateResponse,
    };}
    $root=$argv[1];
    foreach(['CreatesNewUsers','UpdatesUserProfileInformation','UpdatesUserPasswords'] as $name) require $root.'/src/Contracts/'.$name.'.php';
    foreach(['RegisteredUserController','ProfileInformationController','PasswordController'] as $name)require $root.'/src/Http/Controllers/'.$name.'.php';
    foreach(['RegisterResponse','ProfileInformationUpdatedResponse','PasswordUpdateResponse'] as $name)require $root.'/src/Http/Responses/'.$name.'.php';
    require $root.'/src/Events/PasswordUpdatedViaController.php';
    class Creator implements \Laravel\Fortify\Contracts\CreatesNewUsers {
        public function create(array $input){$user=new User;foreach(['name','email','password'] as $key)$user->$key=$input[$key];return Harness::$user=$user;}
    }
    class Profile implements \Laravel\Fortify\Contracts\UpdatesUserProfileInformation {
        public function update($user,$input){$user->name=$input['name'];$user->email=$input['email'];}
    }
    class Passwords implements \Laravel\Fortify\Contracts\UpdatesUserPasswords {
        public function update($user,$input){$user->password=$input['password'];}
    }
    class Guard implements \Illuminate\Contracts\Auth\StatefulGuard {public function login($user,$remember){Harness::$events[]='Login';}}
    class Broker implements \Illuminate\Contracts\Auth\PasswordBroker {public function deleteToken($user){if(Harness::$fail)throw new \RuntimeException('delete rejected');Harness::$tokens=0;}}
    $steps=[
        ['action'=>'register','input'=>['name'=>'Tester','email'=>'A@EXAMPLE.TEST','password'=>'good:secret','password_confirmation'=>'good:secret','remember'=>true]],
        ['action'=>'profile','input'=>['name'=>'Changed','email'=>'CHANGED@EXAMPLE.TEST']],
        ['action'=>'password','input'=>['current_password'=>'good:secret','password'=>'updated:secret','password_confirmation'=>'updated:secret']],
        ['action'=>'password','fail'=>true,'input'=>['current_password'=>'updated:secret','password'=>'next:secret','password_confirmation'=>'next:secret']],
    ];
    $output=[];
    foreach($steps as $step){Harness::$events=[];Harness::$fail=$step['fail']??false;if($step['action']==='password')Harness::$tokens=1;$request=new Request($step['input']);
        try{$response=match($step['action']){
            'register'=>(new \Laravel\Fortify\Http\Controllers\RegisteredUserController(new Guard))->store($request,new Creator),
            'profile'=>(new \Laravel\Fortify\Http\Controllers\ProfileInformationController)->update($request,new Profile),
            'password'=>(new \Laravel\Fortify\Http\Controllers\PasswordController)->update($request,new Passwords),
        };$response=$response->toResponse($request);$status=$response->status;}catch(\RuntimeException $error){$status=500;}
        $output[]=$step+['status'=>$status,'email'=>Harness::$user->email,'password'=>Harness::$user->password,'tokens'=>Harness::$tokens,'events'=>Harness::$events,'regenerations'=>Harness::$regenerations];
    }
    echo json_encode($output,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),"\n";
}
