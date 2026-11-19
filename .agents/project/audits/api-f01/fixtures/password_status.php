<?php
// Execute the pinned, unmodified controller with minimal HTTP/clock boundaries.
namespace Illuminate\Routing { class Controller {} }
namespace Illuminate\Http {
    class Request {
        public function __construct(private $last, private $seconds) {}
        public function session() { return $this; }
        public function get($key, $default) { return $this->last; }
        public function input($key, $default) { return $this->seconds; }
    }
}
namespace Illuminate\Support\Facades {
    class Date {
        public static function now() { return new class { public function unix() { return 2000000000; } }; }
    }
}
namespace {
    function config($key, $default) { return $default; }
    function response() { return new class {
        public function json($data, $headers) { return ['body'=>$data, 'headers'=>$headers]; }
    }; }
    require $argv[1];
    $controller = new \Laravel\Fortify\Http\Controllers\ConfirmedPasswordStatusController;
    $values = [null,false,true,[],[1],['key'=>1],0,1,-1,950,951,950.5,1e30,
        '', '0','1','950','951','950.5',' 1000 ',"\t1e4\n",'z','1000x','950x',
        '9','-1','+951','1e9999','-1e9999','9223372036854775808','-9223372036854775809'];
    $cases = [];
    foreach ([-9223372036854775807,-1000,-1,0,1,9,950,951,2000000000,9223372036854775807] as $elapsed) {
        // The subtraction in the original controller must remain in the PHP int domain.
        if (2000000000 - $elapsed > PHP_INT_MAX) continue;
        foreach ($values as $seconds) {
            $last = 2000000000 - $elapsed;
            $cases[] = ['elapsed'=>$elapsed,'seconds'=>$seconds,'response'=>$controller->show(new \Illuminate\Http\Request($last,$seconds))];
        }
    }
    echo json_encode($cases, JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),"\n";
}
