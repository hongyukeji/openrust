<?php
require '/app/vendor/autoload.php';
use Illuminate\Container\Container;
use Illuminate\Support\Facades\Facade;
use Illuminate\Support\Facades\Schema;
use Illuminate\Database\MySqlConnection;
$connection = new MySqlConnection(fn()=>throw new LogicException('Unexpected PDO request during pretend compilation'), 'reference', '', ['charset'=>'utf8mb4','collation'=>'utf8mb4_unicode_ci']);
$connection->useDefaultSchemaGrammar();
Facade::setFacadeApplication(new Container);
Schema::swap($connection->getSchemaBuilder());
$tables=[];
foreach(glob('/app/database/migrations/*.php') as $file){
    $migration=require $file;
    $tables[pathinfo($file, PATHINFO_FILENAME)]=array_map(fn($row)=>$row['query'], $connection->pretend(fn()=>$migration->up()));
}
$versions=[];
foreach(Composer\InstalledVersions::getInstalledPackages() as $package){
 if(str_starts_with($package,'illuminate/')){
  $version=Composer\InstalledVersions::getPrettyVersion($package);
  if($version!=='v13.40.0')throw new RuntimeException('Unpinned Illuminate version');
  $versions[$package]=$version;
 }
}
if(count($tables)!==5)throw new RuntimeException('Expected all five fixed Passport migrations');
echo json_encode(['passport'=>'f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7','php'=>PHP_VERSION,'illuminate'=>$versions,'migrations'=>$tables],JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),"\n";
