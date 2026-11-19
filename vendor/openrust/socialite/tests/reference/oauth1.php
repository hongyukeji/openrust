<?php
namespace Illuminate\Http {
class RedirectResponse {function __construct(public $url){}}
class Session {public $data=[];function put($k,$v){$this->data[$k]=$v;}function get($k){return $this->data[$k]??null;}}
class Request {public $values=[];public $store;function __construct(){$this->store=new Session();}function has($keys){foreach($keys as $k)if(!array_key_exists($k,$this->values))return false;return true;}function input($k){return $this->values[$k]??null;}function session(){return $this->store;}}
}
namespace GuzzleHttp {
class Client {public $calls=[];public $temporary='oauth_token=temp&oauth_token_secret=temp-secret&oauth_callback_confirmed=true';public $token='oauth_token=access&oauth_token_secret=access-secret';public $raw;function post($url,$options){$this->calls[]=[$url,$options];return new Reply(str_contains($url,'request_token')?$this->temporary:$this->token);}function get($url,$options){$this->calls[]=[$url,$options];return new Reply(json_encode($this->raw));}}
class Reply {function __construct(private $body){}function getBody(){return $this->body;}}
}
namespace {
$stage=$argv[1];$roots=['socialite'=>$stage.'/upstream','league'=>$stage.'/league','psr7'=>$stage.'/guzzlehttp-psr7','psr'=>$stage.'/psr-http-message'];
foreach(json_decode(file_get_contents(__DIR__.'/oauth1-source-sha256.json'),true) as $file=>$hash){[$prefix,$relative]=explode('/',$file,2);if(hash_file('sha256',$roots[$prefix].'/'.$relative)!==$hash)throw new \RuntimeException('Unexpected OAuth1 reference source');}
spl_autoload_register(function($class)use($roots){foreach(['Laravel\\Socialite\\'=>['socialite','src/'],'League\\OAuth1\\Client\\'=>['league','src/'],'GuzzleHttp\\Psr7\\'=>['psr7','src/'],'Psr\\Http\\Message\\'=>['psr','src/']] as $prefix=>[$root,$path])if(str_starts_with($class,$prefix))require $roots[$root].'/'.$path.str_replace('\\','/',substr($class,strlen($prefix))).'.php';});
class Server extends \League\OAuth1\Client\Server\Twitter {public $http;function createHttpClient(){return $this->http;}protected function baseProtocolParameters(){return ['oauth_consumer_key'=>'client','oauth_nonce'=>'fixed-nonce','oauth_signature_method'=>'HMAC-SHA1','oauth_timestamp'=>'1700000000','oauth_version'=>'1.0'];}}
function make(){ $r=new \Illuminate\Http\Request();$s=new Server(['identifier'=>'client','secret'=>'consumer + %25','callback_uri'=>'https://rp.test/callback']);$s->http=new \GuzzleHttp\Client();$s->http->raw=['id'=>42,'id_str'=>'42','screen_name'=>'alice','name'=>'Alice','email'=>'alice@test','location'=>'上海','description'=>'Hello','profile_image_url'=>'https://image.test/alice_normal.jpg','url'=>'https://alice.test','other_url'=>'https://else.test','verified'=>true];return [$r,$s,new \Laravel\Socialite\One\TwitterProvider($r,$s)];}
function user($u){return ['id'=>$u->getId(),'nickname'=>$u->getNickname(),'name'=>$u->getName(),'email'=>$u->getEmail(),'avatar'=>$u->getAvatar(),'raw'=>$u->getRaw(),'attributes'=>$u->attributes];}
$out=['signatures'=>[]];
foreach([
 ['POST','https://api.twitter.com/oauth/request_token',['oauth_callback'=>'https://rp.test/callback?a=1&b=中文']],
 ['GET','https://example.test:443/resource?include_email=true',[]],
 ['GET','http://EXAMPLE.test:8080/resource?x=a+b&x=last&user.name=alice',[]],
 ['POST','https://example.test/resource?z=%252B&plus=%2B&tilde=~',['oauth_verifier'=>'a+b %25 / 中文']],
 ['GET','https://example.test/resource?a[second]=2&a[first]=1&b[]=x&b[]=y',[]],
 ['GET','https://example.test/resource?q=from-url',['q'=>'override','%61'=>'encoded key']],
 ['POST','https://example.test/resource?empty=&zero=0',['oauth_token'=>'a','oauth_verifier'=>'']],
 ] as [$method,$url,$parameters]){foreach(['','token + %25'] as $secret){$client=new \League\OAuth1\Client\Credentials\ClientCredentials();$client->setIdentifier('client');$client->setSecret('consumer + %25');$sign=new \League\OAuth1\Client\Signature\HmacSha1Signature($client);$token=new \League\OAuth1\Client\Credentials\TokenCredentials();$token->setSecret($secret);$sign->setCredentials($token);$out['signatures'][]=['method'=>$method,'url'=>$url,'parameters'=>(object)$parameters,'secret'=>$secret,'signature'=>$sign->sign($url,$parameters,$method)];}}
[$r,$s,$p]=make();$out['raw']=$s->http->raw;$p->scope('write + 中文');$redirect=$p->redirect();$out['redirect']=$redirect->url;$out['redirect_http']=$s->http->calls;$r->values=['oauth_token'=>'temp','oauth_verifier'=>'verified + 中文'];$out['callback']=user($p->user());$out['temporary_retained']=$r->session()->get('oauth.temp')!==null;
$out['direct']=user($p->userFromTokenAndSecret('access','access-secret'));$out['cached_calls']=count($s->http->calls);$p->userFromTokenAndSecret('access','changed-secret');$out['changed_secret_calls']=count($s->http->calls);$out['http']=$s->http->calls;
foreach(['missing-verifier','missing-temp','wrong-token','null-token','empty-verifier','zero-token','null-verifier','false-verifier','true-verifier','numeric-verifier'] as $case){[$r,$s,$p]=make();$r->values=['oauth_token'=>'temp','oauth_verifier'=>'v'];if($case!=='missing-temp')$p->redirect();if($case==='missing-verifier')unset($r->values['oauth_verifier']);if($case==='wrong-token')$r->values['oauth_token']='bad';if($case==='null-token')$r->values['oauth_token']=null;if($case==='empty-verifier')$r->values['oauth_verifier']='';if($case==='zero-token')$r->values['oauth_token']='0';foreach(['null-verifier'=>null,'false-verifier'=>false,'true-verifier'=>true,'numeric-verifier'=>0] as $key=>$v)if($case===$key)$r->values['oauth_verifier']=$v;try{$p->user();$error=null;}catch(\Throwable $e){$error=get_class($e);}$out['callbacks'][$case]=['error'=>$error,'calls'=>count($s->http->calls)];}
foreach(['oauth_token=t&oauth_token_secret=s&oauth_callback_confirmed=true','oauth_token=t&oauth_token_secret=s&oauth_callback_confirmed=false','oauth_token=t&oauth_token_secret=s','error=denied'] as $body){[$r,$s,$p]=make();$s->http->temporary=$body;try{$p->redirect();$accepted=true;}catch(\Throwable $e){$accepted=false;}$out['temporary'][]=['body'=>$body,'accepted'=>$accepted];}
echo json_encode($out,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR)."\n";
}
