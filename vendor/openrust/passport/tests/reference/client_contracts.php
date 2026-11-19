<?php
// These shells load the real Client setter/Scope methods without a PHP database runtime.
namespace Illuminate\Contracts\Support { interface Arrayable {} interface Jsonable {} }
namespace Illuminate\Database\Eloquent { class Model {} }
namespace Illuminate\Database\Eloquent\Concerns { trait HasUuids {} }
namespace Illuminate\Database\Eloquent\Factories { trait HasFactory {} }
namespace Illuminate\Database\Eloquent\Casts {
    class Attribute {
        public function __construct(public mixed $get = null, public mixed $set = null) {}
        public static function make(mixed $get = null, mixed $set = null): self { return new self($get, $set); }
    }
}
namespace {
    foreach ([
        'Client.php' => '503aad2b892dfb60d897ef52ed52f73d97e303182888f92ebd2c893725e7721d',
        'Scope.php' => 'af7aa63a93285af271d71a88fc2871ee7a78f5d18ea0615eb7ef74c89d4dfd10',
        'ResolvesInheritedScopes.php' => '11e212c5191f7ba80127c8f7abbbc9f38485776d1869083f67d4daddd14ff8c5',
    ] as $file => $expected) {
        if (hash_file('sha256', $argv[1].'/'.$file) !== $expected) { throw new \RuntimeException('Unexpected reference source'); }
    }
    require $argv[1].'/Scope.php';
    require $argv[1].'/ResolvesInheritedScopes.php';
    require $argv[1].'/Client.php';
    class Probe extends \Laravel\Passport\Client {
        public int $entered = 0;
        protected function castAttributeAsHashedString($key, $value) {
            if ($value === null) { return null; }
            $this->entered++;
            throw new \RuntimeException('hash boundary');
        }
        public function assign($value): array {
            $this->plainSecret = 'before';
            try { ($this->secret()->set)($value); $error = false; }
            catch (\Throwable) { $error = true; }
            return ['plain' => $this->plainSecret, 'entered' => $this->entered, 'error' => $error];
        }
    }
    $warnings = [];
    set_error_handler(function ($level, $message) use (&$warnings) { $warnings[] = [$level, $message]; return true; });
    $secrets = [];
    $bits = ['0000000000000000','8000000000000000','3ff0000000000000','3ee4f8b588e368f1',
        '3f1a36e2eb1c432d','42d6bcc41e900000','430c6bf526340000','4341c37937e08000',
        '7fefffffffffffff','0000000000000001','7ff0000000000000','fff0000000000000','7ff8000000000000'];
    // Fixed integer LCG components avoid host RNG or locale dependencies.
    $seed = 123456789;
    for ($n=0; $n<512; $n++) {
        $seed = ($seed * 1103515245 + 12345) & 0x7fffffff;
        $hi = $seed | (($n % 2) ? 0x80000000 : 0);
        $seed = ($seed * 1103515245 + 12345) & 0x7fffffff;
        $bits[] = sprintf('%08x%08x', $hi, $seed);
    }
    foreach ($bits as $hex) {
        $value = unpack('E', hex2bin($hex))[1];
        $secrets[] = ['type'=>'float','bits'=>$hex,'expected'=>(new Probe)->assign($value)];
    }
    foreach ([null, true, false, 0, -1, PHP_INT_MAX, PHP_INT_MIN, '', '0', 'plain', '账/😀', [], ['x']] as $value) {
        $secrets[] = ['type'=>'value','value'=>$value,'expected'=>(new Probe)->assign($value)];
    }
    $strings = ['', 'orders:read', '账/😀', "<>&'\"/\\\n\r\t\x00\x08\x0c\x1f\x7f", "a\u{2028}b\u{2029}",
        '0', '-0', '0.0', '-0.0', '1.0', '1e-5', '1e-4', '1e16', '1e17', '1e20',
        '  +42\t', ' 42 ', '0123', '1e999', 'NAN', 'INF', '9223372036854775808', '9007199254740993', '1.2345678901234567', '-9223372036854775808', '-9223372036854775808 ',
        '-9223372036854775809', '9223372036854775807 ', '1.7976931348623157e308', '2.2250738585072014e-308'];
    foreach (array_slice($bits, 0, 64) as $hex) {
        $value = unpack('E', hex2bin($hex))[1];
        if (is_finite($value)) { $strings[] = json_encode($value, JSON_PRESERVE_ZERO_FRACTION); }
    }
    $flags = [0,1,2,4,8,15,32,64,128,256,512,1024,2048,256|2048,64|256,32|1024,32|128|256,15|64|128|256,0x7fffffff];
    $json = [];
    foreach ($strings as $id) { foreach ($flags as $options) {
        $description = "desc/账😀\u{2028}";
        $scope = new \Laravel\Passport\Scope($id, $description);
        $json[] = ['id'=>$id,'description'=>$description,'options'=>$options,'expected'=>$scope->toJson($options)];
    }}
    $arrays = [];
    $haystacks = [[], [null], [false], [true], [0], [1], [-1], [0.0], [1e20], [PHP_INT_MAX], [PHP_INT_MIN],
        ['0'], ['00'], ['1e0'], ['9223372036854775808'], ['9223372036854775809'], ['INF'],
        [''=>['x'], '00'=>true, '1'=>'tail'], ['1'=>'one','0'=>'zero'], ['0'=>'zero','1'=>'one'],
        ['01'=>'a','-0'=>'b','9223372036854775808'=>'c'], [['x']], [[],false,0], ['账','a:b']];
    foreach (['', '0', '00', '1', '1e0', '+1', '-1', '1e20', '1.0E+20', 'INF', 'NAN', '9223372036854775807',
        '9223372036854775808', '9223372036854775809', '-9223372036854775808', ' 0 ', 'x', '账'] as $needle) {
        foreach ($haystacks as $array) {
            $arrays[] = ['needle'=>$needle,'array'=>$array,'expected'=>in_array($needle,$array)];
        }
    }
    echo json_encode(['php'=>PHP_VERSION,'passport'=>'f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7',
        'warnings'=>$warnings,'secrets'=>$secrets,'json'=>$json,'arrays'=>$arrays], JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES|JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR), "\n";
}
