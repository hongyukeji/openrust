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
$loader=new Illuminate\Translation\ArrayLoader;
$loader->addMessages('en','validation',require $root.'/src/Illuminate/Translation/lang/en/validation.php');
$translator=new Illuminate\Translation\Translator($loader,'en');
$values=[null,'','  ',false,true,0,1,0.0,1.0,'0','1','no','yes','off','false','text',[],[1],['a'=>1]];
$cases=[];
foreach(['missing','filled','declined','prohibited','exclude','same:other','different:other,third'] as $rule) {
 foreach(['','nullable|','sometimes|','required|'] as $prefix) {
  foreach(array_merge([['other'=>'text']],array_map(fn($v)=>['value'=>$v,'other'=>'text','third'=>0],$values)) as $data) {
   $rules=['value'=>$prefix.$rule];
   $v=new Illuminate\Validation\Validator($translator,$data,$rules);
   $passes=$v->passes();
   $cases[]=['data'=>(object)$data,'rules'=>(object)$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
  }
 }
}
foreach(['same:items.*.other','different:items.*.other'] as $rule) {
 $data=['items'=>[['value'=>1,'other'=>'1'],['value'=>'yes','other'=>'yes'],['value'=>null,'other'=>null]]];
 $rules=['items.*.value'=>$rule];$v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
 $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
}
$conditional=[
 'accepted_if:other,yes', 'declined_if:other,yes', 'missing_if:other,yes', 'missing_unless:other,yes',
 'missing_with:other,third', 'missing_with_all:other,third', 'present_if:other,yes', 'present_unless:other,yes',
 'present_with:other,third', 'present_with_all:other,third', 'required_if_accepted:other', 'required_if_declined:other',
 'required_unless:other,yes', 'required_with:other,third', 'required_with_all:other,third',
 'required_without:other,third', 'required_without_all:other,third', 'prohibited_if:other,yes',
 'prohibited_if_accepted:other', 'prohibited_if_declined:other', 'prohibited_unless:other,yes',
 'prohibits:other,third', 'exclude_unless:other,yes', 'exclude_with:other,third', 'exclude_without:other,third',
 'missing_if:other,null', 'present_unless:other,null', 'required_with_all', 'required_without_all', 'prohibits',
];
foreach($conditional as $rule) {
 foreach(['','nullable|','sometimes|','required|'] as $prefix) {
  $inputs=[[],['value'=>null],['value'=>''],['value'=>false],['value'=>0],['value'=>'text'],['value'=>[]]];
  $others=[[],['other'=>null],['other'=>true],['other'=>false],['other'=>'yes'],['other'=>'no'],['other'=>'text','third'=>null],['other'=>'','third'=>'x'],['other'=>0,'third'=>false]];
  foreach($inputs as $input) foreach($others as $other) {
   $data=array_merge($input,$other);$rules=['value'=>$prefix.$rule];
   $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
   $cases[]=['data'=>(object)$data,'rules'=>(object)$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
  }
 }
}
foreach(['required_if:other,true','required_if:other,false','exclude_if:other,true','exclude_if:other,false',
 'missing_if:other,true','present_if:other,false','required_unless:other,NULL','missing_unless:other,NULL'] as $rule) {
 foreach(['boolean','boolean:strict'] as $boolean) foreach([null,false,true,0,1,'0','1','false','true'] as $other) foreach([[],['value'=>null],['value'=>'text']] as $input) {
  $data=array_merge(['other'=>$other],$input);$rules=['other'=>$boolean,'value'=>$rule];
  $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>(object)$data,'rules'=>(object)$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
}
foreach($conditional as $rule) {
 $rule=str_replace(['other','third'],['items.*.other','items.*.third'],$rule);
 $data=['items'=>[['value'=>null,'other'=>true],['value'=>'text','other'=>'yes','third'=>null],['value'=>false,'other'=>null],['other'=>'no','third'=>'x']]];
 $rules=['items.*.other'=>'boolean','items.*.value'=>$rule];
 $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
 $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
}
$arrayRules=['array_keys:a,b','array_keys:0,1','required_array_keys:a,b','required_array_keys:0,1',
 'required_array_keys','in_array_keys:a,b','in_array_keys:0','in_array_keys',
 'contains:0,false','contains:a,b','contains','doesnt_contain:0,false','doesnt_contain:a,b','doesnt_contain',
 'not_in:0,1,a','array|not_in:0,1,a','array|in:0,1,a','not_in','array|not_in','in'];
$arrayValues=[null,'','a','other',false,true,0,1,0.0,[],['a','b'],[0,1],[false,true],['0','false'],['a'=>null,'b'=>false],['a'=>0],['0'=>1,'1'=>2],['01'=>1],[[0]]];
foreach($arrayRules as $rule) foreach(['','nullable|','required|'] as $prefix) {
 foreach(array_merge([[]],array_map(fn($value)=>['value'=>$value],$arrayValues)) as $data) {
  $rules=['value'=>$prefix.$rule];$v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>(object)$data,'rules'=>(object)$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
}
$customCases=[
 [['value'=>['three'=>1,'four'=>2]],['value'=>'array_keys:one,two'],':attribute :ATTRIBUTE :Attribute | :values :VALUES :Values | :unexpected :UNEXPECTED :Unexpected'],
 [['value'=>[]],['value'=>'required_array_keys:one,two'],':values :VALUES :Values'],
 [['value'=>['one']],['value'=>'doesnt_contain:one,two'],':values :VALUES :Values'],
 [['value'=>'three'],['value'=>'starts_with:one,two'],':values :VALUES :Values'],
 [['value'=>'one'],['value'=>'not_in:one,two'],':values :VALUES :Values'],
 [['other'=>'one','third'=>'two'],['value'=>'required_with:other,third'],':values :VALUES :Values'],
 [['items'=>[['value'=>1,'other'=>2]]],['items.*.value'=>'same:items.*.other'],':other :OTHER :Other'],
 [['items'=>[['other'=>'one']]],['items.*.value'=>'required_if:items.*.other,one'],':other :OTHER :Other :value :VALUE :Value'],
 [['items'=>[['other'=>'one']]],['items.*.value'=>'present_if:items.*.other,one'],':other :OTHER :Other :value :VALUE :Value'],
];
foreach($customCases as [$data,$rules,$message]) {
 $messages=[];foreach($rules as $attribute=>$rule) $messages[explode(':',$rule,2)[0]]=$message;
 $attributes=['value'=>'test value','other'=>'second field','third'=>'third field','items.*.other'=>'other item'];
 $v=new Illuminate\Validation\Validator($translator,$data,$rules,$messages,$attributes);$passes=$v->passes();
 $cases[]=['data'=>$data,'rules'=>$rules,'messages'=>$messages,'attributes'=>$attributes,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
}
foreach(['distinct','distinct:strict','distinct:ignore_case','distinct:strict,ignore_case'] as $rule) {
 foreach([[1,'1',1.0],[0,false,null,'',[]],['Alpha','alpha','ALPHA'],['K','K','k'],['ß','SS','ss'],['a','a\n'],[[],[]],['Array',[]],[9007199254740992,9007199254740993],['9007199254740992','9007199254740993']] as $values) {
  $data=['values'=>$values];$rules=['values.*'=>$rule];$v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
}
foreach(['in_array:pool.*','in_array:pool.*.name','in_array:pool'] as $rule) foreach([null,false,true,0,1,1.0,'1','alpha',[],['alpha']] as $value) {
 $data=['value'=>$value,'pool'=>[1,'alpha',['name'=>'alpha']]];$rules=['value'=>$rule];
 $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
 $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
}
foreach(['date_format:Y-m-d','date_format:d/m/Y','date_format:Y-m-d,Y/m/d','date_format:Y','date_format:U','date_format:Y-m-d\\TH:i:sP','date_format:H:i:s.u'] as $rule) {
 foreach([null,false,1,1.0,'1','0001','2024-02-29','2023-02-29','2024-02-30','2024-2-29','2024/02/29','29/02/2024','2024-11-22T12:30:00+00:00','2024-11-22T12:30:00Z','12:30:00.123456','12:30:00.1','0','-1',''] as $value) {
  $data=['value'=>$value];$rules=['value'=>$rule];$v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
}
if (class_exists(Brick\Math\BigDecimal::class)) {
 foreach(['multiple_of:3','multiple_of:0','multiple_of:0.1','multiple_of:bad','multiple_of:-0.5'] as $rule) {
  foreach([null,false,true,0,0.0,1,3,6,-6,0.3,'0.3','0.30000000000000000001','9007199254740993',' 6 ','3e2','wrong','',[]] as $value) {
   $data=['value'=>$value];$rules=['value'=>$rule];$v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
   $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
  }
 }
}
if (class_exists(Symfony\Component\Uid\Ulid::class)) {
 foreach(['01ARZ3NDEKTSV4RRFFQ69G5FAV','01arz3ndektsv4rrffq69g5fav','7ZZZZZZZZZZZZZZZZZZZZZZZZZ','8ZZZZZZZZZZZZZZZZZZZZZZZZZ','01ARZ3NDEKTSV4RRFFQ69G5FAI','00000000000000000000000000','01ARZ3NDEKTSV4RRFFQ69G5FAV\n',0,false,null,[], ''] as $value) {
  $data=['value'=>$value];$rules=['value'=>'ulid'];$v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
}
date_default_timezone_set('UTC');
$container=new Illuminate\Container\Container;
$container->instance('date',new Illuminate\Support\DateFactory);
Illuminate\Support\Facades\Facade::setFacadeApplication($container);
foreach(['before','before_or_equal','after','after_or_equal','date_equals'] as $name) {
 foreach(['2024-02-29','other','missing'] as $parameter) foreach(['2024-02-28','2024-02-29','2024-03-01','invalid',null,false,0,1.0,[], ''] as $value) {
  $data=['value'=>$value,'other'=>'2024-02-29'];$rules=['value'=>$name.':'.$parameter];
  $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
 foreach(['28/02/2024','29/02/2024','01/03/2024','invalid',null,false,0,[]] as $value) foreach(['29/02/2024',null] as $other) {
  $data=['value'=>$value,'other'=>$other];$rules=['value'=>'date_format:d/m/Y|'.$name.':other'];
  $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
}
foreach(['UTC','Asia/Shanghai','America/New_York'] as $timezone) {
 date_default_timezone_set($timezone);
 Carbon\Carbon::setTestNow(new DateTimeImmutable('2024-03-10T02:00:00Z'));
 foreach(['before:today','before_or_equal:today','after:today','date_equals:today','before:tomorrow'] as $rule) foreach(['2024-03-09 20:00:00 UTC','2024-03-10'] as $value) {
  $data=['value'=>$value];$rules=['value'=>$rule];$v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'clock'=>['timezone'=>$timezone,'at'=>'2024-03-10T02:00:00Z'],'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
}
Carbon\Carbon::setTestNow();date_default_timezone_set('UTC');
$sizeValues=[null,'','  ',false,true,0,1,-1,0.0,1.0,'0','1','12','100.01','1e3','abc','中文',[],[1,2],['a'=>1]];
foreach(['gt','lt','gte','lte'] as $name) {
 foreach(['','numeric|','array|'] as $prefix) foreach($sizeValues as $value) foreach($sizeValues as $other) {
  $data=['value'=>$value,'other'=>$other];$rules=['value'=>$prefix.$name.':other'];
  $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
 foreach(['0','1','-1','100.01','1e3','missing'] as $parameter) foreach($sizeValues as $value) {
  $data=['value'=>$value];$rules=['value'=>$name.':'.$parameter];
  $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
 foreach([['first'=>'1','second'=>'word'],['first'=>'word','second'=>'1']] as $data) {
  $rules=['first'=>$name.':missing|min:2','second'=>$name.':missing|min:2'];
  $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
 $data=['items'=>[['value'=>'4','other'=>3],['value'=>'abc','other'=>'ab'],['value'=>[1],'other'=>['x'=>1,'y'=>2]]]];$rules=['items.*.value'=>$name.':items.*.other'];
 $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
 $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
}
foreach(['','all','all_with_bc','per_country,cn','per_country,US','africa','america','antarctica','arctic','asia','atlantic','australia','europe','indian','pacific','utc'] as $group) {
 foreach(array_merge(timezone_identifiers_list(DateTimeZone::ALL_WITH_BC),[null,false,0,[],'asia/shanghai','Invalid/Zone','+08:00','']) as $value) {
  $data=['value'=>$value];$rules=['value'=>'timezone'.($group===''?'':':'.$group)];
  $v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
  $cases[]=['data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
 }
}
Illuminate\Validation\Validator::fakeDnsLookups();
$urls=[null,false,0,[], '', 'text','example.test','example.test:80','example.test:80/path','example.test:80?x=1','example.test:65536',
 'https://example.test','//example.test/path','https://user:pass@example.test:443/a?b#c','ftp://example.test','1://example.test',
 'https://127.0.0.1','https://127.1','https://[::1]','https://[::1]:443','https://localhost','https://例子.测试','https://xn--fsqu00a.xn--0zwm56d',
 'https://example.test.','https://example.test..','https://bad_host.test','https://-host.test','https://host-.test','https://a..b',
 'https://user@other@example.test','https://example.test:','https://example.test:+80','https://example.test:80x','https://example.test:99999',
 'https:///example.test','file:///example.test','file://example.test/path','mailto:user@example.test','https://example.test/invalid space',
 "https://exam\nple.test",' https://example.test','https:// example.test','https://example.test ','https://1','https://0',
 'http:/example.test',':80','//example.test:80','//example.test:80?x=1','//example.test:80#x','http://example.test:00080'];
foreach($urls as $value) foreach(['','nullable|','required|'] as $prefix) {
 $data=['value'=>$value];$rules=['value'=>$prefix.'active_url'];$v=new Illuminate\Validation\Validator($translator,$data,$rules);$passes=$v->passes();
 $cases[]=['data'=>$data,'rules'=>$rules,'dns_fake'=>true,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
}
Illuminate\Validation\Validator::flushState();
$mutations=[];
$scenarios=[
 [['value'=>20],['value'=>'integer|min:18'],[['passes',[]],['setValue',['value',16]],['passes',[]],['setValue',['value',30]],['passes',[]]]],
 [['users'=>[['age'=>20]]],['users.*.age'=>'integer|min:18'],[['getRules',[]],['addRules',[['users.*.age'=>'min:25','token'=>'required']]],['passes',[]],['setData',[['users'=>[['age'=>20],['age'=>30]]]]],['getRules',[]],['passes',[]]]],
 [['value'=>20],['value'=>'integer'],[['appendRules',[['value'=>'min:25']]],['passes',[]],['setData',[['value'=>21]]],['passes',[]],['setRules',[['value'=>'integer|min:18']]],['passes',[]]]],
 [['kind'=>'x','value'=>'bad'],['kind'=>'starts_with:a','value'=>'integer'],[['setCustomMessages',[['starts_with'=>':attribute :values']]],['setAttributeNames',[['kind'=>'种类']]],['setValueNames',[['kind'=>['a'=>'甲']]]],['passes',[]],['setCustomMessages',[['integer'=>'整数错误']]],['addCustomAttributes',[['value'=>'数值']]],['addCustomValues',[['kind'=>['a'=>'A']]]],['passes',[]],['setAttributeNames',[[]]],['setValueNames',[[]]],['passes',[]]]],
];
foreach($scenarios as [$data,$rules,$actions]) {
 $v=new Illuminate\Validation\Validator($translator,$data,$rules);$steps=[];
 foreach($actions as [$method,$arguments]) {
  $result=$v->$method(...$arguments);$step=['method'=>$method,'arguments'=>$arguments];
  if($method==='passes') $step+=['passes'=>$result,'errors'=>(object)$v->errors()->toArray(),'validated'=>$result?(object)$v->validated():null];
  if($method==='getRules') $step['counts']=array_map('count',$result);
  $steps[]=$step;
 }
 $mutations[]=['data'=>$data,'rules'=>$rules,'steps'=>$steps];
}
$extensionCases=[];
foreach(['extend','extendImplicit','extendDependent'] as $mode) {
 foreach([[],['value'=>null],['value'=>''],['value'=>'  '],['value'=>false],['value'=>0],['value'=>'pass'],['value'=>'fail'],['value'=>[]],['value'=>['x'=>1]]] as $data) {
  foreach(['custom_rule:other,token|integer','nullable|custom_rule:other,token','sometimes|custom_rule:other,token'] as $spec) {
   $factory=new Illuminate\Validation\Factory($translator);$calls=[];
   $factory->$mode('CustomRule',function($attribute,$value,$parameters,$validator)use(&$calls){$calls[]=['attribute'=>$attribute,'value'=>$value,'parameters'=>$parameters];return $value==='pass';},'Invalid :attribute (:token).');
   $factory->replacer('CustomRule',fn($message,$attribute,$rule,$parameters,$validator)=>str_replace(':token',implode('/',$parameters),$message));
   $rules=['value'=>$spec];$v=$factory->make($data,$rules);$passes=$v->passes();
   $extensionCases[]=['mode'=>$mode,'data'=>(object)$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'calls'=>$calls];
  }
 }
}
foreach(['extend','extendDependent'] as $mode) {
 $factory=new Illuminate\Validation\Factory($translator);$calls=[];
 $factory->$mode('custom_rule',function($attribute,$value,$parameters,$validator)use(&$calls){$calls[]=['attribute'=>$attribute,'value'=>$value,'parameters'=>$parameters];return false;},'Invalid :attribute (:token).');
 $factory->replacer('custom_rule',fn($message,$attribute,$rule,$parameters,$validator)=>str_replace(':token',implode('/',$parameters),$message));
 $data=['items'=>[['value'=>'a','other'=>'b'],['value'=>'c','other'=>'d']]];$rules=['items.*.value'=>'custom_rule:items.*.other,token'];
 $v=$factory->make($data,$rules);$passes=$v->passes();
 $extensionCases[]=['mode'=>$mode,'data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'calls'=>$calls];
}
$arrayPolicyCases=[];
$scenarios=[
 [['user'=>['name'=>'Ada','secret'=>'hidden']],['user'=>'array','user.name'=>'string']],
 [['user'=>['name'=>'Ada','secret'=>'hidden']],['user.name'=>'string','user'=>'array']],
 [['user'=>['name'=>'Ada','secret'=>'hidden']],['user'=>'nullable','user.name'=>'string']],
 [['user'=>['name'=>'Ada','secret'=>'hidden']],['user'=>'array:name,secret','user.name'=>'string']],
 [['users'=>[['name'=>'Ada','secret'=>'hidden'],['name'=>'Bob','secret'=>'hidden']]],['users'=>'list','users.*'=>'array','users.*.name'=>'string']],
 [['users'=>[['name'=>'Ada','secret'=>'hidden'],['name'=>'Bob','secret'=>'hidden']]],['users'=>'array','users.*.name'=>'string']],
 [['user'=>null],['user'=>'nullable|array','user.name'=>'sometimes|string']],
 [['user'=>[]],['user'=>'array','user.name'=>'sometimes|string']],
];
foreach([true,false] as $exclude) foreach($scenarios as [$data,$rules]) {
 $factory=new Illuminate\Validation\Factory($translator);
 if($exclude)$factory->excludeUnvalidatedArrayKeys();else $factory->includeUnvalidatedArrayKeys();
 $v=$factory->make($data,$rules);$passes=$v->passes();
 $arrayPolicyCases[]=['exclude'=>$exclude,'data'=>$data,'rules'=>$rules,'passes'=>$passes,'errors'=>(object)$v->errors()->toArray(),'validated'=>$passes?(object)$v->validated():null];
}
echo json_encode(['source'=>'laravel/framework@1449bf241017087fbd38c118ecaabef3f6797fdc','php'=>PHP_VERSION,'timezone_database'=>timezone_version_get(),'dependencies'=>array_combine(['brick/math','symfony/uid','nesbot/carbon','psr/container'],array_map(fn($package)=>Composer\InstalledVersions::getPrettyVersion($package),['brick/math','symfony/uid','nesbot/carbon','psr/container'])),'cases'=>$cases,'mutations'=>$mutations,'extensions'=>$extensionCases,'array_policy'=>$arrayPolicyCases],JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES|JSON_PRESERVE_ZERO_FRACTION);
