<?php
namespace Illuminate\Support {
    class Collection {
        public function __construct(private $values) {}
        public static function times($count, $callback) { return new self(array_map($callback, range(1, $count))); }
        public function all() { return $this->values; }
    }
}
namespace Illuminate\Validation {
    class ValidationException extends \Exception {
        public $errors; public $bag;
        public static function withMessages($errors) { $error = new self; $error->errors = $errors; return $error; }
        public function errorBag($bag) { $this->bag = $bag; return $this; }
    }
}
namespace Laravel\Fortify {
    class Fortify {
        public static $recoveryCodeGenerator;
        public static function currentEncrypter() { return \Harness::$encrypter; }
        public static function confirmsTwoFactorAuthentication() { return true; }
        public static function username() { return 'email'; }
    }
}
namespace Laravel\Fortify\Events {
    class AccountEvent {
        public static function dispatch($user, $code = null) { \Harness::$events[] = substr(strrchr(static::class, '\\'), 1); }
    }
    class TwoFactorAuthenticationEnabled extends AccountEvent {}
    class TwoFactorAuthenticationConfirmed extends AccountEvent {}
    class TwoFactorAuthenticationDisabled extends AccountEvent {}
    class RecoveryCodesGenerated extends AccountEvent {}
    class RecoveryCodeReplaced extends AccountEvent {}
}
namespace {
    function config($key, $default = null) { return $key === 'fortify-options.two-factor-authentication.secret-length' ? ' 16x' : $default; }
    function __($message) { return $message; }
    function now() { return 'CONFIRMED'; }
    class Harness {
        public static $encrypter; public static $events = []; public static $generated = []; public static $counter = 0;
    }
    class Encrypter {
        public function encrypt($value) { return 'fixture:'.base64_encode($value); }
        public function decrypt($value) { return base64_decode(substr($value, 8)); }
    }
    Harness::$encrypter = new Encrypter;
    $root = $argv[1];
    require $root.'/src/Contracts/TwoFactorAuthenticationProvider.php';
    require $root.'/src/TwoFactorAuthenticatable.php';
    require $root.'/src/RecoveryCode.php';
    foreach (['EnableTwoFactorAuthentication','ConfirmTwoFactorAuthentication','DisableTwoFactorAuthentication','GenerateNewRecoveryCodes'] as $action) require $root.'/src/Actions/'.$action.'.php';
    class Provider implements \Laravel\Fortify\Contracts\TwoFactorAuthenticationProvider {
        public function generateSecretKey($length = 16) { Harness::$generated[] = $length; return 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ'; }
        public function verify($secret, $code) { return $secret === 'GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ' && $code === '287082'; }
        public function qrCodeUrl($companyName, $companyEmail, $secret) { throw new \LogicException('QR URI covered by the separate provider oracle'); }
    }
    class Model {
        use \Laravel\Fortify\TwoFactorAuthenticatable;
        public $two_factor_secret; public $two_factor_recovery_codes; public $two_factor_confirmed_at;
        public function forceFill($attributes) { foreach ($attributes as $key => $value) $this->$key = $value; return $this; }
        public function save() { return true; }
    }
    $model = new Model; $provider = new Provider;
    $enable = new \Laravel\Fortify\Actions\EnableTwoFactorAuthentication($provider);
    $confirm = new \Laravel\Fortify\Actions\ConfirmTwoFactorAuthentication($provider);
    $disable = new \Laravel\Fortify\Actions\DisableTwoFactorAuthentication;
    $generate = new \Laravel\Fortify\Actions\GenerateNewRecoveryCodes;
    \Laravel\Fortify\Fortify::$recoveryCodeGenerator = fn() => 'code-'.Harness::$counter++;
    $cases = [];
    $steps = [['status'],['enable'],['enable'],['confirm','000000'],['confirm','0'],['confirm','287082'],['replace','code-0'],['generate'],['enable',true],['disable'],['disable'],['unicode-generate'],['unicode-replace','é/😀\\x']];
    foreach ($steps as $step) {
        $status = 200;
        try {
            match ($step[0]) {
                'enable' => $enable($model, $step[1] ?? false),
                'confirm' => $confirm($model, $step[1]),
                'replace', 'unicode-replace' => $model->replaceRecoveryCode($step[1]),
                'generate' => $generate($model),
                'unicode-generate' => (function () use ($model, $generate) { \Laravel\Fortify\Fortify::$recoveryCodeGenerator = fn() => 'é/😀\\x'; $generate($model); })(),
                'disable' => $disable($model),
                default => null,
            };
            $response = ['codes_json'=>$model->two_factor_recovery_codes === null ? null : Harness::$encrypter->decrypt($model->two_factor_recovery_codes), 'secret'=>$model->two_factor_secret === null ? null : Harness::$encrypter->decrypt($model->two_factor_secret), 'codes'=>$model->two_factor_recovery_codes === null ? null : $model->recoveryCodes(), 'enabled'=>$model->hasEnabledTwoFactorAuthentication(), 'confirmed'=>$model->two_factor_confirmed_at !== null, 'generated'=>Harness::$generated, 'events'=>Harness::$events];
        } catch (\Illuminate\Validation\ValidationException $error) {
            $status = 422; $response = ['errors'=>$error->errors];
        }
        $cases[] = ['step'=>$step,'status'=>$status,'response'=>$response];
    }
    echo json_encode($cases, JSON_PRETTY_PRINT|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR),"\n";
}
