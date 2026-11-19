<?php
namespace Symfony\Component\HttpFoundation { class Response {public function __construct(public $status=200,public $location=null,public $flash=null) {}} }
namespace Illuminate\Contracts\Support {interface Responsable {public function toResponse($request);}}
namespace Illuminate\Http {class JsonResponse extends \Symfony\Component\HttpFoundation\Response {public function __construct(public $body,$status=200){parent::__construct($status);}}}
namespace Illuminate\Routing {class Controller {}}
namespace Illuminate\Foundation\Http {class FormRequest {public function user(){return \Harness::$user;} public function route($key){return \Harness::$route[$key]??null;}public function wantsJson(){return \Harness::$json;}}}
namespace Illuminate\Auth\Events {class Verified {public function __construct(public $user){}}}
namespace Laravel\Fortify {class Fortify {const VERIFICATION_LINK_SENT='verification-link-sent';public static function redirects($name){if($name!=='email-verification')throw new \LogicException;return '/home';}}}
namespace {
    class Harness {public static $user,$json,$intended,$route=[],$events=[],$notifications=0;}
    class User {
        public function __construct(public $verified,public $save=true){}
        public function getKey(){return '001';}
        public function getEmailForVerification(){return 'a@example.test';}
        public function hasVerifiedEmail(){return $this->verified;}
        public function markEmailAsVerified(){if($this->save==='throw')throw new RuntimeException('save rejected');if($this->save)$this->verified=true;return $this->save;}
        public function sendEmailVerificationNotification(){Harness::$notifications++;}
    }
    class Redirector {
        public function intended($path){$target=Harness::$intended??$path;Harness::$intended=null;return new \Symfony\Component\HttpFoundation\Response(302,$target);}
        public function with($key,$value){return new \Symfony\Component\HttpFoundation\Response(302,'/email/verify',[$key=>$value]);}
    }
    function redirect(){return new Redirector;}
    function back(){return new Redirector;}
    function event($event){Harness::$events[]='Verified';}
    function app($name,$params=[]){return match($name){
        \Laravel\Fortify\Contracts\VerifyEmailResponse::class=>new \Laravel\Fortify\Http\Responses\VerifyEmailResponse,
        \Laravel\Fortify\Contracts\EmailVerificationNotificationSentResponse::class=>new \Laravel\Fortify\Http\Responses\EmailVerificationNotificationSentResponse,
        \Laravel\Fortify\Http\Responses\RedirectAsIntended::class=>new \Laravel\Fortify\Http\Responses\RedirectAsIntended(...$params),
        \Laravel\Fortify\Contracts\VerifyEmailViewResponse::class=>new class implements \Laravel\Fortify\Contracts\VerifyEmailViewResponse {public function toResponse($request){return new \Symfony\Component\HttpFoundation\Response;}},
    };}
    class_alias(\Illuminate\Foundation\Http\FormRequest::class, 'Illuminate\\Http\\Request');
    $root=$argv[1];
    foreach(['VerifyEmailResponse','EmailVerificationNotificationSentResponse','VerifyEmailViewResponse'] as $name)require $root.'/src/Contracts/'.$name.'.php';
    foreach(['VerifyEmailResponse','EmailVerificationNotificationSentResponse','RedirectAsIntended'] as $name)require $root.'/src/Http/Responses/'.$name.'.php';
    foreach(['EmailVerificationPromptController','VerifyEmailController','EmailVerificationNotificationController'] as $name)require $root.'/src/Http/Controllers/'.$name.'.php';
    require $root.'/src/Http/Requests/VerifyEmailRequest.php';
    $request=new \Laravel\Fortify\Http\Requests\VerifyEmailRequest;
    Harness::$user=new User(false);
    $authorization=[];
    foreach([['001',sha1('a@example.test')],['1',sha1('a@example.test')],['001',sha1('other@example.test')],['001',strtoupper(sha1('a@example.test'))],['',sha1('a@example.test')]] as [$id,$hash]){
        Harness::$route=compact('id','hash');$authorization[]=['id'=>$id,'hash'=>$hash,'authorized'=>$request->authorize()];
    }
    $cases=[];
    foreach([true,false] as $json)foreach([false,true] as $verified)foreach(['prompt','send','verify'] as $action)$cases[]=compact('json','verified','action')+['save'=>true,'intended'=>null];
    foreach([true,false] as $json)foreach([false,'throw'] as $save)$cases[]=['json'=>$json,'verified'=>false,'action'=>'verify','save'=>$save,'intended'=>null];
    foreach(['prompt','send','verify'] as $action)$cases[]=['json'=>false,'verified'=>true,'action'=>$action,'save'=>true,'intended'=>'/account/settings'];
    $output=[];
    foreach($cases as $case){Harness::$user=new User($case['verified'],$case['save']);Harness::$json=$case['json'];Harness::$intended=$case['intended'];Harness::$events=[];Harness::$notifications=0;
        try{$response=match($case['action']){
            'prompt'=>(new \Laravel\Fortify\Http\Controllers\EmailVerificationPromptController)($request),
            'send'=>(new \Laravel\Fortify\Http\Controllers\EmailVerificationNotificationController)->store($request),
            'verify'=>(new \Laravel\Fortify\Http\Controllers\VerifyEmailController)($request),
        };if($response instanceof \Illuminate\Contracts\Support\Responsable)$response=$response->toResponse($request);$status=$response->status;$location=$response->location;$flash=$response->flash;}catch(RuntimeException $error){$status=500;$location=null;$flash=null;}
        $output[]=$case+['status'=>$status,'location'=>$location,'flash'=>$flash,'after_verified'=>Harness::$user->verified,'events'=>Harness::$events,'notifications'=>Harness::$notifications];
    }
    echo json_encode(['authorization'=>$authorization,'cases'=>$output],JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),"\n";
}
