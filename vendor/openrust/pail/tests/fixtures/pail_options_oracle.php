<?php
namespace Illuminate\Console {
    class Command {
        public function __construct(private array $values) {}
        public function option($name) { return $this->values[$name] ?? null; }
    }
}
namespace {
    require '/ref/src/Options.php';
    require '/ref/src/ValueObjects/MessageLogged.php';
    $records=[];
    foreach ([12, '012', null] as $id) {
        $records[]=['message'=>"Import CRITICAL report 你好 🚀\nnext",'datetime'=>'2026-11-19T03:04:05.123456+08:00',
            'level_name'=>'CRITICAL','context'=>['z'=>'North','a'=>'South','number'=>1.0,'exponent'=>1e17,'fraction'=>.00001,
            '__pail'=>['origin'=>['type'=>'http','method'=>'GET','path'=>'reports/weekly','auth_id'=>$id,'auth_email'=>'user@example.test']]]];
    }
    $vectors=[];
    $auths=[null,'12','012',''];
    foreach ($records as $index=>$record) {
        $message=\Laravel\Pail\ValueObjects\MessageLogged::fromJson(json_encode($record));
        foreach ([null,'NoRtH','CRITICAL','user@example.test','你好','\\u4f60','"z":"North","a":"South"','"number":1,','"exponent":1.0e+17','"fraction":1.0e-5'] as $filter) {
            foreach ($auths as $auth) {
                $options=new \Laravel\Pail\Options(3600,$auth,null,$filter,null);
                $vectors[]=['record'=>$index,'auth'=>$auth,'level'=>null,'filter'=>$filter,'message'=>null,'accepts'=>$options->accepts($message)];
            }
        }
        foreach ([null,'critical','CRitiCAL','error',''] as $level) {
            foreach ([null,'IMPORT','North','你好','İmport',''] as $filter) {
                $options=new \Laravel\Pail\Options(3600,null,$level,null,$filter);
                $vectors[]=['record'=>$index,'auth'=>null,'level'=>$level,'filter'=>null,'message'=>$filter,'accepts'=>$options->accepts($message)];
            }
        }
    }
    $timeouts=[];
    foreach (['3600','','junk','12junk','1.9','1e2','1e2junk','-1.9','1e309','-1e309','1e20','-1e20','9223372036854775807','9223372036854775808','-9223372036854775809','  .5foo','+12rest','1e','0x10','-.5','1.9999999999999999'] as $value) {
        $options=\Laravel\Pail\Options::fromCommand(new \Illuminate\Console\Command(['timeout'=>$value]));
        $timeouts[]=['input'=>$value,'value'=>$options->timeout()];
    }
    $result=['upstream'=>'fdb73f5eacf03db576c710d5a00101ba185f2254','php_version'=>PHP_VERSION,
      'records'=>array_map(fn($r)=>json_encode($r),$records),
      'strings'=>array_map(fn($r)=>(string)\Laravel\Pail\ValueObjects\MessageLogged::fromJson(json_encode($r)),$records),
      'filters'=>$vectors,'timeouts'=>$timeouts];
    $result['filters']=array_map(fn($row)=>array_values($row),$result['filters']);
    $result['timeouts']=array_map(fn($row)=>[$row['input'],$row['value']],$result['timeouts']);
    $keys=array_keys($result);
    echo "{\n";
    foreach ($result as $key=>$value) {
        echo '  ',json_encode($key),': ';
        if (is_array($value)) {
            echo "[\n";
            foreach ($value as $index=>$row) {
                echo '    ',json_encode($row,JSON_UNESCAPED_SLASHES),$index<count($value)-1?',':'',"\n";
            }
            echo '  ]';
        } else { echo json_encode($value); }
        echo $key!==end($keys)?',':'',"\n";
    }
    echo "}\n";
}
