<?php
// Invoke EasyWeChat's actual MiniApp Utils; host stubs only replace its HTTP transport.
// MIT, w7corp/easywechat ba8022698a613e37b10ed1e4db604c5a96af0a63.
namespace EasyWeChat\Kernel\Exceptions { class HttpException extends \RuntimeException {} }
namespace EasyWeChat\MiniApp {
    class Application {
        public array $calls = [];
        public function __construct(public array $response) {}
        public function getAccount() { return $this; }
        public function getAppId() { return 'mini-app'; }
        public function getSecret() { return 'private-secret'; }
        public function getHttpClient() { return $this; }
        public function request($method, $url, $options) {
            $this->calls[] = compact('method', 'url', 'options');
            return $this;
        }
        public function toArray($throw) { return $this->response; }
    }
}
namespace {
    $source = $argv[1];
    if (hash_file('sha256', $source) !== 'de24fde19d0210fcd1fdb22462111b1d60d591deef299ae8cbaec7d9fa500f21') {
        throw new \RuntimeException('Unexpected EasyWeChat reference source');
    }
    require $source;
    $result = [];
    foreach ([
        'union' => ['openid' => 'id-one', 'session_key' => 'private-session', 'unionid' => 'union-one'],
        'openid' => ['openid' => 'id-one', 'session_key' => 'private-session'],
    ] as $name => $response) {
        $app = new \EasyWeChat\MiniApp\Application($response);
        $utils = new \EasyWeChat\MiniApp\Utils($app);
        $session = $utils->codeToSession('code+&中');
        $result[$name] = ['calls' => $app->calls, 'session' => $session];
    }
    $app = new \EasyWeChat\MiniApp\Application(['errcode' => 40029, 'errmsg' => 'invalid code']);
    try {
        (new \EasyWeChat\MiniApp\Utils($app))->codeToSession('invalid');
        throw new \RuntimeException('Expected code rejection');
    } catch (\EasyWeChat\Kernel\Exceptions\HttpException $e) {
        $result['rejected'] = true;
    }
    echo json_encode($result, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR)."\n";
}
