<?php
// Run the unmodified pinned upstream classes; no Composer or application database is needed.
namespace Illuminate\Contracts\Config { interface Repository extends \ArrayAccess {} }
namespace {
    // Reject a different upstream revision before executing its code.
    $manifest = json_decode(file_get_contents(__DIR__.'/support_reference.sources.json'), true, 512, JSON_THROW_ON_ERROR);
    $sourceRoot = ($argv[1] ?? '');
    foreach ($manifest['files'] as $file => $sha256) {
        $source = $sourceRoot.'/'.$file;
        if (!is_file($source) || hash_file('sha256', $source) !== $sha256) {
            throw new \RuntimeException('Unexpected pinned reference source: '.$file.' at '.$manifest['commit']);
        }
    }

    $root = $argv[1];
    require "$root/src/Support/LogOptions.php";
    require "$root/src/Support/ActivityLogStatus.php";
    $cases = [];
    $capture = function ($name, $options) use (&$cases) {
        $cases[$name] = get_object_vars($options);
    };
    $capture('defaults', \Spatie\Activitylog\Support\LogOptions::defaults());
    $capture('all', \Spatie\Activitylog\Support\LogOptions::defaults()->logAll());
    $capture('replace', \Spatie\Activitylog\Support\LogOptions::defaults()->logAll()->logOnly(['name'])->logOnly(['email']));
    $capture('fillable', \Spatie\Activitylog\Support\LogOptions::defaults()->logFillable()->dontLogFillable());
    $capture('dirty', \Spatie\Activitylog\Support\LogOptions::defaults()->logOnlyDirty()->dontLogIfAttributesChangedOnly(['updated_at']));
    $capture('empty', \Spatie\Activitylog\Support\LogOptions::defaults()->dontLogEmptyChanges()->logEmptyChanges());
    $capture('null_log', \Spatie\Activitylog\Support\LogOptions::defaults()->useLogName(null));
    $capture('combined', \Spatie\Activitylog\Support\LogOptions::defaults()->logUnguarded()->logFillable()->logExcept(['password'])->useLogName('accounts')->useAttributeRawValues(['name']));
    $status = [];
    foreach ([[], ['activitylog.enabled'=>null], ['activitylog.enabled'=>true], ['activitylog.enabled'=>false]] as $values) {
        $config = new class($values) extends \ArrayObject implements \Illuminate\Contracts\Config\Repository {};
        $value = new \Spatie\Activitylog\Support\ActivityLogStatus($config);
        $status[] = ['disabled'=>$value->disabled(), 'disable'=>$value->disable(), 'afterDisable'=>$value->disabled(), 'enable'=>$value->enable(), 'afterEnable'=>$value->disabled()];
    }
    echo json_encode(['options'=>$cases, 'status'=>$status], JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES), "\n";
}
