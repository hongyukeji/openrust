<?php
    // Reject a different upstream revision before executing its code.
    $manifest = json_decode(file_get_contents(__DIR__.'/changes_reference.sources.json'), true, 512, JSON_THROW_ON_ERROR);
    $sourceRoot = ($argv[1] ?? '');
    foreach ($manifest['files'] as $file => $sha256) {
        $source = $sourceRoot.'/'.$file;
        if (!is_file($source) || hash_file('sha256', $source) !== $sha256) {
            throw new \RuntimeException('Unexpected pinned reference source: '.$file.' at '.$manifest['commit']);
        }
    }

require $argv[1].'/src/Support/ChangeDetector.php';
function collect($values) { return new class($values) {
    public function __construct(private $values) {}
    public function only($keys) { return new self(array_intersect_key($this->values,array_flip($keys))); }
    public function all() { return $this->values; }
}; }
$values = [null,false,true,0,1,-1,1.5,'','0','00','1','1.0','1e0',' 1 ','+1','-1','1.5','abc','1abc',[],[0],[null],['key'=>0],['key'=>null],['0'=>1],['1'=>1],'9223372036854775807','9223372036854775806',PHP_INT_MAX,PHP_INT_MAX-1];
$cases=[];
foreach ($values as $new) foreach ($values as $old) {
    $result=\Spatie\Activitylog\Support\ChangeDetector::filterDirty(['value'=>$new],['value'=>$old]);
    $cases[]=['new'=>$new,'old'=>$old,'changed'=>array_key_exists('value',$result['attributes'])];
}
echo json_encode($cases,JSON_PRETTY_PRINT|JSON_PRESERVE_ZERO_FRACTION),"\n";
