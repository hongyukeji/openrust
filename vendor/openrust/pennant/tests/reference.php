<?php

// Unmodified pinned ArrayDriver and PendingScopedFeatureInteraction; minimal framework fakes.
// Usage: php reference.php /path/to/pinned/laravel-pennant
namespace Illuminate\Contracts\Events { interface Dispatcher { public function dispatch($event); } }
namespace Illuminate\Queue { trait SerializesModels {} }
namespace Illuminate\Support {
    class Collection {
        public function __construct(public array $items) {}
        public static function make($items) { return $items instanceof self ? $items : new self($items); }
        public static function wrap($items) { return new self($items === null ? [] : (is_array($items) ? $items : [$items])); }
        public function all() { return $this->items; }
        public function map($fn) { $result = []; foreach ($this->items as $k => $v) { $result[$k] = $fn($v, $k); } return new self($result); }
        public function pipe($fn) { return $fn($this); }
        public function crossJoin($items) { $result = []; foreach ($this->items as $a) { foreach ($items as $b) { $result[] = [$a, $b]; } } return new self($result); }
        public function every($fn) { foreach ($this->items as $k => $v) { if (!$fn($v, $k)) return false; } return true; }
        public function some($fn) { foreach ($this->items as $k => $v) { if ($fn($v, $k)) return true; } return false; }
    }
}
namespace Laravel\Pennant {
    class Feature {
        public static function serializeScope($scope) { return $scope === null ? '__laravel_null' : (string) $scope; }
    }
}
namespace {
    function with($value, $fn) { return $fn($value); }
    $root = $argv[1];
    foreach (['Contracts/CanListStoredFeatures.php', 'Contracts/Driver.php', 'Contracts/HasFlushableCache.php', 'Events/UnknownFeatureResolved.php', 'functions.php', 'Drivers/ArrayDriver.php', 'PendingScopedFeatureInteraction.php'] as $file) { require "$root/src/$file"; }
    class Events implements \Illuminate\Contracts\Events\Dispatcher { public function dispatch($event) {} }
    // The adapter only supplies the decorator's bulk signature; aggregation runs upstream.
    class Store {
        public function __construct(public $driver) {}
        public function getAllMissing($features) {
            $result = [];
            foreach ($features as $feature) { foreach ($feature['scope'] as $scope) { $result[$feature['feature']][] = $this->driver->get($feature['feature'], $scope); } }
            return $result;
        }
        public function get($feature, $scope) { return $this->driver->get($feature, $scope); }
    }
    $variants = [false, true, null, 0, '', [], ['value']];
    $cases = [];
    foreach ($variants as $a) foreach ($variants as $b) foreach ($variants as $c) foreach ($variants as $d) {
        $driver = new \Laravel\Pennant\Drivers\ArrayDriver(new Events, []);
        foreach ([['one', 'left', $a], ['two', 'left', $b], ['one', 'right', $c], ['two', 'right', $d]] as [$feature, $scope, $value]) { $driver->set($feature, $scope, $value); }
        // Actual ArrayDriver treats null as unresolved and returns false without a resolver.
        $pending = (new \Laravel\Pennant\PendingScopedFeatureInteraction(new Store($driver)))->for(['left', 'right']);
        $cases[] = [
            'values' => [$a, $b, $c, $d],
            'expected' => [$pending->allAreActive(['one', 'two']), $pending->someAreActive(['one', 'two']), $pending->allAreInactive(['one', 'two']), $pending->someAreInactive(['one', 'two'])],
        ];
    }
    echo json_encode(['commit' => '9046466feb0146b6223db8b8e9c589aeba23f7f0', 'cases' => $cases]), "\n";
}
