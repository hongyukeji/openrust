<?php

declare(strict_types=1);

header('Content-Type: application/json; charset=utf-8');

// 仅观察真实 HTTP SAPI 填充的 $_COOKIE；不使用 parse_str 或显式 Request::create 入参。
$json = json_encode($_COOKIE, JSON_PRESERVE_ZERO_FRACTION);
if ($json !== false) {
    echo $json;
    return;
}

// JSON 无法表示非法 UTF-8 时保留每个 PHP 字符串的原字节，不把 false 误当空 bag。
function cookie_binary_tree(mixed $value): array
{
    if (is_array($value)) {
        $children = [];
        foreach ($value as $key => $child) {
            $children[] = [
                'key_type' => is_int($key) ? 'int' : 'string',
                'key' => (string) $key,
                'value' => cookie_binary_tree($child),
            ];
        }
        return ['type' => 'array', 'children' => $children];
    }

    return ['type' => 'bytes', 'base64' => base64_encode((string) $value)];
}

$error = json_last_error_msg();
echo json_encode([
    'type' => 'binary_tree',
    'json_error' => $error,
    'cookies' => cookie_binary_tree($_COOKIE),
], JSON_PRESERVE_ZERO_FRACTION);
