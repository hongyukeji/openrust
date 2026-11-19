<?php
namespace Illuminate\Support { class Arr {public static function get($array,$key,$default=null){foreach(explode('.',$key) as $part){if(!is_array($array)||!array_key_exists($part,$array)){return $default;}$array=$array[$part];}return $array;}} }
namespace Illuminate\Http {
 class Session {public $data=[];function __construct($data){$this->data=$data;}function pull($key){$v=$this->data[$key]??null;unset($this->data[$key]);return $v;}function get($key){return $this->data[$key]??null;}}
 class Request {private $session;private $input;function __construct($state=null,$incoming=null){$this->session=new Session(['state'=>$state]);$this->input=['state'=>$incoming];}function session(){return $this->session;}function input($name){return $this->input[$name]??null;}}
}
namespace GuzzleHttp {
 class RequestOptions {const HEADERS='headers';const FORM_PARAMS='form_params';}
 class Client {public $calls=[];public $mode='ok';function get($url,$options){$this->calls[]=[$url,$options];$data=str_ends_with($url,'/emails')?[['primary'=>true,'verified'=>false,'email'=>'bad@test'],['primary'=>true,'verified'=>true,'email'=>'alice@test']]:['id'=>42,'node_id'=>'node-42','login'=>'alice','name'=>'Alice','email'=>'unverified@test','avatar_url'=>'https://images.test/alice'];if($this->mode==='email-error'&&str_ends_with($url,'/emails')){throw new \Exception('email failure');}return new Reply($data);}function post($url,$options){$this->calls[]=[$url,$options];return new Reply(['access_token'=>'access','refresh_token'=>'refresh','expires_in'=>3600,'scope'=>'read,user:email']);}}
 class Reply {function __construct(private $data){}function getBody(){return json_encode($this->data);}}
}
namespace {
 $dir=$argv[1];$expected=json_decode(file_get_contents(__DIR__.'/source-sha256.json'),true,512,JSON_THROW_ON_ERROR);
 foreach($expected as $file=>$hash){if(hash_file('sha256',$dir.'/'.$file)!==$hash){throw new \RuntimeException('Unexpected Socialite reference source');}}
 foreach(array_keys($expected) as $file){require $dir.'/'.$file;}
 class Probe extends \Laravel\Socialite\Two\GithubProvider {function fields(){return $this->getCodeFields('fixed-state');}function invalid(){return $this->hasInvalidState();}}
 function probe($state=null,$incoming=null){return new Probe(new \Illuminate\Http\Request($state,$incoming),'client','secret','https://rp.test/callback');}
 $out=[];
 $p=probe();$out['default']=$p->fields();
 $p->scopes(['read','user:email','read']);$out['merged']=$p->fields();
 $p->setScopes(['profile','profile','email']);$out['set']=$p->fields();
 $p->stateless();$out['stateless']=$p->fields();
 $p->with(['prompt'=>'select account','login_hint'=>'中文 & +','scope'=>'override']);$out['parameters']=$p->fields();
 foreach([[null,null],['abc','abc'],['abc','wrong'],['abc',null],['abc',['abc']],['',''],['0','0']] as $i=>$pair){$p=probe(...$pair);$out['state'][]=['first'=>$p->invalid(),'replay'=>$p->invalid()];}
 $p=probe('abc','wrong')->stateless();$out['stateless_state']=$p->invalid();
 $http=new \GuzzleHttp\Client();$p=probe()->setHttpClient($http);$u=$p->userFromToken('access');$out['user']=['id'=>$u->getId(),'nickname'=>$u->getNickname(),'name'=>$u->getName(),'email'=>$u->getEmail(),'avatar'=>$u->getAvatar(),'raw'=>$u->getRaw(),'attributes'=>$u->attributes];$out['calls']=$http->calls;
 $http->mode='email-error';$out['email_failure']=$p->userFromToken('access')->getEmail();
 $token=$p->refreshToken('refresh-input');$out['refresh']=['token'=>$token->token,'refreshToken'=>$token->refreshToken,'expiresIn'=>$token->expiresIn,'approvedScopes'=>$token->approvedScopes];$out['refresh_call']=end($http->calls);
 echo json_encode($out,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR)."\n";
}
