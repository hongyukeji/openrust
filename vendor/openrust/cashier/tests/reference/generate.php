<?php
// Run against external, fixed official SDK source; never a production dependency.
$root = $argv[1] ?? throw new RuntimeException('Pass the audited upstream directory');
$manifest = json_decode(file_get_contents(__DIR__.'/source-sha256.json'), true, flags: JSON_THROW_ON_ERROR);
foreach ($manifest['files'] as $file => $expected) {
    if (hash_file('sha256', $root.'/'.$file) !== $expected) {
        throw new RuntimeException('Official SDK source fingerprint mismatch: '.$file);
    }
}
require $root.'/stripe-stripe-php/lib/Util/Util.php';
require $root.'/stripe-stripe-php/lib/WebhookSignature.php';
require $root.'/PaddleHQ-paddle-php-sdk/src/Notifications/Secret.php';
require $root.'/PaddleHQ-paddle-php-sdk/src/Notifications/PaddleSignature.php';
$timestamp = time();
$secret = 'whsec_fixture_secret';
$stripe = '{"id":"evt_reference","object":"event","type":"fixture.reference","livemode":false,"data":{"object":{"id":"fixture"}}}';
$paddle = '{"event_id":"evt_reference","event_type":"fixture.reference","data":{"id":"fixture"}}';
$method = new ReflectionMethod(Stripe\WebhookSignature::class, 'computeSignature');
$stripeHash = $method->invoke(null, "$timestamp.$stripe", $secret);
$stripeHeader = "t=$timestamp,v1=$stripeHash";
if (!Stripe\WebhookSignature::verifyHeader($stripe, $stripeHeader, $secret, 300)) { throw new RuntimeException('Stripe reference rejected'); }
$paddleHash = Paddle\SDK\Notifications\PaddleSignature::calculateHMAC("$timestamp:$paddle", $secret);
$paddleHeader = "ts=$timestamp;h1=$paddleHash";
if (!Paddle\SDK\Notifications\PaddleSignature::parse($paddleHeader)->verify($paddle, new Paddle\SDK\Notifications\Secret($secret))) { throw new RuntimeException('Paddle reference rejected'); }
echo json_encode(['stripe'=>['body'=>$stripe,'header'=>$stripeHeader],'paddle'=>['body'=>$paddle,'header'=>$paddleHeader]], JSON_THROW_ON_ERROR|JSON_UNESCAPED_SLASHES), "\n";
