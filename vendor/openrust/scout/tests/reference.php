<?php
// Fixed CollectionEngine scalar/json_encode matching; PHP 8.5.12.
$values = [null, false, true, 0, 123, 1.0, 1.25, 1.0e-8, 1.0e20,
    "ÄPFEL", "ÉCOLE", "ΑΘΉΝΑ", "МОСКВА", "中文/😀", ["中文/😀", 1.0, 1.0e-8, false, null],
    ["nested" => ["url" => "https://example.test/中文", "number" => 1.0e20]]];
$queries = ["äpfel", "école", "αθήνα", "москва", "null", "false", "true", "0", "1", "1.0", "1.0e-8", "1.0e+20",
    "中文", "\\u4e2d", "\\/", "\\ud83d\\ude00", "https", "nested", "", " "];
$cases = [];
foreach ($queries as $query) {
    $ids = [];
    foreach ($values as $index => $value) {
        $text = is_scalar($value) ? (string) $value : json_encode($value);
        if ($query === "" || str_contains(mb_strtolower($text, "UTF-8"), mb_strtolower($query, "UTF-8"))) {
            $ids[] = $index + 1;
        }
    }
    $cases[] = ['query' => $query, 'ids' => $ids];
}
echo json_encode(['values' => $values, 'cases' => $cases], JSON_PRETTY_PRINT | JSON_PRESERVE_ZERO_FRACTION | JSON_UNESCAPED_UNICODE), "\n";
