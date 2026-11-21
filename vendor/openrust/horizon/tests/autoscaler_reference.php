<?php

// Run the unmodified pinned AutoScaler against small in-memory queue/pool fakes.
// Usage: php tests/autoscaler_reference.php /path/to/pinned/laravel-horizon
namespace Illuminate\Contracts\Queue { interface Factory {} }
namespace Laravel\Horizon\Contracts { interface MetricsRepository {} }
namespace Illuminate\Support {
    class Collection implements \Countable, \ArrayAccess {
        public function __construct(public array $items) {}
        public function count(): int { return count($this->items); }
        public function mapWithKeys($fn) {
            $items = [];
            foreach ($this->items as $key => $value) {
                foreach ($fn($value, $key) as $k => $v) { $items[$k] = $v; }
            }
            return new self($items);
        }
        public function map($fn) { return new self(array_map($fn, $this->items)); }
        public function sum($field) {
            return array_sum(array_map($field instanceof \Closure ? $field : fn ($v) => $v[$field], $this->items));
        }
        public function sort() { $items = $this->items; asort($items); return new self($items); }
        public function each($fn) { foreach ($this->items as $k => $v) { $fn($v, $k); } }
        public function offsetExists($offset): bool { return isset($this->items[$offset]); }
        public function offsetGet($offset): mixed { return $this->items[$offset]; }
        public function offsetSet($offset, $value): void { $this->items[$offset] = $value; }
        public function offsetUnset($offset): void { unset($this->items[$offset]); }
    }
}
namespace Laravel\Horizon {
    class Supervisor {
        public $processPools;
        public function __construct(public $options) {}
        public function pruneTerminatingProcesses() {}
        public function totalProcessCount() {
            return array_sum(array_map(fn ($p) => $p->totalProcessCount(), $this->processPools->items));
        }
    }
}
namespace {
    function collect($items) { return new \Illuminate\Support\Collection($items); }
    class Pool {
        public function __construct(public string $name, public int $active, public int $terminating = 0) {}
        public function queue() { return $this->name; }
        public function totalProcessCount() { return $this->active + $this->terminating; }
        public function scale($count) { $this->active = $count; }
    }
    class Queues implements \Illuminate\Contracts\Queue\Factory {
        public function __construct(public array $sizes) {}
        public function connection($name) { return $this; }
        public function readyNow($queue) { return $this->sizes[(int) $queue]; }
    }
    class Metrics implements \Laravel\Horizon\Contracts\MetricsRepository {
        public function __construct(public array $runtimes) {}
        public function runtimeForQueue($queue) { return $this->runtimes[(int) $queue]; }
    }
    $upstream = $argv[1] ?? throw new \RuntimeException('Provide pinned upstream source directory');
    require_once $upstream.'/src/SupervisorOptions.php';
    require_once $upstream.'/src/AutoScaler.php';
    class Scaler extends \Laravel\Horizon\AutoScaler {
        public function recommendations($supervisor) {
            return $this->numberOfWorkersPerQueue($supervisor,
                $this->timeToClearPerQueue($supervisor, $supervisor->processPools))->items;
        }
    }
    $scenarios = [
        ['name' => 'upstream-convergence', 'max' => 20, 'sizes' => [20, 10], 'runtimes' => [10, 10], 'current' => [10, 10], 'iterations' => 5],
        ['name' => 'upstream-empty', 'max' => 10, 'sizes' => [0, 0], 'runtimes' => [0, 0], 'current' => [5, 5], 'iterations' => 5],
        ['name' => 'upstream-busy', 'max' => 10, 'sizes' => [50, 0], 'runtimes' => [50, 0], 'current' => [1, 1], 'iterations' => 10],
        ['name' => 'upstream-minimum', 'max' => 5, 'min' => 2, 'sizes' => [1000, 1], 'runtimes' => [50, 1], 'current' => [3, 2], 'iterations' => 2],
        ['name' => 'upstream-max-shift', 'max' => 150, 'shift' => 10, 'sizes' => [600, 300], 'runtimes' => [75, 75], 'current' => [75, 75], 'iterations' => 5],
        ['name' => 'upstream-above-max', 'max' => 15, 'sizes' => [1, 1], 'runtimes' => [1, 1], 'current' => [16, 1], 'iterations' => 2],
        ['name' => 'upstream-size', 'max' => 100, 'strategy' => 'size', 'sizes' => [1000, 500], 'runtimes' => [10, 1000], 'current' => [50, 50], 'iterations' => 2],
        ['name' => 'upstream-logarithmic', 'max' => 10, 'strategy' => 'log', 'shift' => 10, 'sizes' => [999, 99, 99, 99, 9], 'runtimes' => [10, 10, 10, 10, 10], 'current' => [1, 1, 1, 1, 1]],
        ['name' => 'upstream-log-size-comparison', 'max' => 25, 'strategy' => 'size', 'shift' => 25, 'sizes' => [946, 13702, 0], 'runtimes' => [1, 1, 0], 'current' => [1, 1, 1]],
        ['name' => 'upstream-log-comparison', 'max' => 25, 'strategy' => 'log', 'shift' => 25, 'sizes' => [946, 13702, 0], 'runtimes' => [1, 1, 0], 'current' => [1, 1, 1]],
        ['name' => 'upstream-log-runtime-independent', 'max' => 3, 'strategy' => 'log', 'sizes' => [99, 9], 'runtimes' => [1, 1000], 'current' => [1, 1]],
        ['name' => 'upstream-priority', 'balance' => false, 'max' => 10, 'sizes' => [1], 'runtimes' => [0], 'current' => [10]],
        ['name' => 'upstream-priority-growing', 'balance' => false, 'max' => 10, 'sizes' => [11], 'runtimes' => [1000], 'current' => [5]],
        ['name' => 'unknown-runtime', 'max' => 10, 'sizes' => [5, 10], 'runtimes' => [0, 0], 'current' => [1, 1]],
        ['name' => 'simple-rounding', 'balance' => 'simple', 'max' => 8, 'sizes' => [0, 0, 0], 'runtimes' => [0, 0, 0], 'current' => [1, 1, 1], 'iterations' => 5],
        ['name' => 'terminating-counts', 'max' => 10, 'sizes' => [50, 0], 'runtimes' => [50, 0], 'current' => [3, 1], 'terminating' => [2, 2]],
    ];
    mt_srand(1340);
    for ($i = 0; $i < 180; $i++) {
        $count = mt_rand(1, 5);
        $scenarios[] = [
            'name' => 'generated-'.$i, 'max' => mt_rand($count * 2, 40), 'min' => mt_rand(1, 2),
            'shift' => mt_rand(1, 10), 'balance' => $count === 1 && $i % 4 === 0 ? false : ($i % 5 === 0 ? 'simple' : 'auto'),
            'strategy' => ['size', 'time', 'log'][$i % 3],
            'sizes' => array_map(fn () => mt_rand(0, 2000), range(1, $count)),
            'runtimes' => array_map(fn () => mt_rand(0, 2000) / 10, range(1, $count)),
            'current' => array_map(fn () => mt_rand(1, 10), range(1, $count)),
            'terminating' => array_map(fn () => mt_rand(0, 2), range(1, $count)),
        ];
    }
    $results = [];
    foreach ($scenarios as $scenario) {
        $options = new \Laravel\Horizon\SupervisorOptions('reference', 'redis', 'default');
        $options->balance = $scenario['balance'] ?? 'auto';
        $options->autoScalingStrategy = $scenario['strategy'] ?? 'time';
        $options->maxProcesses = $scenario['max'];
        $options->minProcesses = $scenario['min'] ?? 1;
        $options->balanceMaxShift = $scenario['shift'] ?? 1;
        $supervisor = new \Laravel\Horizon\Supervisor($options);
        $pools = [];
        foreach ($scenario['current'] as $k => $current) {
            $pools[$k] = new Pool((string) $k, $current, $scenario['terminating'][$k] ?? 0);
        }
        $supervisor->processPools = collect($pools);
        $scaler = new Scaler(new Queues($scenario['sizes']), new Metrics($scenario['runtimes']));
        for ($iteration = 0; $iteration < ($scenario['iterations'] ?? 1); $iteration++) {
            $row = $scenario;
            $row['current'] = array_values(array_map(fn ($pool) => $pool->active, $pools));
            $row['terminating'] = array_values(array_map(fn ($pool) => $pool->terminating, $pools));
            $recommendations = $scaler->recommendations($supervisor);
            $row['order'] = array_keys($recommendations);
            ksort($recommendations);
            $row['recommendations'] = array_values($recommendations);
            $scaler->scale($supervisor);
            $row['expected'] = array_values(array_map(fn ($pool) => $pool->active, $pools));
            $results[] = $row;
        }
    }
    echo json_encode(['commit' => 'b86d5aa87ca474db352ea2668bdd1df3a90bf8aa', 'cases' => $results], JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR), "\n";
}
