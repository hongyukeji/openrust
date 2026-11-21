import assert from 'node:assert/strict';

// Build artifact locations may be documented, but Artisan must use public entrypoints.
const buildArtifactCommand = /(?:^|[\s`"'&])((?:[^\s`"'&]*[/\\])?target[/\\](?:[^\s`"'&/\\]+[/\\])*(?:debug|release)[/\\][^\s`"'&]+)["']?(?:[\t ]|\\\r?\n|`\r?\n)+artisan\b/im;

export function assertCommandEntrypoints(text, file) {
    const match = buildArtifactCommand.exec(text);
    if (!match) return;
    const line = text.slice(0, match.index).split('\n').length;
    assert.fail(`${file}:${line}: use ./artisan for source commands or ./main artisan after deployment; build artifact command: ${match[1]} artisan`);
}
