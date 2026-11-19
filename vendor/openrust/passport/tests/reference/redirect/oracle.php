<?php
$directory = $argv[1] ?? __DIR__;
spl_autoload_register(function($class) use ($directory) {
    foreach (['League\\Uri\\'=>['league__uri','league__uri-interfaces'],'Psr\\Http\\Message\\'=>['psr__http-message/src']] as $prefix=>$roots) {
        if (!str_starts_with($class,$prefix)) continue;
        foreach($roots as $root) {
            $file=$directory.'/php-vendor/'.$root.'/'.str_replace('\\','/',substr($class,strlen($prefix))).'.php';
            if (is_file($file)) {require $file; return;}
        }
    }
});
foreach(json_decode(file_get_contents($directory.'/league-sha256.json'),true) as $file=>$sha) {
    if(hash_file('sha256',$directory.'/upstream/league__'.str_replace('/','__',$file))!==$sha) throw new Exception('Upstream SHA mismatch');
}
require $directory.'/upstream/league__src__RedirectUriValidators__RedirectUriValidatorInterface.php';
require $directory.'/upstream/league__src__RedirectUriValidators__RedirectUriValidator.php';
$rows=[];
foreach(json_decode(file_get_contents($directory.'/redirect-input.json'),true) as [$allowed,$requested]) {
 try {$result=(new League\OAuth2\Server\RedirectUriValidators\RedirectUriValidator([$allowed]))->validateRedirectUri($requested);}
 catch(Throwable $e) {$result=get_class($e);}
 $rows[]=['registered'=>$allowed,'requested'=>$requested,'result'=>$result];
}
echo json_encode(['php'=>PHP_VERSION,'rows'=>$rows],JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES)."\n";
