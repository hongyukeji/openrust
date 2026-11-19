<?php
// Run against the exact Passport source directory passed as the first argument.
foreach ([
    'Passport.php' => '10e8efa4ba67b760ea949f17b9aad208e04c9894788430eeb7eae2728b7ac190',
    'ResolvesInheritedScopes.php' => '11e212c5191f7ba80127c8f7abbbc9f38485776d1869083f67d4daddd14ff8c5',
] as $file => $expected) {
    if (hash_file('sha256', $argv[1].'/'.$file) !== $expected) {
        throw new RuntimeException('Unexpected Passport reference source');
    }
}
require $argv[1].'/Passport.php';
require $argv[1].'/ResolvesInheritedScopes.php';
class ScopeProbe
{
    use Laravel\Passport\ResolvesInheritedScopes;
    public function allows(string $scope, array $restrictions): bool
    {
        return $this->scopeExistsIn($scope, $restrictions);
    }
}
$probe = new ScopeProbe;
$cases = [];
foreach ([false, true] as $inherited) {
    Laravel\Passport\Passport::$withInheritedScopes = $inherited;
    foreach (['orders', 'orders:read', 'orders:read:edit', 'orders:', ':orders', '', ':', 'a::b', '*', '0', '00', '账:读'] as $scope) {
        foreach ([[], ['orders'], ['orders:read'], [''], ['a:'], ['*'], [0], [true], [false], [null], ['named' => 'orders'], [['orders']], ['账']] as $restrictions) {
            $cases[] = ['scope' => $scope, 'scopes' => $restrictions, 'inherited' => $inherited,
                'allowed' => $probe->allows($scope, $restrictions)];
        }
    }
}
echo json_encode(['passport' => 'f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7',
    'php' => PHP_VERSION, 'trait_sha256' => hash_file('sha256', $argv[1].'/ResolvesInheritedScopes.php'),
    'cases' => $cases], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES | JSON_PRETTY_PRINT | JSON_THROW_ON_ERROR), "\n";
