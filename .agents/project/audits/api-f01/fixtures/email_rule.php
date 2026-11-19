<?php
namespace PHPUnit\Framework { class TestCase {} }
namespace {
$base=$argv[1];
spl_autoload_register(function ($name) use ($base) {
    $prefixes = [
        'Egulias\\EmailValidator\\' => '/upstream/src/',
        'Doctrine\\Common\\Lexer\\' => '/doctrine/src/',
        'Symfony\\Polyfill\\Intl\\Idn\\' => '/idn/',
        'Symfony\\Polyfill\\Intl\\Normalizer\\' => '/normalizer/',
        'Symfony\\Polyfill\\Mbstring\\' => '/mbstring/',
    ];
    foreach ($prefixes as $prefix=>$dir) {
        if (str_starts_with($name, $prefix)) {
            $path=$base.$dir.str_replace('\\','/',substr($name,strlen($prefix))).'.php';
            if (is_file($path)) require $path;
            return;
        }
    }
});
require $base.'/mbstring/bootstrap.php';
require $base.'/normalizer/bootstrap.php';
require $base.'/normalizer/Resources/stubs/Normalizer.php';
require $base.'/idn/bootstrap.php';
require $base.'/upstream/tests/EmailValidator/Validation/RFCValidationTest.php';
$test='Egulias\\EmailValidator\\Tests\\EmailValidator\\Validation\\RFCValidationTest';
$values=[];
foreach (['getValidEmails','getInvalidEmails','getValidEmailsWithWarnings','getInvalidEmailsWithErrors'] as $method) {
    foreach ($test::$method() as $row) $values[]=$row[0];
}
// Exhaustive short-token combinations exercise the same original parser.
$tokens=['a','1','.', '@', '"', '(', ')', '[', ']', '\\', ' ', "\t", "\0", '::', '-', '_', 'IPv6','é','K','ſ','ß','ς',"\u{200c}"];
foreach ($tokens as $a) foreach ($tokens as $b) {
    foreach ([$a.$b.'@example.test','a@'.$a.$b.'.test','"'.$a.$b.'"@test','a('.$a.$b.')@test','a@['.$a.$b.']'] as $value) $values[]=$value;
}
foreach (['a','é','ъ','ß','ς','例','Ａ',"\u{200c}","\u{0301}"] as $char) {
    foreach ([1,31,32,40,55,57,58,59,60,61,62,63,64,65,126,253,254,255] as $length) {
        $word=str_repeat($char,$length);
        foreach ([$word.'@test','a@'.$word.'.test','a@test.'.$word,'a@['.$word.']'] as $value) $values[]=$value;
    }
}
mt_srand(20261120);
$alphabet=['a','b','1','.','@','"','(',')','[',']','\\',' ',"\t", "\0",'-','é','例','ß',"\u{0301}","\u{FFFD}"];
for ($i=0;$i<6000;$i++) {
    $word='';
    for ($j=0,$length=mt_rand(1,18);$j<$length;$j++) $word.=$alphabet[mt_rand(0,count($alphabet)-1)];
    $values[] = match ($i%3) {0=>$word.'@example.test', 1=>'a@'.$word.'.test', default=>$word};
}
foreach ([null,false,true,0,1,[],['a'],new \stdClass(),'',"a\r@b", "a@b\n", "a\r\n @b", 'a@b[none]suffix','a@b[.-]suffix','a@b[.]suffix'] as $value) $values[]=$value;
$trait = file_get_contents($base.'/laravel/Concerns/ValidatesAttributes.php');
$start = strpos($trait,'    public function validateEmail(');
$end = strpos($trait, "\n    /**", $start);
$body=substr($trait,$start,$end-$start);
// Use the real pinned Laravel method, including its value and CR/LF gates.
eval('namespace Illuminate\\Validation\\Concerns; use Harness\\Collection; use Harness\\Container; use Egulias\\EmailValidator\\EmailValidator; use Egulias\\EmailValidator\\Validation\\RFCValidation; use Egulias\\EmailValidator\\Validation\\MultipleValidationWithAnd; class EmailHarness { '.$body.' }');
$validator=new \Illuminate\Validation\Concerns\EmailHarness();
$rows=[]; $invalidUtf8=[];
foreach ($values as $value) {
    $passes=$validator->validateEmail('email',$value,[]);
    if (is_string($value) && !preg_match('//u', $value)) {
        $invalidUtf8[]=['base64'=>base64_encode($value),'passes'=>$passes];
    } else {
        $rows[]=['value'=>$value,'passes'=>$passes];
    }
}
echo json_encode(['php'=>PHP_VERSION,'cases'=>$rows,'invalid_utf8'=>$invalidUtf8], JSON_UNESCAPED_UNICODE|JSON_PRETTY_PRINT|JSON_THROW_ON_ERROR)."\n";
}
namespace Harness {
class Collection {
    private $items;
    function __construct($items) {$this->items=$items;}
    function unique() {return new self(array_unique($this->items));}
    function map($fn) {return new self(array_map($fn,$this->items));}
    function all() {return $this->items;}
}
class Container {
    static function getInstance() {return new self();}
    function make($class) {return new $class();}
}
}
