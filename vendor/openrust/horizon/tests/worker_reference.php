<?php

// Run the unmodified pinned Laravel gatherWorkerOptions method.
// Usage: php worker_reference.php /path/to/pinned/Illuminate/Queue/Console/WorkCommand.php
namespace Illuminate\Console {
    class Command {
        public array $options = [];
        public function option($name) { return $this->options[$name] ?? null; }
    }
}
namespace Illuminate\Support { trait InteractsWithTime {} }
namespace Illuminate\Queue {
    class WorkerOptions {
        public function __construct(public $name, public $backoff, ...$unused) {}
    }
}
namespace {
    // Reject a different upstream revision before executing its code.
    $manifest = json_decode(file_get_contents(__DIR__.'/worker_reference.sources.json'), true, 512, JSON_THROW_ON_ERROR);
    $sourceRoot = dirname($argv[1] ?? '');
    foreach ($manifest['files'] as $file => $sha256) {
        $source = $sourceRoot.'/'.$file;
        if (!is_file($source) || hash_file('sha256', $source) !== $sha256) {
            throw new \RuntimeException('Unexpected pinned reference source: '.$file.' at '.$manifest['commit']);
        }
    }

    require $argv[1];
    $class = new \ReflectionClass(\Illuminate\Queue\Console\WorkCommand::class);
    $method = $class->getMethod('gatherWorkerOptions');
    $cases = [];
    foreach (['0', '1', '01', '10', '4294967295', '1,3', '3,1', '01,05', '0,0', '10,20'] as $backoff) {
        foreach ([0, 1, 2, 10, 4294967295] as $delay) {
            $command = $class->newInstanceWithoutConstructor();
            $command->options = ['backoff' => $backoff, 'delay' => (string) $delay];
            $result = $method->invoke($command)->backoff;
            $cases[] = ['backoff' => $backoff, 'delay' => $delay, 'expected' => array_map('intval', explode(',', (string) $result))];
        }
    }
    echo json_encode(['commit' => '1449bf241017087fbd38c118ecaabef3f6797fdc', 'cases' => $cases], JSON_PRETTY_PRINT), "\n";
}
