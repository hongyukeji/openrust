<?php
spl_autoload_register(function ($class) use ($argv) {
 if (str_starts_with($class, 'Illuminate\\')) {
  $relative = $class === 'Illuminate\\Support\\Traits\\Macroable' ? 'Illuminate/Macroable/Traits/Macroable' : str_replace('\\', '/', $class);
  $file = rtrim($argv[1], '/').'/src/'.$relative.'.php';
  if (is_file($file)) require $file;
 }
});
require rtrim($argv[1] ?? throw new RuntimeException('请传入固定上游源码目录'), '/').'/src/Illuminate/Validation/Concerns/ValidatesAttributes.php';
class Fixture { use Illuminate\Validation\Concerns\ValidatesAttributes; }
$validator = new Fixture;
$cases=[];
$rules=[
 'lowercase'=>['abc','ABC','中文','123','é','É','ß','İ','i̇','ΟΣ','ος','οσ',123,true,null,[]],
 'uppercase'=>['ABC','abc','中文','123','é','É','ß','SS','İ','i̇','ΟΣ','ος','οσ',123,true,null,[]],
 'not_regex:/^foo/i'=>['foo','FOObar','bar',123,true,[],null],
 'starts_with:foo,bar'=>['foo1','bar2','afoo',123,true,[],null],
 'doesnt_start_with:foo,bar'=>['foo1','bar2','afoo',123,true,[],null],
 'ends_with:foo,bar'=>['1foo','2bar','fooa',123,true,[],null],
 'doesnt_end_with:foo,bar'=>['1foo','2bar','fooa',123,true,[],null],
 'starts_with:12'=>[123,12.5,'0123'],
 'ends_with:23'=>[123,1.23,'1230'],
 'starts_with:'=>['foo',123],
 'doesnt_start_with'=>['foo',123,true],
 'ends_with:'=>['foo',123],
 'doesnt_end_with'=>['foo',123,true],
 'digits:3'=>['123','012',123,123.0,'12','-12','1.2',true,'１２３'],
 'digits_between:2,4'=>['12','1234','1','12345',123,true],
 'max_digits:3'=>['123','12','1234',123.0,true,'-1'],
 'min_digits:3'=>['123','001','12',123.0,true,'1.2'],
 'decimal:2'=>['1','1.00','1.2',1.0,true,'1e1',"1.20\n",' 1.20','.5','-0.20'],
 'decimal:1,3'=>['1.0','1.23','1.234','1.2345','1','-0.2'],
 'alpha'=>['abc','中文',"e\u{0301}",'abc1',123,true,'',null],
 'alpha:ascii'=>['abc','中文',"e\u{0301}",123],
 'alpha_dash'=>['abc-1_','中文-１２',123,1.0,1.2,true,'a b'],
 'alpha_num'=>['abc1','中文１２',123,1.0,true,'a-b'],
 'list'=>[[],[1,2],['a'=>1],['0'=>1,'1'=>2],['1'=>1]],
 'ip'=>['127.0.0.1','::1','::ffff:192.0.2.1','01.2.3.4','256.0.0.1','fe80::1%lo0',1,true],
 'ipv4'=>['127.0.0.1','::1','01.2.3.4'],
 'ipv6'=>['::1','127.0.0.1','::ffff:192.0.2.1'],
 'mac_address'=>['01:23:45:67:89:ab','01-23-45-67-89-ab','0123.4567.89ab','0123456789ab','01:23-45:67:89:ab','1:2:3:4:5:6',true],
 'json'=>['{}','[]','null','true','1','{broken}',true,false,123,1.2,null,[]],
 'hex_color'=>['#fff','#ffff','#123456','#12345678',"#abc\n",'fff','#12345','#GGG',123]
];
$methods=['lowercase'=>'Lowercase','uppercase'=>'Uppercase','not_regex'=>'NotRegex','starts_with'=>'StartsWith','doesnt_start_with'=>'DoesntStartWith','ends_with'=>'EndsWith','doesnt_end_with'=>'DoesntEndWith','digits'=>'Digits','digits_between'=>'DigitsBetween','max_digits'=>'MaxDigits','min_digits'=>'MinDigits','decimal'=>'Decimal','alpha'=>'Alpha','alpha_dash'=>'AlphaDash','alpha_num'=>'AlphaNum','list'=>'List','ip'=>'Ip','ipv4'=>'Ipv4','ipv6'=>'Ipv6','mac_address'=>'MacAddress','json'=>'Json','hex_color'=>'HexColor'];
foreach($rules as $rule=>$values) {
 [$name,$parameter]=array_pad(explode(':',$rule,2),2,null);
 foreach($values as $value) {
  $method='validate'.$methods[$name];
  $cases[]=['rule'=>$rule,'value'=>$value,'passes'=>(bool)$validator->$method('value',$value,$parameter===null?[]:explode(',',$parameter))];
 }
}
echo json_encode(['source'=>'laravel/framework@1449bf241017087fbd38c118ecaabef3f6797fdc','cases'=>$cases],JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES|JSON_PRESERVE_ZERO_FRACTION);
