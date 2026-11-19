<?php
// Run the unmodified trait from the fixed Laravel framework commit.
if (PHP_VERSION !== '8.5.12') { throw new RuntimeException('Expected PHP 8.5.12'); }
$source = $argv[1];
if (hash_file('sha256', $source) !== '2101e3beeaedb2395ab44547778ccd036de559eaa5a2c1e80095d48cfa5cf64f') { throw new RuntimeException('Source SHA mismatch'); }
require $source;
$probe = new class {
    use Illuminate\Http\Concerns\CanBePrecognitive;
    public function matches($attribute, $header) {
        return $this->shouldValidatePrecognitiveAttribute($attribute, explode(',', $header));
    }
};
$attributes = ['name', 'confirmation', 'users.0.email', 'users.1.email', 'users.0.score', 'profile.name', 'user_name', 'é', '中文', 'star*', 'empty.', "line\n", 'a/b', 'a+b', 'a[b]'];
$selectors = ['', '*', '**', 'name', 'name,confirmation', ' name', 'name ', 'name,,', 'users.*.email', 'users.*', 'users.0.*', '*.0.email', '*.*.*', '*.*', 'profile.name', 'profile*', 'é', '中文', 'star*', 'empty.*', 'line', 'a/b', 'a+b', 'a[b]', "\xff", "\x00", 'name,name', 'users.*.email, name'];
$cases = [];
foreach ($selectors as $selector) {
    $selected = array_values(array_filter($attributes, fn ($attribute) => $probe->matches($attribute, $selector)));
    $cases[] = ['header' => base64_encode($selector), 'selected' => $selected];
}
echo json_encode(['php'=>PHP_VERSION, 'source_sha256'=>hash_file('sha256',$source), 'attributes'=>$attributes,'cases'=>$cases], JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR)."\n";
