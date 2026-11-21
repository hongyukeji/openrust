import assert from 'node:assert/strict';
import { test } from 'node:test';
import { assertCommandEntrypoints } from './command_entrypoints.mjs';

test('rejects build directory Artisan commands across paths and shells', () => {
    const commands = [
        './target/debug/main artisan horizon:status',
        'target/debug/main artisan horizon',
        './target/release/main artisan migrate',
        './target/aarch64-apple-darwin/release/main artisan list',
        '/srv/app/target/debug/main artisan queue:work',
        'APP_ENV=production ./target/release/main artisan horizon',
        'Run `./target/debug/main artisan pail`.',
        '"./target/debug/main" artisan list',
        '.\\target\\debug\\main.exe artisan pail',
        'target\\x86_64-pc-windows-msvc\\release\\main.exe artisan list',
        'C:\\app\\target\\release\\main.exe artisan list',
        '& ".\\target\\debug\\main.exe" artisan list',
        './target/debug/main \\\n  artisan list',
        '.\\target\\debug\\main.exe `\r\n  artisan list',
    ];
    for (const command of commands) {
        assert.throws(() => assertCommandEntrypoints(command, 'example.md'), /example\.md:\d+: use \.\/artisan/, command);
    }
});

test('allows public entrypoints and build artifact explanations', () => {
    const examples = [
        './artisan horizon:status',
        './main artisan horizon:status',
        '.\\artisan.bat pail --timeout=0',
        '.\\main.exe artisan list',
        'cargo build --release --locked --features standalone',
        'cp target/release/main release/main',
        'cp ./target/release/main ./main',
        'Copy-Item target/release/main.exe release/',
        'Place `target/release/main` in the deployment root.',
        'The artifact is `target/release/main.exe`; run `./main artisan list`.',
    ];
    for (const example of examples) {
        assert.doesNotThrow(() => assertCommandEntrypoints(example, 'example.md'), example);
    }
});
