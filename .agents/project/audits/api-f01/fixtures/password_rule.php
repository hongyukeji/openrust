<?php
require $argv[1].'/vendor/autoload.php';
use Illuminate\Translation\{Translator,ArrayLoader};
use Illuminate\Validation\{Factory,Rules\Password};
use Illuminate\Support\Facades\Facade;
use Illuminate\Container\Container;
$loader=new ArrayLoader; $loader->addMessages('en','validation',require $argv[1].'/validation.php');
$factory=new Factory(new Translator($loader,'en'));
$container=new Container; $container->instance('validator',$factory); Facade::setFacadeApplication($container);
$values=[null,false,true,0,1,12345678,1.25,[],[1],['a'=>1], '', ' ', "\t", "\0", '1234567','12345678',str_repeat('é',8),str_repeat('例',8),'Aa12345!',"a\nA12345",'aⅠ²③四！',str_repeat('a',20)];
$cases=[];
foreach(['default','required','sometimes','min1','max8','strong'] as $mode) {
 foreach(array_merge([['missing'=>true]],array_map(fn($value)=>['value'=>$value],$values)) as $input) foreach([false,true] as $nullable) {
  $password=match($mode){'required'=>Password::required(),'sometimes'=>Password::sometimes(),'min1'=>Password::min(0),'max8'=>Password::min(3)->max(8),'strong'=>Password::min(3)->mixedCase()->letters()->numbers()->symbols(),default=>Password::default()};
  $data=array_key_exists('value',$input)?['password'=>$input['value']]:[];
  $validator=$factory->make($data,['password'=>array_merge($nullable?['nullable']:[],[$password])]);
  $cases[]=['kind'=>'password','mode'=>$mode,'nullable'=>$nullable,'data'=>(object)$data,'passes'=>$validator->passes(),'failed'=>$validator->failed(),'messages'=>$validator->errors()->toArray()];
 }
}
foreach($values as $value) foreach($values as $other) {
 $data=['password'=>$value,'password_confirmation'=>$other];
 $validator=$factory->make($data,['password'=>'confirmed']);
 $cases[]=['kind'=>'confirmed','data'=>$data,'passes'=>$validator->passes(),'failed'=>$validator->failed(),'messages'=>$validator->errors()->toArray()];
}
foreach([
 ['data'=>['password'=>'secret','repeat'=>'secret'],'field'=>'password','rule'=>'confirmed:repeat'],
 ['data'=>['password'=>'secret','repeat'=>'bad'],'field'=>'password','rule'=>'confirmed:repeat'],
 ['data'=>['password'=>null],'field'=>'password','rule'=>'confirmed'],
 ['data'=>['users'=>[['password'=>'secret','password_confirmation'=>'secret'],['password'=>'x','password_confirmation'=>'y']]],'field'=>'users.*.password','rule'=>'confirmed'],
 ['data'=>['password'=>['a'=>1,'b'=>2],'password_confirmation'=>['b'=>2,'a'=>1]],'field'=>'password','rule'=>'confirmed'],
 ['data'=>['password'=>['a'=>1,'b'=>2],'password_confirmation'=>['a'=>1,'b'=>2]],'field'=>'password','rule'=>'confirmed'],
] as $row) {$validator=$factory->make(json_decode(json_encode($row['data']),true),[$row['field']=>$row['rule']]);$cases[]=['kind'=>'confirmed',...$row,'passes'=>$validator->passes(),'failed'=>$validator->failed(),'messages'=>$validator->errors()->toArray()];}
foreach([
 ['mode'=>'strong','data'=>['password'=>'abc'],'custom_messages'=>['password.password.mixed'=>'Mixed :attribute','password.password.symbols'=>'Symbol :attribute','password.password.numbers'=>'Number :attribute'],'attributes'=>['password'=>'access code']],
 ['mode'=>'default','data'=>['password'=>'abc'],'custom_messages'=>['password.min'=>'Minimum :min for :attribute'],'attributes'=>['password'=>'access code']],
] as $row) {
 $rule=$row['mode']==='strong'?Password::min(3)->mixedCase()->letters()->numbers()->symbols():Password::default();
 $validator=$factory->make($row['data'],['password'=>[$rule]],$row['custom_messages'],$row['attributes']);
 $cases[]=['kind'=>'password','nullable'=>false,...$row,'passes'=>$validator->passes(),'failed'=>$validator->failed(),'messages'=>$validator->errors()->toArray()];
}
foreach([
 ['data'=>['users'=>[['password'=>'x','repeat'=>'x'],['password'=>'y','repeat'=>'z']]],'field'=>'users.*.password','rule'=>'confirmed:users.*.repeat'],
 ['data'=>['password'=>[],'password_confirmation'=>(object)[]],'field'=>'password','rule'=>'confirmed'],
 ['data'=>['password'=>['a'],'password_confirmation'=>(object)['0'=>'a']],'field'=>'password','rule'=>'confirmed'],
] as $row) {$validator=$factory->make(json_decode(json_encode($row['data']),true),[$row['field']=>$row['rule']]);$cases[]=['kind'=>'confirmed',...$row,'passes'=>$validator->passes(),'failed'=>$validator->failed(),'messages'=>$validator->errors()->toArray()];}
echo json_encode(['php'=>PHP_VERSION,'cases'=>$cases],JSON_UNESCAPED_UNICODE|JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR)."\n";
