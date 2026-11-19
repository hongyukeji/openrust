<?php
namespace Illuminate\Support {class Arr {static function get($a,$k,$default=null){if(is_array($a)&&array_key_exists($k,$a))return $a[$k];foreach(explode('.',$k) as $p){if(!is_array($a)||!array_key_exists($p,$a))return $default;$a=$a[$p];}return $a;}}}
namespace Illuminate\Http {class Request {}}
namespace GuzzleHttp {class RequestOptions {const HEADERS='headers';const FORM_PARAMS='form_params';const QUERY='query';}class Reply {function __construct(private $v){}function getBody(){return json_encode($this->v);}}class Client {public $calls=[];public $keys;public $mode='ok';function get($url,$opts=[]){$this->calls[]=[$url,$opts];if(str_ends_with($url,'/certs'))return new Reply($this->keys);return new Reply(['sub'=>'user-42','name'=>'Alice','email'=>'alice@test','email_verified'=>true,'picture'=>'https://images.test/alice']);}function post($url,$opts){$this->calls[]=[$url,$opts];return new Reply(['access_token'=>'access','expires_in'=>3600,'scope'=>'openid profile email']);}}}
namespace {
$root=$argv[1];$jwt=$argv[2];$manifest=json_decode(file_get_contents(__DIR__.'/google-source-sha256.json'),true,512,JSON_THROW_ON_ERROR);
foreach($manifest as $name=>$hash){$file=str_starts_with($name,'jwt/')?$jwt.'/'.substr($name,4):$root.'/'.$name;if(hash_file('sha256',$file)!==$hash)throw new \RuntimeException('Unexpected reference source');}
spl_autoload_register(function($class)use($jwt){if(str_starts_with($class,'Firebase\\JWT\\'))require $jwt.'/src/'.substr($class,13).'.php';});
foreach(array_keys($manifest) as $file){if(!str_starts_with($file,'jwt/'))require $root.'/'.$file;}
class Probe extends \Laravel\Socialite\Two\GoogleProvider {function mapping($v){return $this->mapUserToObject($v);}function fetch($token){return $this->getUserByToken($token);}function fields(){return $this->getCodeFields('fixed-state');}}
$private=file_get_contents(__DIR__.'/../fixtures/google-private.pem');$second=file_get_contents(__DIR__.'/../fixtures/google-second-private.pem');
function jwk($private){$details=openssl_pkey_get_details(openssl_pkey_get_private($private));return ['kty'=>'RSA','kid'=>'test-key','alg'=>'RS256','n'=>rtrim(strtr(base64_encode($details['rsa']['n']),'+/','-_'),'='),'e'=>rtrim(strtr(base64_encode($details['rsa']['e']),'+/','-_'),'=')];}
$key=jwk($private);$key2=jwk($second);$keys=['keys'=>[$key]];$base=['sub'=>'user-42','iss'=>'https://accounts.google.com','aud'=>'client','exp'=>4000000000,'name'=>'Alice','email'=>'alice@test','email_verified'=>true,'picture'=>'https://images.test/alice'];
$p=new Probe(new \Illuminate\Http\Request(),'client','secret','https://rp.test/callback');$http=new \GuzzleHttp\Client();$http->keys=$keys;$p->setHttpClient($http);\Firebase\JWT\JWT::$timestamp=2000000000;
$out=['keys'=>$keys,'rotated_keys'=>['keys'=>[$key2]],'default_fields'=>$p->fields()];$cases=[];
foreach(['valid'=>[], 'no-exp'=>['exp'=>null], 'numeric-dates'=>['exp'=>'4000000000','iat'=>'1','nbf'=>'1'], 'null-dates'=>['exp'=>null,'iat'=>null,'nbf'=>null], 'nbf-overrides-iat'=>['nbf'=>1,'iat'=>4000000000], 'issuer'=>['iss'=>'https://bad.test'], 'audience'=>['aud'=>'other'], 'audience-array'=>['aud'=>['client']], 'no-issuer'=>['iss'=>null], 'no-audience'=>['aud'=>null], 'expired'=>['exp'=>1], 'future-nbf'=>['nbf'=>4000000000], 'future-iat'=>['iat'=>4000000000]] as $name=>$change){$claims=array_merge($base,$change);if($name==='no-exp')unset($claims['exp']);$token=\Firebase\JWT\JWT::encode($claims,$private,'RS256','test-key');$cases[$name]=[$token,$keys];}
$cases['rotation']=[\Firebase\JWT\JWT::encode($base,$second,'RS256','test-key'),['keys'=>[$key2]]];
$token=\Firebase\JWT\JWT::encode($base,$private,'RS256','test-key');$parts=explode('.',$token);$sig=base64_decode(strtr($parts[2],'-_','+/'));$sig[0]=chr(ord($sig[0])^1);$parts[2]=rtrim(strtr(base64_encode($sig),'+/','-_'),'=');$cases['signature']=[implode('.',$parts),$keys];
$cases['unknown-kid']=[\Firebase\JWT\JWT::encode($base,$private,'RS256','missing'),$keys];$cases['missing-kid']=[\Firebase\JWT\JWT::encode($base,$private,'RS256'),$keys];
$cases['hmac-confusion']=[\Firebase\JWT\JWT::encode($base,openssl_pkey_get_details(openssl_pkey_get_private($private))['key'],'HS256','test-key'),$keys];
foreach(['exp','nbf','iat'] as $field){$parts=explode('.',$token);$parts[1]=rtrim(strtr(base64_encode(json_encode(array_merge($base,[$field=>'bad']))),'+/','-_'),'=');openssl_sign($parts[0].'.'.$parts[1],$signature,$private,OPENSSL_ALGO_SHA256);$parts[2]=rtrim(strtr(base64_encode($signature),'+/','-_'),'=');$cases['malformed-'.$field]=[implode('.',$parts),$keys];}
$mismatch=$key;$mismatch['alg']='RS512';$cases['key-algorithm']=[$token,['keys'=>[$mismatch]]];
$missingAlg=$key;unset($missingAlg['alg']);$cases['missing-key-algorithm']=[$token,['keys'=>[$missingAlg]]];
$cases['empty-jwks']=[$token,['keys'=>[]]];
$invalidKey=$key;$invalidKey['n']='bad';$cases['malformed-key']=[$token,['keys'=>[$invalidKey]]];
foreach($cases as $name=>[$token,$keys]){$http->keys=$keys;try{$raw=$p->fetch($token);$ok=true;}catch(\Throwable $e){$ok=false;}$out['jwt'][$name]=['token'=>$token,'keys'=>$keys,'accepted'=>$ok];}
$http->keys=['keys'=>[$key]];$raw=$p->fetch('ordinary-access');$u=$p->mapping($raw);$out['user']=['id'=>$u->getId(),'nickname'=>$u->getNickname(),'name'=>$u->getName(),'email'=>$u->getEmail(),'avatar'=>$u->getAvatar(),'raw'=>$u->getRaw(),'attributes'=>$u->attributes];$token=$p->refreshToken('keep-refresh');$out['refresh']=['token'=>$token->token,'refreshToken'=>$token->refreshToken,'expiresIn'=>$token->expiresIn,'approvedScopes'=>$token->approvedScopes];$out['http']=array_slice($http->calls,-2);
echo json_encode($out,JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR)."\n";
}
