<?php
// 参考源码固定为 1449bf241017087fbd38c118ecaabef3f6797fdc。
$root = rtrim($argv[1] ?? throw new RuntimeException('请传入固定上游 Framework 源码目录'), '/').'/src/Illuminate/';
spl_autoload_register(function ($name) use ($root) {
    if (str_starts_with($name, 'Illuminate\\')) {
        $file = $root.str_replace('\\','/',substr($name,11)).'.php';
        if (is_file($file)) require $file;
    }
});
use Illuminate\JsonSchema\JsonSchema;
$cases = [
 ['type'=>'string','minLength'=>1,'maxLength'=>12,'format'=>'email','default'=>'a','title'=>'邮箱'],
 ['type'=>'integer','minimum'=>'-2','maximum'=>'10.0','multipleOf'=>'2','default'=>3],
 ['type'=>'number','minimum'=>0.25,'maximum'=>3,'multipleOf'=>'0.25'],
 ['type'=>'boolean','default'=>false],
 ['type'=>'object','properties'=>['b'=>['type'=>'string'],'a'=>['type'=>['integer','null']]],'required'=>['a'],'additionalProperties'=>false],
 ['type'=>'object','properties'=>[]],
 ['type'=>'array','items'=>['type'=>'string'],'minItems'=>0,'maxItems'=>2,'uniqueItems'=>true,'default'=>[]],
 ['type'=>'array','items'=>[],'uniqueItems'=>false],
 ['type'=>['integer','string','integer','null'],'description'=>'union','enum'=>[1,'a',1]],
 ['anyOf'=>[['type'=>'string'],['type'=>'integer'],['type'=>'null']],'title'=>'组合'],
 ['anyOf'=>[['type'=>'string'],['type'=>'null']],'title'=>'nullable'],
 ['oneOf'=>[['type'=>'null'],['type'=>'integer','minimum'=>1]],'title'=>'one'],
 ['$defs'=>['a/b~c'=>['type'=>'string','minLength'=>2]],'$ref'=>'#/$defs/a~1b~0c','maxLength'=>5],
 ['type'=>'object','properties'=>['a'=>['$ref'=>'#/$defs/name'],'b'=>['$ref'=>'#/$defs/name']],'$defs'=>['name'=>['type'=>'string']]],
 ['enum'=>[1,1.5]], ['enum'=>[true,false]], ['minLength'=>'2'], ['minimum'=>'1.5'],
 ['additionalProperties'=>true], ['type'=>['null'],'properties'=>[]],
 ['anyOf'=>[['type'=>'null'],['type'=>['null']]]], ['anyOf'=>[]],
 ['type'=>'integer','minimum'=>1.5], ['type'=>'number','maximum'=>'broken'],
 ['type'=>['integer','string'],'minimum'=>0], ['type'=>'null'], ['type'=>'unsupported'],
 ['type'=>'array','items'=>true], ['type'=>'object','properties'=>['a'=>false]],
 ['$ref'=>'#/missing'], ['$ref'=>'https://invalid.example/schema'], ['$ref'=>'#'],
 ['$defs'=>['a'=>['$ref'=>'#/$defs/b'],'b'=>['$ref'=>'#/$defs/a']],'$ref'=>'#/$defs/a'],
 ['type'=>'string','default'=>null], ['oneOf'=>[['type'=>'string'],['type'=>'integer']]],
 ['oneOf'=>[['type'=>'string','title'=>'a'],['type'=>'null']],'title'=>'conflict'],
 ['anyOf'=>[true,['type'=>'string']]], ['enum'=>[1,'a']], [],
];
$out=[];
foreach($cases as $schema){
    try { $out[]=['schema'=>(object)$schema,'expected'=>JsonSchema::fromArray($schema)->toArray()]; }
    catch(Throwable $e) { $out[]=['schema'=>(object)$schema,'error'=>true]; }
}
echo json_encode(['source'=>'laravel/framework@1449bf241017087fbd38c118ecaabef3f6797fdc','cases'=>$out], JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES);
