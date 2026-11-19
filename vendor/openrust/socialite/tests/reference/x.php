<?php
namespace Illuminate\Support { class Arr {public static function first($array,$callback){foreach($array as $key=>$value){if($callback($value,$key)){return $value;}}return null;}public static function get($array,$key,$default=null){if(is_array($array)&&array_key_exists($key,$array)){return $array[$key];}foreach(explode('.',$key) as $part){if(!is_array($array)||!array_key_exists($part,$array)){return $default;}$array=$array[$part];}return $array;}} }
namespace Illuminate\Http {
 class Session {public $data=[];function __construct($data){$this->data=$data;}function pull($key){$v=$this->data[$key]??null;unset($this->data[$key]);return $v;}function get($key){return $this->data[$key]??null;}}
 class Request {private $session;private $input;function __construct($state=null,$incoming=null){$this->session=new Session(['state'=>$state]);$this->input=['state'=>$incoming];}function session(){return $this->session;}function input($name){return $this->input[$name]??null;}}
}
namespace GuzzleHttp {
 class RequestOptions {const HEADERS='headers';const FORM_PARAMS='form_params';const AUTH='auth';const QUERY='query';}
 class Client {public $calls=[];public $mode='ok';function get($url,$options){$this->calls[]=[$url,$options];$data=str_ends_with($url,'/emails')?[['primary'=>true,'verified'=>false,'email'=>'bad@test'],['primary'=>true,'verified'=>true,'email'=>'alice@test']]:['id'=>42,'node_id'=>'node-42','login'=>'alice','name'=>'Alice','email'=>'unverified@test','avatar_url'=>'https://images.test/alice'];if($this->mode==='email-error'&&str_ends_with($url,'/emails')){throw new \Exception('email failure');}return new Reply($data);}function post($url,$options){$this->calls[]=[$url,$options];return new Reply(['access_token'=>'access','refresh_token'=>'refresh','expires_in'=>3600,'scope'=>'read,user:email']);}}
 class Reply {function __construct(private $data){}function getBody(){return json_encode($this->data);}}
}
namespace {
$dir=$argv[1];$expected=json_decode(file_get_contents(__DIR__.'/x-source-sha256.json'),true,512,JSON_THROW_ON_ERROR);
foreach($expected as $file=>$hash){if(hash_file('sha256',$dir.'/'.$file)!==$hash){throw new \RuntimeException('Unexpected Socialite reference source');}}
foreach(array_keys($expected) as $file){require $dir.'/'.$file;}
trait Expose { function fields(){return $this->getCodeFields('fixed-state');}function mapping($raw){return $this->mapUserToObject($raw);}function refresh($token){return $this->getRefreshTokenResponse($token);} }
class TwitterProbe extends \Laravel\Socialite\Two\TwitterProvider {use Expose;}
class XProbe extends \Laravel\Socialite\Two\XProvider {use Expose;}
$out=[];$raw=['id'=>'user-42','username'=>'alice','name'=>'Alice','confirmed_email'=>'alice@test','profile_image_url'=>'https://images.test/alice'];
foreach(['twitter'=>TwitterProbe::class,'x'=>XProbe::class] as $name=>$class){
 $r=new \Illuminate\Http\Request();$r->session()->data['code_verifier']=str_repeat('v',96);$p=new $class($r,'client','secret','https://rp.test/callback');
 $out[$name]['default']=$p->fields();$p->stateless();$out[$name]['stateless']=$p->fields();
 $p->with(['prompt'=>'select account + ~ *','state'=>'override']);$out[$name]['parameters']=$p->fields();$out[$name]['url']=$p->getAuthUrl('fixed-state');
 $u=$p->mapping($raw);$out[$name]['user']=['id'=>$u->getId(),'nickname'=>$u->getNickname(),'name'=>$u->getName(),'email'=>$u->getEmail(),'avatar'=>$u->getAvatar(),'raw'=>$u->getRaw(),'attributes'=>$u->attributes];
 $http=new \GuzzleHttp\Client();$p->setHttpClient($http);$p->getAccessTokenResponse('fixed-code');$p->refresh('refresh-input');$out[$name]['http']=$http->calls;
}
echo json_encode($out,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR)."\n";
}
