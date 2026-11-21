<?php
// Calls the unmodified pinned FeatureManager::serializeScope with a minimal container.
namespace Illuminate\Contracts\Container { interface Container {} }
namespace {
    require $argv[1].'/src/Contracts/FeatureScopeSerializeable.php';
    require $argv[1].'/src/GlobalScope.php';
    require $argv[1].'/src/FeatureManager.php';
    $manager = new \Laravel\Pennant\FeatureManager(new class implements \Illuminate\Contracts\Container\Container {});
    $values = [null, '', '001', '1e3', 0, 1, -42, PHP_INT_MAX, PHP_INT_MIN,
        0.0, -0.0, 1.0, 0.000001, 0.0000001, 1e14, 1e15, 1e20, 1e-20,
        1.2345678901234567, -1.2345678901234567, 1.0/3.0, 2.2250738585072014e-308, 1.7976931348623157e308];
    $cases = array_map(fn ($scope) => ['scope'=>$scope, 'serialized'=>$manager->serializeScope($scope)], $values);
    echo json_encode(['commit'=>'9046466feb0146b6223db8b8e9c589aeba23f7f0', 'cases'=>$cases, 'global'=>$manager->serializeScope(new \Laravel\Pennant\GlobalScope)], JSON_PRETTY_PRINT | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR), "\n";
}
