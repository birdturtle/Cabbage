import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const read = path => readFileSync(new URL('../' + path, import.meta.url), 'utf8');
const config = JSON.parse(read('src-tauri/tauri.conf.json'));
assert.equal(config.identifier, 'com.birdturtle.cabbage');
assert.equal(config.productName, 'Cabbage');

const storage = read('src-tauri/src/storage.rs');
assert.ok(!storage.includes('com.hapticscape'));
assert.ok(storage.includes('com.birdturtle.cabbage.jagex.v1'));

const desktop = read('linux/install-desktop-entry.sh');
assert.ok(desktop.includes('com.birdturtle.cabbage.desktop'));
assert.ok(!desktop.includes('x-scheme-handler/hapticscape'));
assert.ok(!desktop.includes('com.hapticscape'));

const main = read('src-tauri/src/main.rs');
assert.ok(!main.includes('deep_links::receive('));
assert.ok(!main.includes('deep_links::start('));
assert.ok(read('src-tauri/src/updates.rs').includes('const REPOSITORY: &str = "birdturtle/Cabbage";'));
