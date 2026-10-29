<?php
// 自造边界 case；只执行运算表达式及真实官方 HandleExceptions，不构造假 Store。
// 用法：php -n oracle.php /path/to/fixed/HandleExceptions.php > golden.json
if ($argc !== 2 || hash_file('sha256', $argv[1]) !== 'dad8c51973a06c8a636ab744aafd7f06721fec08afa6255019c81ffafea04524') {
    fwrite(STDERR, "固定 HandleExceptions 源 SHA 不匹配\n"); exit(1);
}
require $argv[1];
error_reporting(E_ALL);
$handler = new Illuminate\Foundation\Bootstrap\HandleExceptions;
$cases = [
    ['null', 'increment', null, 1], ['false', 'increment', false, 1],
    ['true', 'increment', true, 1], ['numeric', 'increment', '42', 1],
    ['space_decimal', 'increment', ' 2.5 ', 1], ['scientific', 'increment', '1e3', 1],
    ['leading_warning', 'increment', '42tail', 1], ['empty', 'increment', '', 1],
    ['nonnumeric', 'increment', 'tail', 1], ['array_scalar', 'increment', [1], 1],
    ['int_overflow', 'increment', PHP_INT_MAX, 1],
    ['min_decrement', 'decrement', PHP_INT_MIN, PHP_INT_MIN],
    ['finite_overflow', 'increment', PHP_FLOAT_MAX, PHP_FLOAT_MAX],
    ['left_union', 'increment', ['a'=>1,2=>'left'], ['a'=>9,2=>'right','b'=>2]],
    ['array_decrement', 'decrement', ['a'=>1], ['c'=>3]],
    ['push_null', 'push', null, 'new'], ['push_false', 'push', false, 'new'],
    ['push_sparse', 'push', [2=>'two',5=>'five'], 'new'],
    ['push_negative', 'push', [-5=>'a',-2=>'b'], 'new'],
    ['push_assoc', 'push', ['label'=>'old'], 'new'],
    ['push_string', 'push', 'abc', 'new'], ['push_int', 'push', 3, 'new'],
];
$output = [];
foreach ($cases as [$name, $op, $initial, $amount]) {
    foreach (['expression', 'official_handle_error'] as $mode) {
        $events = [];
        set_error_handler(function ($severity, $message, $file, $line) use (&$events, $mode, $handler) {
            $events[] = ['level'=>$severity, 'message'=>$message];
            if ($mode === 'official_handle_error') { $handler->handleError($severity, $message, $file, $line); }
            return true;
        });
        $row = ['case'=>$name, 'mode'=>$mode, 'op'=>$op, 'initial'=>$initial, 'amount'=>$amount];
        try {
            $value = $initial;
            if ($op === 'push') { $value[] = $amount; }
            else { $value = $value + ($op === 'decrement' ? $amount * -1 : $amount); }
            if (is_float($value) && !is_finite($value)) {
                $row += ['outcome'=>'nonfinite', 'type'=>'float', 'value'=>strval($value)];
            } else { $row += ['outcome'=>'value', 'type'=>get_debug_type($value), 'value'=>$value]; }
        } catch (Throwable $error) { $row += ['outcome'=>'error', 'type'=>get_class($error)]; }
        finally { restore_error_handler(); }
        $row['events'] = $events;
        $output[] = $row;
    }
}
echo json_encode(['php_version'=>PHP_VERSION, 'cases'=>$output], JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR), "\n";
