<?php
ini_set('display_errors', 'stderr');
require rtrim($argv[2] ?? throw new RuntimeException('请传入固定 PHP 依赖的 vendor 目录'),'/').'/autoload.php';
$root=rtrim($argv[1],'/');
$maps=[];
foreach(glob($root.'/src/Illuminate/*/composer.json') as $file) {
 $component=json_decode(file_get_contents($file),true);
 foreach($component['autoload']['psr-4']??[] as $prefix=>$path) $maps[$prefix][]=dirname($file).'/'.$path;
}
uksort($maps,fn($a,$b)=>strlen($b)-strlen($a));
spl_autoload_register(function($class)use($maps){foreach($maps as $prefix=>$paths)if(str_starts_with($class,$prefix))foreach($paths as $path){
 $file=$path.str_replace('\\','/',substr($class,strlen($prefix))).'.php';
 if(is_file($file)){require $file;return;}
}});
require $root.'/src/Illuminate/Collections/functions.php';
require $root.'/src/Illuminate/Collections/helpers.php';
require $root.'/src/Illuminate/Support/helpers.php';
PHPUnit\TextUI\Configuration\Registry::init(
 (new PHPUnit\TextUI\CliArguments\Builder)->fromParameters([]),
 PHPUnit\TextUI\XmlConfiguration\DefaultConfiguration::create()
);
$cases=[];
$bodies=[
 '<p data-name="Ada">  Ada &amp; Bob </p><p>中文&nbsp;测试</p>',
 '<p>first</p>\n<p>second</p>',
 '<script>if (a < b) alert("<b>secret</b>")</script> visible',
 '<!-- comment --><style>.a {color:red}</style> text',
 '&lt;em&gt;A&lt;/em&gt; &apos; &copy; &#0; &#128; &#x1F600; &NotEqualTilde;',
 '0', '', "a\0b", 'a < b > c', 'a <broken',
 'a <?php echo "?>"; ?> b', '<b title="x>y">A</b>',
];
$needles=['Ada','Ada & Bob','中文 测试','first second','secret','visible','comment','text','<b>','<em>A</em>',"'",'©','😀','0','','missing'];
foreach($bodies as $body) foreach($needles as $needle) foreach([true,false] as $escape) foreach(['assertSee','assertDontSee','assertSeeText','assertDontSeeText'] as $method) {
 $response=new Illuminate\Testing\TestResponse(new Symfony\Component\HttpFoundation\Response($body));
 try{$response->$method($needle,$escape);$passes=true;}catch(PHPUnit\Framework\AssertionFailedError $error){$passes=false;}
 $cases[]=['body'=>$body,'method'=>$method,'arguments'=>[$needle,$escape],'passes'=>$passes];
}
foreach($bodies as $body) foreach([['Ada','Bob'],['Bob','Ada'],['first','second'],['second','first'],['0','0'],['',''],['<b>','secret'],['©','😀'],['missing'],[]] as $needles) foreach([true,false] as $escape) foreach(['assertSeeInOrder','assertSeeHtmlInOrder','assertSeeTextInOrder'] as $method) {
 $response=new Illuminate\Testing\TestResponse(new Symfony\Component\HttpFoundation\Response($body));
 $arguments=$method==='assertSeeHtmlInOrder'?[$needles]:[$needles,$escape];
 try{$response->$method(...$arguments);$passes=true;}catch(PHPUnit\Framework\AssertionFailedError $error){$passes=false;}
 $cases[]=['body'=>$body,'method'=>$method,'arguments'=>$arguments,'passes'=>$passes];
}
$entities=[];
foreach(array_values(get_html_translation_table(HTML_ENTITIES,ENT_QUOTES|ENT_HTML401,'UTF-8')) as $entity) $entities[$entity]=html_entity_decode($entity,ENT_QUOTES,'UTF-8');
foreach(array_merge(range(0,255),[0xD800,0xDFFF,0xFFFE,0xFFFF,0x1FFFE,0x10FFFF,0x110000]) as $code) foreach(['&#'.$code.';','&#x'.dechex($code).';'] as $entity) $entities[$entity]=html_entity_decode($entity,ENT_QUOTES,'UTF-8');
foreach($entities as $entity=>$decoded) {
 $needle=$decoded===$entity?substr($entity,0,-1):$decoded;
 $response=new Illuminate\Testing\TestResponse(new Symfony\Component\HttpFoundation\Response($entity));
 try{$response->assertSeeInOrder([$needle],false);$passes=true;}catch(PHPUnit\Framework\AssertionFailedError $error){$passes=false;}
 $cases[]=['body'=>$entity,'method'=>'assertSeeInOrder','arguments'=>[[$needle],false],'passes'=>$passes];
}
$strippedAllowed=[];
foreach($bodies as $body)foreach([[],['b'],['p','script'],['em'],['B','style']] as $allowed)$strippedAllowed[]=['body'=>$body,'allowed'=>$allowed,'stripped'=>strip_tags($body,$allowed)];
$strip=[];foreach($bodies as $body)$strip[$body]=strip_tags($body);
$jsonCases=[];
$documents=[[],(object)[],['id'=>1,'name'=>'Ada','active'=>true,'nil'=>null],['id'=>'1','name'=>'Bob'],['data'=>[['id'=>1,'name'=>'Ada'],['id'=>2,'name'=>'Bob']],'meta'=>['count'=>2]],['a'=>['x'=>1,'y'=>2],'b'=>['z'=>3]],['items'=>[3,1,2],'other'=>['a'=>1,'b'=>2]],['a.b'=>'literal','a'=>['b'=>'nested']],['zero'=>0,'false'=>false,'empty'=>[],'object'=>(object)[]],['values'=>[null,false,true,0,1,1.0,'0','01','1','',[],(object)[],['a'=>1]]],['id'=>1.0,'values'=>[1.0,1,1e-8,1e20]],['01'=>'a','1'=>'b','0'=>'c']];
$probes=[[],['id'=>1],['id'=>'1'],['id'=>2],['name'=>'Ada'],['nil'=>null],['missing'=>null],['id'=>1,'name'=>'Bob'],['data'=>[['id'=>1]]],['items'=>[1,2,3]],['items'=>[3]],['a'=>['x'=>1]],['a'=>[]],['a'=>null],['a.b'=>'literal'],['zero'=>false],['id'=>1.0],['values'=>[1,1.0]],['values'=>[1.0,1]],['values'=>[1e-8,1e20]],[1],[3]];
$addJson=function($body,$method,$arguments)use(&$jsonCases){
 $response=new Illuminate\Testing\TestResponse(new Symfony\Component\HttpFoundation\Response(json_encode($body,JSON_PRESERVE_ZERO_FRACTION)));
 try{$response->$method(...$arguments);$passes=true;$failure=null;}catch(PHPUnit\Framework\AssertionFailedError $error){$passes=false;$failure=$error::class;}catch(TypeError $error){$passes=false;$failure=$error::class;}
 $jsonCases[]=['body'=>$body,'method'=>$method,'arguments'=>$arguments,'passes'=>$passes,'failure'=>$failure];
};
foreach($documents as $document){
 foreach($probes as $probe){
  foreach([false,true] as $strict)$addJson($document,'assertJson',[$probe,$strict]);
  foreach(['assertJsonFragment','assertJsonMissingExact','assertSimilarJson','assertExactJson'] as $method)$addJson($document,$method,[$probe]);
  foreach([false,true] as $exact)$addJson($document,'assertJsonMissing',[$probe,$exact]);
 }
 foreach(['id','missing','nil','data.*.id','data.*.missing','a.b','a.*','items','items.*','empty','object','data.{first}.name','values'] as $path){
  foreach([null,1,'1',false,[1,2],['Ada','Bob']] as $expect)$addJson($document,'assertJsonPath',[$path,$expect]);
  $addJson($document,'assertJsonMissingPath',[$path]);
  foreach(['assertJsonIsArray','assertJsonIsObject'] as $method)$addJson($document,$method,[$path]);
  $value=data_get(json_decode(json_encode($document),true),$path);
  if(is_array($value)){
   foreach([0,1,2,3] as $count)$addJson($document,'assertJsonCount',[$count,$path]);
   foreach([[],[1,2],[2,1],[3,1,2],['1','2'],['x'=>1,'y'=>2],['01','1'],[1,'01'],[null,false],[[1,2],[2,1]]] as $expect)$addJson($document,'assertJsonPathCanonicalizing',[$path,$expect]);
  }
 }
 foreach([['id'=>1],['missing'=>null],['data.*.id'=>[1,2]]] as $paths) $addJson($document,'assertJsonPaths',[$paths]);
 foreach([['items'=>[1,2,3]],['data.*.id'=>[2,1]]] as $paths) $addJson($document,'assertJsonPathsCanonicalizing',[$paths]);
 $addJson($document,'assertJsonFragments',[[['id'=>1],['name'=>'Ada']]]);
 $addJson($document,'assertJsonMissingPaths',[['id','missing']]);
 foreach([0,1,2,3,4] as $count)$addJson($document,'assertJsonCount',[$count]);
 foreach([[],['id'],['id','name'],['missing'],['data'=>['*'=>['id']]],['a'=>['x']],['*'=>['id']],['empty'=>[]],['nil'=>[]],['a'=>[]]] as $shape)
  foreach(['assertJsonStructure','assertExactJsonStructure'] as $method)$addJson($document,$method,[$shape]);
 foreach(['assertJsonIsArray','assertJsonIsObject'] as $method)$addJson($document,$method,[]);
}
require $root.'/src/Illuminate/Foundation/helpers.php';
$sessionCases=[];
$sessions=[[],['name'=>'Ada','nil'=>null,'number'=>1],['_old_input'=>['name'=>'Ada','nil'=>null,'zero'=>0,'user'=>['email'=>'a@example.test']]],['errors'=>null],['errors'=>['default'=>[]]],['errors'=>['default'=>['name'=>['The name is required.','Wrong name.'],'email'=>['Invalid email.']],'login'=>['password'=>['Wrong password.']]]],['errors'=>['login'=>['name'=>['Login name required.']]]]];
$addSession=function($snapshot,$body,$contentType,$method,$arguments)use(&$sessionCases){
 $session=new Illuminate\Session\Store('reference',new Illuminate\Session\ArraySessionHandler(120));
 $session->start();
 foreach($snapshot as $key=>$value){
  if($key==='errors' && is_array($value)){
   $bags=new Illuminate\Support\ViewErrorBag;
   foreach($value as $bag=>$messages)$bags->put($bag,new Illuminate\Support\MessageBag($messages));
   $value=$bags;
  }
  $session->put($key,$value);
 }
 Illuminate\Container\Container::getInstance()->instance('session.store',$session);
 $response=new Illuminate\Testing\TestResponse(new Symfony\Component\HttpFoundation\Response($body,200,['content-type'=>$contentType]));
 try{$response->$method(...$arguments);$passes=true;$failure=null;}catch(PHPUnit\Framework\AssertionFailedError $error){$passes=false;$failure=$error::class;}catch(TypeError $error){$passes=false;$failure=$error::class;}
 $sessionCases[]=['session'=>(object)$snapshot,'body'=>$body,'content_type'=>$contentType,'method'=>$method,'arguments'=>$arguments,'passes'=>$passes,'failure'=>$failure];
};
foreach($sessions as $snapshot){
 foreach(['name','nil','missing','number','_old_input','errors','user.email'] as $key){
  foreach([null,'Ada',1,'1',false] as $expect)foreach(['assertSessionHas','assertSessionMissing','assertSessionHasInput'] as $method)$addSession($snapshot,'ok','text/html',$method,[$key,$expect]);
  $addSession($snapshot,'ok','text/html','assertSessionMissingInput',[$key]);
 }
 foreach([['name'],['name'=>'Ada'],['nil'=>null],['missing'=>null],['number'=>'1']] as $bindings)$addSession($snapshot,'ok','text/html','assertSessionHasAll',[$bindings]);
 $addSession($snapshot,'ok','text/html','assertSessionHasNoErrors',[]);
 foreach(['default','login','missing'] as $bag){
  foreach([[],['name'],['other'],['name'=>'required'],['name'=>'The name is required.'],['name'=>['required','name']],['name'=>null]] as $errors){
   foreach(['assertInvalid','assertOnlyInvalid'] as $method)$addSession($snapshot,'ok','text/html',$method,[$errors,$bag]);
   foreach([null,'<li>:message</li>'] as $format)foreach(['assertSessionHasErrors','assertSessionDoesntHaveErrors'] as $method)$addSession($snapshot,'ok','text/html',$method,[$errors,$format,$bag]);
  }
  foreach([null,[],['name'],['other']] as $keys)$addSession($snapshot,'ok','text/html','assertValid',[$keys,$bag]);
 }
}
foreach([[],['errors'=>[]],['errors'=>['name'=>['The name is required.'],'email'=>['Invalid email.']]],['data'=>['errors'=>['name'=>['Invalid name.']]]]] as $json){
 foreach(['errors','data.errors'] as $responseKey){
  foreach([null,[],['name'],['email'],['other'],['name'=>'required'],['name'=>['required','name']],['name'=>[]]] as $errors){
   foreach(['assertInvalid','assertOnlyInvalid','assertJsonValidationErrors','assertOnlyJsonValidationErrors'] as $method){
    $args=in_array($method,['assertInvalid','assertOnlyInvalid'])?[$errors,'default',$responseKey]:[$errors,$responseKey];
    $addSession([],json_encode($json),'application/json',$method,$args);
   }
  }
  foreach([null,[],['name'],['other']] as $keys){
   $addSession([],json_encode($json),'application/json','assertValid',[$keys,'default',$responseKey]);
   $addSession([],json_encode($json),'application/json','assertJsonMissingValidationErrors',[$keys,$responseKey]);
  }
  foreach(['name','other'] as $key)$addSession([],json_encode($json),'application/json','assertJsonValidationErrorFor',[$key,$responseKey]);
 }
}
$encodingCases=[];
foreach([null,false,true,0,1,1.0,-0.0,1.5,1e-8,1e-6,1e16,1e17,1e20,1e21,1.7976931348623157e308,"中文/路径😀",'<tag a="x">&quoted\''] as $value){
 $document=['value'=>$value];
 $default=(new Illuminate\Http\JsonResponse($document))->getContent();
 ob_start();(new Symfony\Component\HttpFoundation\StreamedJsonResponse($document,200,[],15))->sendContent();$stream=ob_get_clean();
 try{(new Illuminate\Testing\TestResponse(new Symfony\Component\HttpFoundation\StreamedJsonResponse($document,200,[],15)))->assertStreamedJsonContent($document);$passes=true;}catch(PHPUnit\Framework\AssertionFailedError $error){$passes=false;}
 $encodingCases[]=['document'=>$document,'default'=>$default,'stream'=>$stream,'assert_streamed_json'=>$passes];
}
echo json_encode(['source'=>'laravel/framework@1449bf241017087fbd38c118ecaabef3f6797fdc','php'=>PHP_VERSION,'phpunit'=>Composer\InstalledVersions::getPrettyVersion('phpunit/phpunit'),'http_foundation'=>Composer\InstalledVersions::getPrettyVersion('symfony/http-foundation'),'cases'=>$cases,'json_cases'=>$jsonCases,'session_cases'=>$sessionCases,'encoding_cases'=>$encodingCases,'entities'=>(object)$entities,'stripped'=>(object)$strip,'stripped_allowed'=>$strippedAllowed],JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES|JSON_PRESERVE_ZERO_FRACTION);
