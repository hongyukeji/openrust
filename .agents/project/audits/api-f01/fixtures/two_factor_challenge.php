<?php
namespace Illuminate\Contracts\Auth { interface StatefulGuard {} }
namespace Illuminate\Routing { class Controller {} }
namespace Illuminate\Http\Exceptions {
    class HttpResponseException extends \Exception { public function __construct(public $response) {} }
}
namespace Illuminate\Foundation\Http {
    class FormRequest {
        public function __construct(private $data,private $session) {}
        public function __get($key) {return $this->data[$key]??null;}
        public function session() {return $this->session;}
        public function filled($key) {return isset($this->data[$key]) && trim($this->data[$key])!=='';}
        public function wantsJson() {return true;}
    }
}
namespace Laravel\Fortify {
    class Fortify {public static function currentEncrypter() {return new \Cipher;}}
}
namespace Laravel\Fortify\Contracts {
    interface TwoFactorChallengeViewResponse {}
    interface TwoFactorLoginResponse {}
    interface FailedTwoFactorLoginResponse {}
    interface TwoFactorAuthenticationProvider {}
}
namespace Laravel\Fortify\Events {
    class TwoFactorAuthenticationFailed {public function __construct(public $user) {}}
    class ValidTwoFactorAuthenticationCodeProvided {public function __construct(public $user) {}}
}
namespace {
    class Harness {public static $user;public static $events=[];}
    class Session {
        public $values=['login.id'=>'001','login.remember'=>true];
        public function has($key) {return isset($this->values[$key]);}
        public function get($key) {return $this->values[$key]??null;}
        public function forget($key) {unset($this->values[$key]);}
        public function pull($key,$default=null) {$value=$this->get($key)??$default;$this->forget($key);return $value;}
        public function regenerate() {Harness::$events[]='regenerate';}
    }
    class Cipher {public function decrypt($value) {return $value;}}
    class User {
        public $two_factor_secret='PUBLIC';
        public $codes=['recovery-0','0'];
        public static function find($key) {return $key==='001' ? Harness::$user : null;}
        public function recoveryCodes() {return $this->codes;}
        public function replaceRecoveryCode($code) {Harness::$events[]='RecoveryCodeReplaced';$this->codes=array_map(fn($value)=>str_replace($code,'replacement',$value),$this->codes);}
    }
    class Provider {public function getModel() {return User::class;}}
    class Guard implements \Illuminate\Contracts\Auth\StatefulGuard {
        public function getProvider() {return new Provider;}
        public function login($user,$remember) {Harness::$events[]='Login';}
    }
    class Engine {public function verify($secret,$code) {return $secret==='PUBLIC' && $code==='287082';}}
    class Failure {public function toResponse($request) {return ['status'=>422,'field'=>$request->filled('recovery_code')?'recovery_code':'code'];}}
    function app($name) {return match ($name) {
        \Illuminate\Contracts\Auth\StatefulGuard::class=>new Guard,
        \Laravel\Fortify\Contracts\FailedTwoFactorLoginResponse::class=>new Failure,
        \Laravel\Fortify\Contracts\TwoFactorAuthenticationProvider::class=>new Engine,
        \Laravel\Fortify\Contracts\TwoFactorLoginResponse::class=>['status'=>204],
    };}
    function event($event) {Harness::$events[]=substr(strrchr($event::class,'\\'),1);}
    function collect($values) {return new class($values) {public function __construct(private $values) {} public function first($callback) {foreach($this->values as $value) {if($callback($value))return $value;}return null;}};}
    function tap($value,$callback) {$callback($value);return $value;}
    require $argv[1].'/src/Http/Requests/TwoFactorLoginRequest.php';
    require $argv[1].'/src/Http/Controllers/TwoFactorAuthenticatedSessionController.php';
    $controller=new \Laravel\Fortify\Http\Controllers\TwoFactorAuthenticatedSessionController(new Guard);
    $inputs=[[],['code'=>'0'],['code'=>'bad'],['code'=>'287082'],['recovery_code'=>'bad'],['recovery_code'=>'0'],['recovery_code'=>'recovery-0','code'=>'bad'],['recovery_code'=>'bad','code'=>'287082'],['recovery_code'=>'','code'=>'287082'],['recovery_code'=>' ','code'=>'bad']];
    $cases=[];
    foreach ([true,false] as $exists) foreach($inputs as $input) {
        Harness::$user=$exists?new User:null;Harness::$events=[];$session=new Session;
        $request=new \Laravel\Fortify\Http\Requests\TwoFactorLoginRequest($input,$session);
        try {$response=$controller->store($request);} catch(\Illuminate\Http\Exceptions\HttpResponseException $exception) {$response=$exception->response;}
        // Native session regeneration is separately exercised; normalize it out of the event list.
        $cases[]=['exists'=>$exists,'input'=>(object)$input,'status'=>$response['status'],'field'=>$response['field']??null,'challenge'=>$session->get('login.id'),'remember'=>$session->get('login.remember'),'events'=>array_values(array_filter(Harness::$events,fn($event)=>$event!=='regenerate'))];
    }
    echo json_encode($cases,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),"\n";
}
