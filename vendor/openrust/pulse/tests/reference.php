<?php
// Execute the unmodified pinned Pulse preaggregation methods with minimal clock/collection adapters.
namespace Illuminate\Support {
    class Collection implements \IteratorAggregate,\JsonSerializable {
        public function __construct(public array $items) {}
        public function getIterator():\Traversable {return new \ArrayIterator($this->items);}
        public function jsonSerialize():mixed {return $this->items;}
    }
}
namespace Carbon {
    class CarbonImmutable {
        public function __construct(public int $time=1795267800) {}
        public static function now():self {return new self;}
        public function subMinutes(int $minutes):self {return new self($this->time-$minutes*60);}
        public function getTimestamp():int {return $this->time;}
    }
}
namespace Laravel\Pulse\Contracts {interface Storage {}}
namespace {
    // Reject a different upstream revision before executing its code.
    $manifest = json_decode(file_get_contents(__DIR__.'/reference.sources.json'), true, 512, JSON_THROW_ON_ERROR);
    $sourceRoot = ($argv[1] ?? '');
    foreach ($manifest['files'] as $file => $sha256) {
        $source = $sourceRoot.'/'.$file;
        if (!is_file($source) || hash_file('sha256', $source) !== $sha256) {
            throw new \RuntimeException('Unexpected pinned reference source: '.$file.' at '.$manifest['commit']);
        }
    }

    function collect(array $items):\Illuminate\Support\Collection {return new \Illuminate\Support\Collection($items);}
    require $argv[1].'/src/Entry.php';
    require $argv[1].'/src/Storage/DatabaseStorage.php';
    class Storage extends \Laravel\Pulse\Storage\DatabaseStorage {
        public function __construct() {}
        protected function periods():array {return [60,360,1440,10080];}
        protected function requiresManualKeyHash():bool {return true;}
        public function aggregateEntries(array $items):array {
            $result=[];
            foreach (['Counts','Minimums','Maximums','Sums','Averages'] as $method) {
                array_push($result,...$this->{'preaggregate'.$method}(collect($items))->items);
            }
            return $result;
        }
    }
    $variants=[null,-9,0,5,100];$cases=[];$storage=new Storage;
    foreach($variants as $a)foreach($variants as $b)foreach($variants as $c){
        $key='case-'.count($cases);$items=[];
        foreach([$a,$b,$c] as $value)$items[]=new \Laravel\Pulse\Entry(1795267800,'reference',$key,$value);
        $cases[]=['key'=>$key,'values'=>[$a,$b,$c],'expected'=>$storage->aggregateEntries($items)];
    }
    echo json_encode(['commit'=>'33cc6fc68cd3052aa06227e88e8d933dd3d70a51','cases'=>$cases],JSON_THROW_ON_ERROR),"\n";
}
