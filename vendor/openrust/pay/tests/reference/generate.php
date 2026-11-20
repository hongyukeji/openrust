<?php
// Fixed official SDK protocol fixtures. Source files are external audit inputs.
// Test keys only. Run from any working directory: php path/to/generate.php
$root = dirname(__DIR__);
$upstream = getenv('PAY_REFERENCE_ROOT');
if (!$upstream) {throw new RuntimeException('PAY_REFERENCE_ROOT is required');}
foreach (json_decode(file_get_contents(__DIR__.'/source-sha256.json'),true) as $file=>$sha) {if (hash_file('sha256',$upstream.'/'.$file)!==$sha) {throw new RuntimeException('Official source mismatch: '.$file);}}
set_include_path($upstream.'/alipay-aop'.PATH_SEPARATOR.get_include_path());
require $upstream.'/alipay-aop/AopClient.php';
require $upstream.'/wechatpay-apiv3-wechatpay-php-src_Crypto_AesInterface.php';
require $upstream.'/wechatpay-apiv3-wechatpay-php-src_Crypto_AesGcm.php';

$params = ['app_id'=>'app123','seller_id'=>'2088123456','out_trade_no'=>'order_001','trade_no'=>'ali_reference','total_amount'=>'12.34','trade_status'=>'TRADE_SUCCESS','sign_type'=>'RSA2','subject'=>'订单 & coffee=1'];
ksort($params);
$client = new AopClient();
$client->rsaPrivateKeyFilePath = $root.'/fixtures/platform.pem';
$unsigned=$params;unset($unsigned['sign_type']);
$params['sign']=$client->rsaSign($unsigned,'RSA2');
$transaction=['appid'=>'app123','mchid'=>'1900001234','out_trade_no'=>'order_001','transaction_id'=>'wx_reference','trade_state'=>'SUCCESS','amount'=>['total'=>1234,'currency'=>'CNY']];
$nonce='fixture-nonc';$aad='transaction';
$plaintext=json_encode($transaction,JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES);
$cipher=\WeChatPay\Crypto\AesGcm::encrypt($plaintext,'0123456789abcdef0123456789abcdef',$nonce,$aad);
file_put_contents(getenv('PAY_REFERENCE_OUTPUT') ?: $root.'/fixtures/php-reference.json',json_encode(['alipay_form'=>http_build_query($params,'','&',PHP_QUERY_RFC1738),'wechat_resource'=>['algorithm'=>'AEAD_AES_256_GCM','original_type'=>'transaction','associated_data'=>$aad,'nonce'=>$nonce,'ciphertext'=>$cipher]],JSON_PRETTY_PRINT|JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES)."\n");
