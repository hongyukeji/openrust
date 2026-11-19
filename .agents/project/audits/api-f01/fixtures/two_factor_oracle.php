<?php
// Harness only: controlled config/clock and an observable get/put cache. All OTP,
// secret validation, QR and provider decisions execute the original pinned sources.
namespace Illuminate\Contracts\Cache { interface Repository {} }
namespace {
$root = $argv[1];
foreach (['upstream', 'google2fa', 'constant_time_encoding'] as $dir) {
    $manifest = json_decode(file_get_contents($root.'/'.match ($dir) {
        'upstream' => 'upstream-manifest.json', 'google2fa' => 'google2fa-manifest.json',
        default => 'encoding-manifest.json',
    }), true, flags: JSON_THROW_ON_ERROR);
    foreach ($manifest['files'] as $path => $sha) {
        if (hash_file('sha256', "$root/$dir/$path") !== $sha) {
            throw new \RuntimeException("Source hash mismatch: $dir/$path");
        }
    }
}
spl_autoload_register(function ($class) use ($root) {
    foreach ([
        'Laravel\\Fortify\\' => "$root/upstream/src/",
        'PragmaRX\\Google2FA\\' => "$root/google2fa/src/",
        'ParagonIE\\ConstantTime\\' => "$root/constant_time_encoding/src/",
    ] as $prefix => $dir) {
        if (str_starts_with($class, $prefix)) {
            require $dir.str_replace('\\', '/', substr($class, strlen($prefix))).'.php';
            return;
        }
    }
});
function config($key, $default = null) {
    if (is_array($key)) { foreach ($key as $name=>$value) $GLOBALS['configuration'][$name]=$value; return; }
    if ($key === 'fortify-options.two-factor-authentication.window') return $GLOBALS['window'];
    return array_key_exists($key, $GLOBALS['configuration'] ?? []) ? $GLOBALS['configuration'][$key] : $default;
}
function optional($value) { return new class($value) {
    public function __construct(private $value) {}
    public function __call($name, $args) { return $this->value?->$name(...$args); }
}; }
class ClockEngine extends \PragmaRX\Google2FA\Google2FA {
    public int $calls = 0;
    public function __construct(public array $seconds) {}
    public function getTimestamp() {
        return (int) floor($this->seconds[min($this->calls++, count($this->seconds)-1)] / 30);
    }
}
class RecordingCache implements \Illuminate\Contracts\Cache\Repository {
    public array $log = [];
    public function __construct(public $old, public string $failure = '') {}
    public function get($key) {
        $this->log[] = ['get', $key];
        if ($this->failure === 'get') throw new \RuntimeException('cache-get');
        return $this->old;
    }
    public function put($key, $value, $ttl) {
        $this->log[] = ['put', $key, $value, $ttl];
        if ($this->failure === 'put') throw new \RuntimeException('cache-put');
        $this->old = $value;
        return $this->failure !== 'put-false';
    }
}
$secret = 'JBSWY3DPEHPK3PXP';
$generator = new ClockEngine([0]);
$inputs = [];
foreach ([0, 1, 59, 1234567890, 128849018880, -30] as $seconds) {
    $step = (int) floor($seconds/30);
    foreach ([-1, 0, 1, 2] as $window) {
        foreach ([-2, -1, 0, 1, 2] as $offset) {
            foreach ([null, $step-2, $step, $step+3] as $old) {
                $inputs[] = compact('secret', 'seconds', 'window', 'old') + [
                    'code' => $generator->oathTotp($secret, $step+$offset),
                    'next' => $seconds+30, 'cache' => true, 'failure' => '',
                ];
            }
        }
    }
}
foreach (['', '!!!!!!!', '!!!!!!!!', 'AAAAAAAA', 'AAAAAAAAA', 'AAAAAAAAAAAAAAA',
          'AAAAAAAAAAAAAA!A', 'jbswy3dpehpk3pxp', $secret."\n", str_repeat('A', 32)] as $key) {
    foreach ([-1, 0, 1] as $window) {
        foreach (['', '0', '123456', '1234567', ' 123456', "123456\n", '１２３４５６'] as $code) {
            $inputs[] = ['secret'=>$key,'seconds'=>1234567890,'next'=>1234567920,
                         'window'=>$window,'old'=>null,'code'=>$code,'cache'=>true,'failure'=>''];
        }
    }
}
foreach ([null, 0, false, true, '1', 1.5, 2] as $window) {
    foreach (['', 'get', 'put', 'put-false'] as $failure) {
        foreach ([true, false] as $cache) {
            $inputs[] = ['secret'=>$secret,'seconds'=>1234567890,'next'=>1234567920,
                'window'=>$window,'old'=>null,'code'=>$generator->oathTotp($secret,41152263),
                'cache'=>$cache,'failure'=>$failure];
        }
    }
}
$result = [];
foreach ($inputs as $input) {
    $GLOBALS['window'] = $input['window'];
    $engine = new ClockEngine([$input['seconds'], $input['next']]);
    $cache = new RecordingCache($input['old'], $input['failure']);
    $provider = new \Laravel\Fortify\TwoFactorAuthenticationProvider($engine, $input['cache'] ? $cache : null);
    try { $out = ['ok'=>$provider->verify($input['secret'], $input['code'])]; }
    catch (\Throwable $error) { $out = ['error'=>str_contains($error->getMessage(), 'cache-')
        ? $error->getMessage() : (new \ReflectionClass($error))->getShortName()]; }
    $result[] = $input + $out + ['calls'=>$engine->calls,'log'=>$cache->log,'window_after'=>$engine->getWindow()];
}
$qrs = [];
$provider = new \Laravel\Fortify\TwoFactorAuthenticationProvider(new ClockEngine([0]));
foreach ([['Open Rust','a+b@example.test',$secret],['公司 / :','邮箱@example.test','RAW+/?='],['','','']] as $input) {
    $qrs[] = ['input'=>$input,'url'=>$provider->qrCodeUrl(...$input)];
}
$lengths = [];
foreach ([0,1,7,8,9,15,16,17,31,32,64] as $length) {
    try { $key = $provider->generateSecretKey($length); $out = ['length'=>strlen($key),'valid'=>(bool)preg_match('/^[A-Z2-7]+$/D',$key)]; }
    catch (\Throwable $error) { $out = ['error'=>(new \ReflectionClass($error))->getShortName()]; }
    $lengths[] = ['input'=>$length] + $out;
}
$features = [];
foreach ([[], ['registration'], ['two-factor-authentication'], ['update-profile-information'],
          ['update-passwords'], ['passkeys'], [true], [false], [0], [null], ['0e10'],
          ['flag'=>true], ['flag'=>'registration'], null, false, 'registration', 1] as $list) {
    foreach ([true, false, 1, 'true', null] as $option) {
        $GLOBALS['configuration'] = ['fortify.features'=>$list, 'fortify-options.two-factor-authentication.confirm'=>$option];
        $out = [];
        foreach (['registration','0e12','', '0'] as $feature) {
            try { $out['enabled'][] = \Laravel\Fortify\Features::enabled($feature); }
            catch (\Throwable $error) { $out['enabled'][] = 'error'; }
        }
        foreach (['hasProfileFeatures','hasSecurityFeatures','canUpdateProfileInformation','canUpdatePasswords','canManageTwoFactorAuthentication','canManagePasskeys'] as $method) {
            try { $out[$method] = \Laravel\Fortify\Features::$method(); }
            catch (\Throwable $error) { $out[$method] = 'error'; }
        }
        try { $out['option'] = \Laravel\Fortify\Features::optionEnabled('two-factor-authentication','confirm'); }
        catch (\Throwable $error) { $out['option'] = 'error'; }
        $features[] = ['list'=>$list, 'option'=>$option,'expected'=>$out];
    }
}
echo json_encode(['php'=>PHP_VERSION,'cases'=>$result,'qr'=>$qrs,'lengths'=>$lengths,'features'=>$features], JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR)."\n";
}
