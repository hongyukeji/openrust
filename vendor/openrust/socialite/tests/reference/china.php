<?php
namespace Illuminate\Support { class Arr {public static function first($array,$callback){foreach($array as $key=>$value){if($callback($value,$key)){return $value;}}return null;}public static function get($array,$key,$default=null){if(is_array($array)&&array_key_exists($key,$array)){return $array[$key];}foreach(explode('.',$key) as $part){if(!is_array($array)||!array_key_exists($part,$array)){return $default;}$array=$array[$part];}return $array;}} }
namespace Illuminate\Http {
 class Session {public $data=[];function __construct($data){$this->data=$data;}function pull($key){$v=$this->data[$key]??null;unset($this->data[$key]);return $v;}function get($key){return $this->data[$key]??null;}}
 class Request {private $session;private $input;function __construct($state=null,$incoming=null){$this->session=new Session(['state'=>$state]);$this->input=['state'=>$incoming];}function session(){return $this->session;}function input($name){return $this->input[$name]??null;}}
}
namespace GuzzleHttp {
 class RequestOptions {const QUERY='query';const HEADERS='headers';const FORM_PARAMS='form_params';}
 class Client {public $calls=[];function get($url,$options){$this->calls[]=[$url,$options];if(str_ends_with($url,'/me')){$data=['client_id'=>'client','openid'=>'id-fixed','unionid'=>'union-id'];}elseif(str_ends_with($url,'/get_user_info')){$data=['ret'=>0,'nickname'=>'读者','figureurl_qq_2'=>'https://images.test/qq'];}elseif(str_contains($url,'access_token')){$data=['access_token'=>'private-token','openid'=>'id-fixed','refresh_token'=>'private-refresh','expires_in'=>7200];}else{$data=['openid'=>'id-fixed','unionid'=>'union-id','nickname'=>'读者','headimgurl'=>'https://images.test/weixin'];}return new Reply($data);}}
 class Reply {function __construct(private $data){}function getBody(){return json_encode($this->data);}}
}
namespace {
$dir=$argv[1];$laravel=$argv[2];$hashes=json_decode(file_get_contents(__DIR__.'/china-source-sha256.json'),true,512,JSON_THROW_ON_ERROR);
foreach($hashes as $file=>$sha){if(!str_starts_with($file,'laravel/'))continue;$path=$laravel.'/'.substr($file,8);if(hash_file('sha256',$path)!==$sha){throw new \RuntimeException('Unexpected reference source');}require $path;}
}
namespace SocialiteProviders\Manager\OAuth2 {
 abstract class AbstractProvider extends \Laravel\Socialite\Two\AbstractProvider {protected $credentialsResponseBody;function getConfig($key,$default=null){return $default;}}
 class User extends \Laravel\Socialite\Two\User {}
}
namespace {
foreach($hashes as $file=>$sha){if(str_starts_with($file,'laravel/'))continue;$path=$dir.'/'.$file;if(hash_file('sha256',$path)!==$sha){throw new \RuntimeException('Unexpected reference source');}require $path;}
trait Expose {function inspect(){ $this->setHttpClient(new \GuzzleHttp\Client());$fields=$this->getCodeFields('fixed-state');$auth=$this->getAuthUrl('fixed-state');$tokens=$this->getTokenFields('fixed-code');$token=$this->getAccessTokenResponse('fixed-code');if(method_exists($this,'withUnionId')){$this->withUnionId();}$raw=$this->getUserByToken('private-token');$u=$this->mapUserToObject($raw);return ['fields'=>$fields,'auth'=>$auth,'token_fields'=>$tokens,'user'=>['id'=>$u->getId(),'nickname'=>$u->getNickname(),'name'=>$u->getName(),'email'=>$u->getEmail(),'avatar'=>$u->getAvatar(),'raw'=>$u->getRaw(),'attributes'=>$u->attributes],'calls'=>$this->getHttpClient()->calls];}}
class Wx extends \SocialiteProviders\Weixin\Provider {use Expose;}
class Web extends \SocialiteProviders\WeixinWeb\Provider {use Expose;}
class Qq extends \SocialiteProviders\QQ\Provider {use Expose;}
$out=[];foreach(['weixin'=>Wx::class,'weixinweb'=>Web::class,'qq'=>Qq::class] as $name=>$class){$out[$name]=(new $class(new \Illuminate\Http\Request(),'client','private-secret','https://rp.test/callback'))->inspect();}
echo json_encode($out,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR)."\n";
}
