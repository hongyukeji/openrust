<?php
// 调用环境保持弱类型，与 Laravel InteractsWithData 相同。
enum TextChoice: string { case Email = 'email'; case One = '1'; case Empty = ''; }
enum NumberChoice: int { case Zero = 0; case One = 1; case Two = 2; case Max = PHP_INT_MAX; case Min = PHP_INT_MIN; }
$warnings = [];
set_error_handler(function(int $severity, string $message) use (&$warnings): bool {
    $warnings[] = ['severity'=>$severity, 'message'=>$message];
    return true;
});
$cases = [
    'empty_array'=>[], 'nonempty_array'=>[1], 'object'=>(object)[],
    'stringable'=>new class {public function __toString(): string {return 'email';}},
    'nonnumeric'=>'email', 'numeric_string'=>'1', 'zero_string'=>'0',
    'prefix_string'=>'1ab', 'spaced_numeric'=>' 1 ', 'decimal_string'=>'1.5',
    'exponent_string'=>'1e0', 'leading_zero'=>'01', 'empty_string'=>'',
    'int_zero'=>0, 'int_one'=>1, 'int_two'=>2,
    'float_one'=>1.0, 'float_fraction'=>1.5, 'negative_fraction'=>-0.5,
    'null'=>null, 'false'=>false, 'true'=>true,
    'infinity'=>INF, 'nan'=>NAN,
];
foreach ($cases as $id=>$value) {
    foreach ([TextChoice::class, NumberChoice::class] as $enum) {
        $warnings = [];
        try {
            $result = $enum::tryFrom($value);
            $row = ['id'=>$id,'enum'=>$enum,'case'=>$result?->name,'warnings'=>$warnings];
        } catch (Throwable $error) {
            $row = ['id'=>$id,'enum'=>$enum,'error_class'=>get_class($error),'message'=>$error->getMessage(),'warnings'=>$warnings];
        }
        echo json_encode($row,JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES), "\n";
    }
}
