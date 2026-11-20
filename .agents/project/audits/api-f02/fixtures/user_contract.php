<?php
// Execute the pinned upstream trait. Only Eloquent attribute/key reads and
// Config::string are stubs; no database or WebAuthn behavior is simulated.
namespace Illuminate\Support\Facades {
    class Config { public static string $secret; public static function string(string $key): string { return self::$secret; } }
}
namespace {
    require '/upstream/src/PasskeyAuthenticatable.php';
    class User {
        use \Laravel\Passkeys\PasskeyAuthenticatable;
        public function __construct(public array $input) {}
        public function getTable() { return $this->input['table']; }
        public function getKey() { return $this->input['id']; }
        public function getAuthIdentifier() { return $this->getKey(); }
        public function getAttribute($name) { return $this->input[$name] ?? null; }
    }
    $cases = [];
    foreach ([1, 42, '01', '用户-id'] as $id) {
        foreach (['app-key', 'base64:YWJjZA==', '密钥'] as $secret) {
            foreach ([[], ['email'=>'a@example.test'], ['name'=>'名字','email'=>'a@example.test'], ['name'=>'','email'=>'']] as $attributes) {
                $input = ['table'=>'users', 'id'=>$id, 'secret'=>$secret] + $attributes;
                \Illuminate\Support\Facades\Config::$secret = $secret;
                $user = new User($input);
                $cases[] = ['input'=>$input, 'handle'=>$user->getPasskeyUserHandle(), 'display'=>$user->getPasskeyDisplayName(), 'username'=>$user->getPasskeyUsername()];
            }
        }
    }
    echo json_encode($cases, JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_THROW_ON_ERROR), "\n";
}
