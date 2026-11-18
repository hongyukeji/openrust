<?php
// Regenerates the Rust printer fixture using the pinned PHP source and dependencies.
// Usage: php printer.php /path/to/vendor/autoload.php /path/to/pail /path/to/printer.json
require $argv[1];
$reference = $argv[2];
spl_autoload_register(function ($class) use ($reference) {
    $prefix = 'Laravel\\Pail\\';
    if (str_starts_with($class, $prefix)) {
        require $reference.'/src/'.str_replace('\\', '/', substr($class, strlen($prefix))).'.php';
    }
});
$data = json_decode(file_get_contents($argv[3]), false, 512, JSON_THROW_ON_ERROR);
assert($data->upstream === 'fdb73f5eacf03db576c710d5a00101ba185f2254');
foreach (['nunomaduro/termwind' => '2.3.3', 'symfony/console' => '7.3.6', 'illuminate/support' => '13.40.0'] as $package => $version) {
    if (ltrim(Composer\InstalledVersions::getPrettyVersion($package), 'v') !== $version) {
        throw new RuntimeException('Unexpected reference version: '.$package);
    }
}
unset($_ENV['PAIL_TESTS']);
putenv('PAIL_TESTS');
$data->php = PHP_VERSION;
foreach ($data->vectors as $vector) {
    putenv('COLUMNS='.$vector->width);
    $_ENV['COLUMNS'] = $vector->width;
    $output = new Symfony\Component\Console\Output\BufferedOutput([32, 64, 128][$vector->verbosity], false);
    $printer = new Laravel\Pail\Printers\CliPrinter($output, '/app/project');
    $message = Laravel\Pail\ValueObjects\MessageLogged::fromJson(json_encode($data->records[$vector->record], JSON_PRESERVE_ZERO_FRACTION));
    $printer->print($message);
    $plain = implode("\n", array_map('rtrim', explode("\n", $output->fetch())));
    $vector->output_sha256 = hash('sha256', $plain);
    $vector->options_sha256 = hash('sha256', $printer->optionsHtml($message));
    $vector->trace_sha256 = hash('sha256', $printer->traceHtml($message));
}
echo json_encode($data, JSON_PRETTY_PRINT | JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_PRESERVE_ZERO_FRACTION), "\n";
