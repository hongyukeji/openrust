<?php
// 独立固定函数参考；不是完整 Laravel 应用或官方测试套件。
require $argv[1].'/'.$argv[2].'/HeaderUtils.php';
require $argv[1].'/framework/Macroable.php';
require $argv[1].'/framework/Arr.php';
$input = json_decode(file_get_contents($argv[3]), true, flags: JSON_THROW_ON_ERROR);
$errors = [];
set_error_handler(function ($severity, $message) use (&$errors) {
    $errors[] = ['severity' => $severity, 'message' => $message];
    return true;
});
$output = ['metadata' => ['php_version' => PHP_VERSION, 'php_version_id' => PHP_VERSION_ID,
    'precision' => ini_get('precision'), 'max_input_vars' => ini_get('max_input_vars'),
    'max_input_nesting_level' => ini_get('max_input_nesting_level'),
    'symfony_tag' => $argv[2]], 'canonical' => [], 'operations' => []];
foreach ($input['canonical'] as $case) {
    $parsed = Symfony\Component\HttpFoundation\HeaderUtils::parseQuery($case['raw']);
    ksort($parsed);
    $case['expected'] = http_build_query($parsed, '', '&', PHP_QUERY_RFC3986);
    $output['canonical'][] = $case;
}
foreach ($input['operations'] as $case) {
    $remaining = $case['current'];
    if ($case['operation'] === 'merge') {
        $remaining = array_merge($remaining, $case['additional']);
    } elseif ($case['operation'] === 'without') {
        $remaining = Illuminate\Support\Arr::except($remaining, $case['keys']);
    }
    $case['expected'] = Illuminate\Support\Arr::query($remaining);
    $case['has_remaining'] = count($remaining) > 0;
    $output['operations'][] = $case;
}
$output['warnings'] = $errors;
echo json_encode($output, JSON_UNESCAPED_SLASHES|JSON_UNESCAPED_UNICODE|JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR), "\n";
