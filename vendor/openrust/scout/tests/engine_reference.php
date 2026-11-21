<?php
// Execute the pinned Engine, CollectionEngine, Builder and Str. Only the SQL/model
// transport and collection container are adapters; no search logic is duplicated.
// php engine_reference.php /source/laravel-scout /source/laravel-framework
namespace Illuminate\Support\Traits { trait Macroable {} trait Conditionable {} trait Tappable {} }
namespace Illuminate\Support {
    class Collection implements \Countable {
        public function __construct(public array $items = []) {}
        public function count(): int { return count($this->items); }
        public function all() { return $this->items; }
        public function values() { return new self(array_values($this->items)); }
        public function first() { return reset($this->items); }
        public function firstWhere($key, $value) { foreach ($this->items as $item) if ($item[$key] === $value) return $item; return null; }
        public function filter($fn) { return new self(array_filter($this->items, $fn)); }
        public function take($limit) { return $limit === null ? $this : new self(array_slice($this->items, 0, $limit)); }
        public function forPage($page, $size) { return new self(array_slice($this->items, ($page-1)*$size, $size)); }
        public function pluck($key) { return new self(array_map(fn($item) => $item->$key, $this->items)); }
        public function sortBy($fn) { $items=$this->items; uasort($items, fn($a,$b)=>$fn($a)<=>$fn($b)); return new self($items); }
    }
}
namespace {
    $manifest = json_decode(file_get_contents(__DIR__.'/engine_reference.sources.json'), true, 512, JSON_THROW_ON_ERROR);
    foreach ($manifest['sources'] as $index => $source) {
        $root = $argv[$index+1] ?? '';
        foreach ($source['files'] as $file => $sha) {
            if (!is_file($root.'/'.$file) || hash_file('sha256', $root.'/'.$file) !== $sha) {
                throw new \RuntimeException('Unexpected pinned reference source: '.$file.' at '.$source['commit']);
            }
        }
    }
    require $argv[2].'/src/Illuminate/Support/Str.php';
    require $argv[1].'/src/Builder.php';
    require $argv[1].'/src/Engines/Engine.php';
    require $argv[1].'/src/Engines/CollectionEngine.php';
    function collect($items = []) { return new \Illuminate\Support\Collection($items); }
    function class_uses_recursive($class) { return []; }
    function config($key, $default = null) { return $default; }
    class Query {
        private array $orders=[];
        public function __construct(private array $items) {}
        public function when($condition, $yes, $no=null) { if ($condition) $yes($this); elseif ($no) $no($this); return $this; }
        public function where($key, $op, $value) {
            $this->items=array_filter($this->items, fn($m)=> match($op) {'='=>$m->$key==$value, '>'=>$m->$key>$value, default=>throw new \RuntimeException('unsupported fixture operator')}); return $this;
        }
        public function whereIn($key,$values) { $this->items=array_filter($this->items,fn($m)=>in_array($m->$key,$values)); return $this; }
        public function whereNotIn($key,$values) { $this->items=array_filter($this->items,fn($m)=>!in_array($m->$key,$values)); return $this; }
        public function orderBy($key,$direction) { $this->orders[]=[$key,$direction]; return $this; }
        public function get() {
            $items=$this->items;
            usort($items,function($a,$b) { foreach($this->orders as [$key,$direction]) { $cmp=$a->$key<=>$b->$key; if ($cmp) return $direction==='asc'?$cmp:-$cmp; } return 0; });
            return collect($items);
        }
    }
    class Model {
        public static array $records=[];
        public function __construct(public int $id=0, public string $title='', public string $status='published', public int $category=1) {}
        public function query() { return new Query(self::$records); }
        public function qualifyColumn($key) { return $key; }
        public function getScoutKeyName() { return 'id'; }
        public function getScoutKey() { return $this->id; }
        public function makeSearchableUsing($models) { return $models; }
        public function shouldBeSearchable() { return $this->status==='published'; }
        public function toSearchableArray() { return ['title'=>$this->title]; }
        public function newCollection() { return collect(); }
        // Deliberately return reversed order: map must restore the engine ordering.
        public function getScoutModelsByIds($builder,$ids) { return collect(array_reverse(array_filter(self::$records,fn($m)=>in_array($m->id,$ids)))); }
    }
    Model::$records=[new Model(1,'ÄPFEL'),new Model(2,'ÉCOLE','published',2),new Model(3,'ΑΘΉΝΑ'),new Model(4,'МОСКВА','published',2),new Model(5,'Rust'),new Model(6,'rust','draft')];
    $engine=new \Laravel\Scout\Engines\CollectionEngine;
    $cases=[];
    foreach(['äpfel','école','αθήνα','москва','RUST','','missing'] as $query) {
        foreach(['all','in','not_in','where','limit','page'] as $mode) {
            $builder=new \Laravel\Scout\Builder(new Model,$query);
            $builder->orderBy('id','asc');
            if($mode==='in')$builder->whereIn('category',[2]);
            if($mode==='not_in')$builder->whereNotIn('category',[2]);
            if($mode==='where')$builder->where('id','>',2);
            if($mode==='limit')$builder->take(2);
            $raw=$mode==='page'?$engine->paginate($builder,2,2):$engine->search($builder);
            $cases[]=['query'=>$query,'mode'=>$mode,'ids'=>$engine->mapIds($raw)->all(),'mapped'=>$engine->map($builder,$raw,new Model)->pluck('id')->all(),'total'=>$engine->getTotalCount($raw)];
        }
    }
    echo json_encode(['records'=>array_map(fn($m)=>get_object_vars($m),Model::$records),'cases'=>$cases],JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE),"\n";
}
