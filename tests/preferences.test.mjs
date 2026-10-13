import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

function launcher(preferences, options = {}) {
  const elements = new Map();
  const calls = [];
  let poll;
  function element(id) {
    if (!elements.has(id)) elements.set(id, {
      dataset: {}, checked: false, children: [], listeners: {},
      querySelectorAll: () => [],
      setAttribute() {},
      classList: { toggle() {} },
      elements: { namedItem: (name) => element(`${id}:${name}`) },
      addEventListener(type, callback) { this.listeners[type] = callback; },
      replaceChildren() { this.children = []; },
      append(child) { this.children.push(child); },
    });
    return elements.get(id);
  }
  const status = {
    settings: { preferences, selectedGame: 'oldschool', osrsClient: 'runelite' }, selectedClient: 'runelite', clientAvailable: true, clients: [{id: 'runelite', game: 'oldschool', name: 'RuneLite', available: true}, {id: 'officialOsrs', game: 'oldschool', name: 'Official', available: false}, {id: 'officialRs3', game: 'runescape', name: 'Official', available: false}], platform: 'linux', accounts: [], characters: [],
    ...options.status,
  };
  runInNewContext(readFileSync(new URL('../ui/app.js', import.meta.url), 'utf8'), {
    window: { __TAURI__: { core: { async invoke(command, args) {
      calls.push({ command, args });
      if (command === 'launcher_status') return status;
      if (command === 'check_updates') {
        if (options.checkGate) await options.checkGate;
        if (options.checkError) throw options.checkError;
        return options.release || { tag: 'v1', name: 'Release', notes: 'Notes' };
      }
      if (command === 'select_client') {
        status.settings.selectedGame = args.game;
        if (args.game === 'oldschool') status.settings.osrsClient = args.client;
        status.selectedClient = args.client;
        status.clientAvailable = args.client === 'runelite';
        status.clientMessage = status.clientAvailable ? '' : 'Official client support is not available yet.';
      }
      if (command === 'save_preferences') status.settings.preferences = args.preferences;
    } } } },
    document: { getElementById: element, querySelectorAll: () => [], createElement: () => element(`option:${Math.random()}`), createElementNS: () => element(`svg:${Math.random()}`) },
    setTimeout: () => 1, clearTimeout() {}, setInterval(callback) { poll = callback; },
  });
  return { calls, element, status, poll: () => poll() };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));

test('accounts load and releases check once; polling preserves unsaved edits', async () => {
  const app = launcher({ minimizeOnPlay: false, checkUpdatesOnStartup: true });
  await settle();
  assert.deepEqual(app.calls.filter((call) => call.command !== 'launcher_status').map((call) => call.command), ['load_accounts', 'check_updates']);
  app.element('preferences-form:minimizeOnPlay').checked = true;
  await app.poll();
  assert.equal(app.element('preferences-form:minimizeOnPlay').checked, true);
  assert.equal(app.calls.filter((call) => call.command === 'load_accounts').length, 1);
  assert.equal(app.calls.filter((call) => call.command === 'check_updates').length, 1);
  await app.element('preferences-form').listeners.submit({ preventDefault() {}, currentTarget: app.element('preferences-form') });
  const saved = app.calls.find((call) => call.command === 'save_preferences');
  assert.equal(saved.args.preferences.minimizeOnPlay, true);
  assert.equal(typeof saved.args.preferences.checkUpdatesOnStartup, 'boolean');
});

test('accounts always load, while release checks remain opt-in', async () => {
  const app = launcher({ minimizeOnPlay: false, checkUpdatesOnStartup: false });
  await settle();
  assert.deepEqual(app.calls.map((call) => call.command), ['launcher_status', 'load_accounts', 'launcher_status']);
});

test('expired account offers the existing browser sign-in flow and keeps its character visible', async () => {
  const app = launcher({ checkUpdatesOnStartup: false }, { status: {
    needsSignIn: true, accounts: [{ id: 'saved', name: 'Character' }], selectedAccount: 'saved',
    characters: [{ accountId: 'character', displayName: 'Character' }], selectedCharacter: 'character',
    accountMessage: 'Sign in again to continue playing.',
  } });
  await settle();
  assert.equal(app.element('reauthenticate').hidden, false);
  assert.equal(app.element('account-message').textContent, 'Sign in again to continue playing.');
  assert.equal(app.element('characters').children.length, 1);
  await app.element('reauthenticate').listeners.click();
  assert.equal(app.calls.some((call) => call.command === 'begin_login'), true);
  assert.equal(app.calls.some((call) => call.command === 'remove_account'), false);
});

test('startup does not install components or launch applications', async () => {
  const app = launcher({ checkUpdatesOnStartup: false });
  await settle();
  assert.equal(app.element('play').disabled, true);
  assert.ok(!app.calls.some(call => ['launch_app', 'install_components', 'install_update'].includes(call.command)));
});

test('release checks show notes and preserve failure feedback', async () => {
  const options = { release: { tag: 'v0.2.0', name: 'Cabbage 0.2.0', notes: 'Fixes', message: 'A newer Cabbage release is available.' } };
  const app = launcher({ checkUpdatesOnStartup: false }, options);
  await settle();
  await app.element('check-updates').listeners.click();
  assert.equal(app.element('release-name').textContent, 'Cabbage 0.2.0');
  assert.equal(app.element('release-notes').textContent, 'Fixes');
  options.checkError = 'Cannot reach GitHub.';
  await app.element('check-updates').listeners.click();
  await app.poll();
  assert.equal(app.element('update-message').textContent, 'Cannot reach GitHub.');
  assert.equal(app.element('release-notes').hidden, true);
});

test('Play passes only the selected account context to the backend and blocks duplicates', async () => {
  const options = { status: { accounts: [{ id: 'saved', name: 'Player' }], selectedAccount: 'saved',
    characters: [{ accountId: 'character', displayName: 'Player' }], selectedCharacter: 'character', clientRunning: false } };
  const app = launcher({ checkUpdatesOnStartup: false }, options);
  await settle();
  assert.equal(Boolean(app.element('play').disabled), false);
  await app.element('play').listeners.click();
  assert.equal(app.calls.filter(call => call.command === 'launch_client').length, 1);
  assert.deepEqual(Object.keys(app.calls.find(call => call.command === 'launch_client').args), []);
  app.status.clientRunning = true;
  await app.poll();
  assert.equal(app.element('play').disabled, true);
  await app.element('play').listeners.click();
  assert.equal(app.calls.filter(call => call.command === 'launch_client').length, 1);
});

test('download progress and failed launch stay visible while polling', async () => {
  const options = { status: { clientBusy: true, clientMessage: 'Downloading Java... 42%' } };
  const app = launcher({ checkUpdatesOnStartup: false }, options);
  await settle();
  assert.equal(app.element('play').disabled, true);
  assert.equal(app.element('play-hint').textContent, 'Downloading Java... 42%');
  app.status.clientBusy = false;
  app.status.clientMessage = 'RuneLite could not start. Retry the launch.';
  await app.poll();
  assert.equal(app.element('play-hint').textContent, 'RuneLite could not start. Retry the launch.');
});

test('game and client selection persist choices and unavailable clients cannot launch', async () => {
  const app = launcher({ checkUpdatesOnStartup: false }, {status: {characters: [{accountId:'character',displayName:'Player'}], selectedCharacter:'character'}});
  await settle();
  await app.element('client-select').listeners.change({ target: {value:'officialOsrs'} });
  assert.equal(app.status.settings.osrsClient, 'officialOsrs');
  assert.equal(app.element('play').disabled, true);
  await app.element('play').listeners.click();
  assert.ok(!app.calls.some(call => call.command === 'launch_client'));
  await app.element('game-select').listeners.change({target:{value:'runescape'}});
  assert.equal(app.element('launch-title').textContent, 'RuneScape');
  assert.equal(app.element('client-select').children.length, 1);
  await app.element('game-select').listeners.change({target:{value:'oldschool'}});
  assert.equal(app.status.selectedClient, 'officialOsrs');
  await app.element('client-select').listeners.change({target:{value:'runelite'}});
  assert.equal(Boolean(app.element('play').disabled), false);
});
