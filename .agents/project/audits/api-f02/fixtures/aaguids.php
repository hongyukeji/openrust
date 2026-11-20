<?php
require '/upstream/src/Support/Aaguids.php';
use Laravel\Passkeys\Support\Aaguids;
$keys = array_merge(array_keys(Aaguids::all()), [Aaguids::unknown(), '', 'nonexistent', strtoupper(array_key_first(Aaguids::all()))]);
$output = [];
foreach ($keys as $key) {
 $output[] = ['id'=>$key, 'label'=>Aaguids::labelFor($key), 'icons'=>array_map(fn ($theme)=>($icon=Aaguids::iconFor($key,$theme))===null?null:hash('sha256',$icon), ['light','dark'])];
}
try { Aaguids::iconFor('', 'invalid'); } catch (InvalidArgumentException $error) { $invalid=$error->getMessage(); }
$before=Aaguids::all(); Aaguids::flush(); assert($before===Aaguids::all());
echo json_encode(['cases'=>$output,'invalid_theme'=>$invalid], JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR),"\n";
