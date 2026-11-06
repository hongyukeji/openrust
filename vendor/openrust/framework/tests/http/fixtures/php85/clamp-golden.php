<?php
// 保留 Laravel Data.php:314-322 的默认与 is_numeric 顺序，以及 Number.php:331 的弱参数转换。
function typed_number_clamp(int|float $number, int|float $min, int|float $max): int|float {
    return min(max($number, $min), $max);
}
function request_clamp(bool $present, mixed $input, mixed $min, mixed $max, mixed $default = 0): int|float {
    $value = $present ? $input : $default;
    if (!is_numeric($value)) {
        $value = $default;
    }
    return typed_number_clamp($value, $min, $max);
}
function clamp_repr(mixed $v): array {
    return match (gettype($v)) {
        'double'=>['type'=>'float','bits'=>bin2hex(pack('E',$v))],
        'integer'=>['type'=>'integer','decimal'=>(string)$v],
        'boolean'=>['type'=>'boolean','value'=>$v],
        'string'=>['type'=>'string','base64'=>base64_encode($v)],
        'NULL'=>['type'=>'null'],
        'array'=>['type'=>'array','count'=>count($v)],
        default=>['type'=>gettype($v)],
    };
}
$cases = [
 ['id'=>'inside_int','value'=>2,'min'=>0,'max'=>4],
 ['id'=>'inside_float','value'=>2.0,'min'=>0,'max'=>4],
 ['id'=>'below','value'=>-1,'min'=>0,'max'=>4],
 ['id'=>'above','value'=>8,'min'=>0,'max'=>4],
 ['id'=>'equal_low_float_first','value'=>1.0,'min'=>1,'max'=>3],
 ['id'=>'equal_low_int_first','value'=>1,'min'=>1.0,'max'=>3],
 ['id'=>'equal_high_float_first','value'=>3.0,'min'=>1,'max'=>3],
 ['id'=>'equal_high_int_first','value'=>3,'min'=>1,'max'=>3.0],
 ['id'=>'equal_all_float_first','value'=>1.0,'min'=>1,'max'=>1],
 ['id'=>'equal_all_int_first','value'=>1,'min'=>1.0,'max'=>1.0],
 ['id'=>'reverse_inside','value'=>5,'min'=>10,'max'=>0],
 ['id'=>'reverse_below','value'=>-5,'min'=>10,'max'=>0],
 ['id'=>'reverse_above','value'=>15,'min'=>10,'max'=>0],
 ['id'=>'missing_int_default','present'=>false,'value'=>999,'min'=>0,'max'=>10,'default'=>2],
 ['id'=>'missing_float_default','present'=>false,'value'=>999,'min'=>0,'max'=>10,'default'=>2.0],
 ['id'=>'numeric_bounds_strings','value'=>'2','min'=>'1','max'=>'3'],
 ['id'=>'bad_default_string','value'=>'bad','min'=>0,'max'=>10,'default'=>'bad'],
 ['id'=>'bad_default_array','value'=>null,'min'=>0,'max'=>10,'default'=>[]],
];
foreach (['078',' 901','nan','1ab','2_000','.6','1.ab','1e3','1e+','1.0','1',' 1 ','1 ',"\f1", "\u{00a0}1",'',null,false,true,[],[1],(object)[]] as $i=>$value) {
 $cases[]=['id'=>'input_kind_'.$i,'value'=>$value,'min'=>0,'max'=>1000,'default'=>7.0];
}
foreach (['positive_inf'=>INF,'negative_inf'=>-INF,'nan'=>NAN,'positive_zero'=>0.0,'negative_zero'=>-0.0,'one_int'=>1,'one_float'=>1.0] as $label=>$v) {
 foreach (['negative_inf'=>-INF,'nan'=>NAN,'positive_inf'=>INF,'positive_zero'=>0.0,'negative_zero'=>-0.0,'zero_int'=>0] as $minLabel=>$min) {
  foreach (['negative_inf'=>-INF,'nan'=>NAN,'positive_inf'=>INF,'positive_zero'=>0.0,'negative_zero'=>-0.0,'one_int'=>1] as $maxLabel=>$max) {
   $cases[]=['id'=>"order_{$label}_{$minLabel}_{$maxLabel}",'value'=>$v,'min'=>$min,'max'=>$max];
  }
 }
}
foreach (['positive_inf'=>INF,'negative_inf'=>-INF,'nan'=>NAN,'positive_zero'=>0.0,'negative_zero'=>-0.0,'zero_int'=>0,'one_float'=>1.0] as $name=>$default) {
 $cases[]=['id'=>'default_'.$name,'value'=>'not numeric','min'=>-INF,'max'=>INF,'default'=>$default];
 $cases[]=['id'=>'missing_default_'.$name,'present'=>false,'value'=>123,'min'=>-INF,'max'=>INF,'default'=>$default];
}
$warnings = [];
set_error_handler(function(int $severity,string $message) use (&$warnings): bool {
 $warnings[]=['severity'=>$severity,'message'=>$message];return true;
});
foreach ($cases as $case) {
 $warnings=[];$present=$case['present']??true;$default=array_key_exists('default',$case)?$case['default']:0;
 // ?? 不能用来选择显式null输入；value 字段本身始终存在。
 $value=$case['value'];
 $row=['id'=>$case['id'],'present'=>$present,'arguments'=>[
   'value'=>clamp_repr($value),'min'=>clamp_repr($case['min']),'max'=>clamp_repr($case['max']),'default'=>clamp_repr($default)],
   'is_numeric_input'=>is_numeric($value),'is_numeric_selected'=>is_numeric($present?$value:$default)];
 try {$row['result']=clamp_repr(request_clamp($present,$value,$case['min'],$case['max'],$default));}
 catch(Throwable $e){$row['error_class']=get_class($e);$row['message']=$e->getMessage();}
 $row['warnings']=$warnings;
 echo json_encode($row,JSON_THROW_ON_ERROR|JSON_UNESCAPED_SLASHES),"\n";
}
