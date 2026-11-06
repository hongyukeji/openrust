<?php
// 未启 strict_types：这里刻意复现固定 Laravel 对 BackedEnum 的弱标量调用。
$warnings = [];
set_error_handler(function(int $severity, string $message) use (&$warnings): bool {
    $warnings[] = ['severity' => $severity, 'message' => $message];
    return true;
});
function representation(mixed $value): array {
    return match (gettype($value)) {
        'double' => ['type'=>'float', 'bits'=>bin2hex(pack('E', $value)), 'display'=>(string)$value],
        'integer' => ['type'=>'integer', 'decimal'=>(string)$value],
        'boolean' => ['type'=>'boolean', 'value'=>$value],
        'NULL' => ['type'=>'null'],
        'string' => ['type'=>'string', 'base64'=>base64_encode($value)],
        default => ['type'=>gettype($value)],
    };
}
$cases = json_decode(file_get_contents($argv[1]), true, 512, JSON_THROW_ON_ERROR);
foreach ($cases as $case) {
    $warnings = [];
    $value = $case['kind'] === 'bits'
        ? unpack('Evalue', hex2bin($case['bits']))['value']
        : json_decode($case['raw'], true, 512, JSON_THROW_ON_ERROR);
    $row = ['id'=>$case['id'], 'input'=>representation($value)];
    foreach (['string','integer','float','boolean'] as $operation) {
        $warnings = [];
        try {
            $converted = match ($operation) {
                'string' => (string)$value,
                'integer' => (int)$value,
                'float' => (float)$value,
                'boolean' => filter_var($value, FILTER_VALIDATE_BOOLEAN),
            };
            $row[$operation] = representation($converted);
        } catch (Throwable $error) {
            $row[$operation] = ['error_class'=>get_class($error), 'message'=>$error->getMessage()];
        }
        $row[$operation]['warnings'] = $warnings;
    }
    echo json_encode($row, JSON_THROW_ON_ERROR | JSON_UNESCAPED_SLASHES), "\n";
}
