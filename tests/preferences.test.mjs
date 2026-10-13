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
      style:{}, focus(){}, contains(){return true;}, getBoundingClientRect(){return {width:150,height:80,left:30,bottom:60};},
      attributes:{}, setAttribute(key,value){this.attributes[key]=value;}, removeAttribute(key){delete this.attributes[key];}, showModal(){this.open=true;}, close(){this.open=false;},
      classList: { toggle() {} },
      elements: { namedItem: (name) => element(`${id}:${name}`) },
      addEventListener(type, callback) { this.listeners[type] = callback; },
      replaceChildren() { this.children = []; },
      append(child) { this.children.push(child); },
    });
    return elements.get(id);
  }
  const status = {
    settings: { preferences, selectedGame: 'oldschool', osrsClient: 'runelite' }, selectedClient: 'runelite', clientAvailable: true, clients: [{id: 'runelite', game: 'oldschool', name: 'RuneLite', available: true}, {id: 'officialOsrs', game: 'oldschool', name: 'Official', available: true}, {id: 'officialRs3', game: 'runescape', name: 'Official', available: true}], platform: 'linux', accounts: [], characters: [],
    ...options.status,
  };
  for (const client of status.clients) {
    client.filesPresent ??= status.clientFilesPresent || false;
    client.inUse ??= status.clientInUse || false; client.busy ??= false;
  }
  runInNewContext(readFileSync(new URL('../ui/app.js', import.meta.url), 'utf8'), {
    window: {innerWidth:1200,innerHeight:800,addEventListener(){}, __TAURI__: { core: { async invoke(command, args) {
      calls.push({ command, args });
      if (command === 'launcher_status') return status;
      if (command === 'installation_status') return options.installation || { installed: false, canInstall: false, offerInstall: false };
      if (command === 'check_updates') {
        if (options.checkGate) await options.checkGate;
        if (options.checkError) throw options.checkError;
        return options.release || { tag: 'v1', name: 'Release', notes: 'Notes' };
      }
      if (command === 'select_client') {
        if(options.selectionGate) await options.selectionGate;
        status.settings.selectedGame = args.game;
        if (args.game === 'oldschool') status.settings.osrsClient = args.client;
        status.selectedClient = args.client;
        status.clientAvailable = status.clients.find(client => client.id === args.client).available;
        status.clientMessage = status.clientAvailable ? '' : 'Official client support is not available yet.';
      }
      if (command === 'save_preferences') status.settings.preferences = args.preferences;
    } } } },
    document: {addEventListener(){}, getElementById: element, querySelectorAll: () => [], createElement: () => element(`option:${Math.random()}`), createElementNS: () => element(`svg:${Math.random()}`) },
    setTimeout: () => 1, clearTimeout() {}, setInterval(callback) { poll = callback; },
  });
  return { calls, element, status, poll: () => poll() };
}

const settle = () => new Promise((resolve) => setImmediate(resolve));

test('accounts load and releases check once; polling preserves unsaved edits', async () => {
  const app = launcher({ afterPlay: 'keepOpen', checkUpdatesOnStartup: true });
  await settle();
  assert.deepEqual(app.calls.filter((call) => call.command !== 'launcher_status').map((call) => call.command), ['installation_status', 'load_accounts', 'check_updates']);
  app.element('preferences-form:afterPlay').value = 'hideToTray';
  await app.poll();
  assert.equal(app.element('preferences-form:afterPlay').value, 'hideToTray');
  assert.equal(app.calls.filter((call) => call.command === 'load_accounts').length, 1);
  assert.equal(app.calls.filter((call) => call.command === 'check_updates').length, 1);
  await app.element('preferences-form').listeners.submit({ preventDefault() {}, currentTarget: app.element('preferences-form') });
  const saved = app.calls.find((call) => call.command === 'save_preferences');
  assert.equal(saved.args.preferences.afterPlay, 'hideToTray');
  assert.equal(typeof saved.args.preferences.checkUpdatesOnStartup, 'boolean');
});

test('accounts always load, while release checks remain opt-in', async () => {
  const app = launcher({ afterPlay: 'keepOpen', checkUpdatesOnStartup: false });
  await settle();
  assert.deepEqual(app.calls.map((call) => call.command), ['launcher_status', 'installation_status', 'load_accounts', 'launcher_status']);
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

test('game and client selection persist choices and support both games', async () => {
  const app = launcher({ checkUpdatesOnStartup: false }, {status: {characters: [{accountId:'character',displayName:'Player'}], selectedCharacter:'character'}});
  await settle();
  await app.element('client-officialOsrs').listeners.click();
  assert.equal(app.status.settings.osrsClient, 'officialOsrs');
  assert.equal(Boolean(app.element('play').disabled), false);
  await app.element('play').listeners.click();
  assert.equal(app.calls.filter(call => call.command === 'launch_client').length, 1);
  await app.element('client-officialRs3').listeners.click();
  assert.equal(Boolean(app.element('play').disabled), false);
  await app.element('play').listeners.click();
  assert.equal(app.calls.filter(call => call.command === 'launch_client').length, 2);
  assert.equal(app.element('launch-title').textContent, 'RuneScape');
  await app.element('client-officialOsrs').listeners.click();
  assert.equal(app.status.selectedClient, 'officialOsrs');
  await app.element('client-runelite').listeners.click();
  assert.equal(Boolean(app.element('play').disabled), false);
});

 test('unavailable client cannot launch even when the click handler is called', async () => {
  const app = launcher({checkUpdatesOnStartup:false}, {status: {
    clients:[{id:'runelite',game:'oldschool',name:'RuneLite',available:true},{id:'officialOsrs',game:'oldschool',name:'Official',available:false}],
    characters:[{accountId:'character',displayName:'Player'}], selectedCharacter:'character'
  }});
  await settle();
  await app.element('client-officialOsrs').listeners.click();
  assert.equal(app.element('play').disabled, true);
  await app.element('play').listeners.click();
  assert.ok(!app.calls.some(call => call.command === 'launch_client'));
});

test('dedicated GPU preference loads, survives polling, and saves on Linux', async () => {
  const app = launcher({preferDedicatedGpu: true, checkUpdatesOnStartup:false});
  await settle();
  assert.equal(app.element('dedicated-gpu-preference').hidden, false);
  assert.equal(app.element('preferences-form:preferDedicatedGpu').checked, true);
  app.element('preferences-form:preferDedicatedGpu').checked = false;
  await app.poll();
  assert.equal(app.element('preferences-form:preferDedicatedGpu').checked, false);
  await app.element('preferences-form').listeners.submit({preventDefault(){},currentTarget:app.element('preferences-form')});
  assert.equal(app.calls.find(call => call.command === 'save_preferences').args.preferences.preferDedicatedGpu, false);
  const windows = launcher({preferDedicatedGpu:false,checkUpdatesOnStartup:false}, {status:{platform:'windows'}});
  await settle();
  assert.equal(windows.element('dedicated-gpu-preference').hidden, true);
});

test('repair captures the selected client and running clients block maintenance', async () => {
  const app = launcher({checkUpdatesOnStartup:false}, {status:{clientFilesPresent:true,clientInUse:false,clientBusy:false}});
  await settle();
  app.element('client-runelite').listeners.contextmenu({preventDefault(){},clientX:20,clientY:30});
  await app.element('repair-client').listeners.click();
  assert.equal(app.calls.find(call => call.command === 'maintain_client').args.client, 'runelite');
  app.status.clientInUse=true; app.status.clients[0].inUse=true;
  app.element('client-runelite').listeners.contextmenu({preventDefault(){},clientX:20,clientY:30}); await app.poll();
  assert.equal(app.element('repair-client').disabled, true);
  assert.equal(app.element('uninstall-client').disabled, true);
  await app.element('repair-client').listeners.click();
  await app.element('uninstall-client').listeners.click();
  assert.equal(app.calls.filter(call => call.command === 'maintain_client').length,1);
});

test('uninstall needs confirmation and preserves the originally selected client', async () => {
  const app = launcher({checkUpdatesOnStartup:false}, {status:{clientFilesPresent:true,clientInUse:false,clientBusy:false}});
  await settle();
  app.element('client-runelite').listeners.contextmenu({preventDefault(){},clientX:20,clientY:30});
  app.element('uninstall-client').listeners.click();
  assert.equal(app.element('uninstall-dialog').open,true);
  assert.ok(!app.calls.some(call => call.command === 'maintain_client'));
  app.element('cancel-uninstall').listeners.click();
  await app.element('confirm-uninstall').listeners.click();
  assert.ok(!app.calls.some(call => call.command === 'maintain_client'));
  app.element('client-runelite').listeners.contextmenu({preventDefault(){},clientX:20,clientY:30});
  app.element('uninstall-client').listeners.click();
  app.status.selectedClient='officialRs3';
  await app.element('confirm-uninstall').listeners.click();
  const call=app.calls.find(call => call.command === 'maintain_client');
  assert.equal(call.args.client,'runelite'); assert.equal(call.args.operation,'uninstall');
});

test('bottom client entries select clients and right-click targets the clicked entry', async () => {
  const app = launcher({checkUpdatesOnStartup:false}); await settle();
  const rs3 = app.status.clients.find(client => client.id==='officialRs3'); rs3.filesPresent=true;
  app.element('client-officialRs3').listeners.contextmenu({preventDefault(){},clientX:1180,clientY:790});
  assert.equal(app.element('client-menu').hidden,false);
  await app.element('repair-client').listeners.click();
  assert.equal(app.calls.find(call => call.command==='maintain_client').args.client,'officialRs3');
  assert.equal(app.status.selectedClient,'runelite');
  await app.element('client-officialRs3').listeners.click();
  assert.equal(app.status.selectedClient,'officialRs3');
  assert.equal(app.element('status-officialRs3').textContent,'Installed');
});

test('client switching stays responsive and rapid clicks save the last choice', async () => {
  let release; const gate = new Promise(resolve => {release=resolve;});
  const app = launcher({checkUpdatesOnStartup:false}, {selectionGate:gate}); await settle();
  const first = app.element('client-officialRs3').listeners.click();
  assert.equal(app.element('launch-title').textContent,'RuneScape');
  assert.equal(Boolean(app.element('sign-in').disabled),false);
  assert.equal(app.element('client-runelite').disabled,false);
  const last = app.element('client-officialOsrs').listeners.click();
  assert.equal(app.element('launch-title').textContent,'Old School RuneScape');
  release(); await Promise.all([first,last]);
  assert.equal(app.status.selectedClient,'officialOsrs');
  assert.equal(app.calls.filter(call=>call.command==='select_client').at(-1).args.client,'officialOsrs');
});

test('preparation progress shows percentage, switches to setup, and clears on completion', async () => {
  const app=launcher({checkUpdatesOnStartup:false},{status:{clientBusy:true,clientMessage:'Downloading client... 20%'}});
  await settle();
  assert.equal(app.element('client-progress').hidden,false);
  assert.equal(app.element('client-progress-fill').style.width,'20%');
  assert.equal(app.element('client-progress').attributes['aria-valuenow'],'20');
  app.status.clientMessage='Installing client...'; await app.poll();
  assert.equal(app.element('client-progress').attributes['aria-valuenow'],undefined);
  assert.equal(app.element('client-progress-fill').style.width,'');
  app.status.clientBusy=false; await app.poll();
  assert.equal(app.element('client-progress').hidden,true);
  app.status.clients[2].busy=true; app.status.clients[2].message='Downloading RuneScape... 40%'; await app.poll();
  assert.equal(app.element('client-progress-fill').style.width,'40%');
});

test('Play stays visually enabled while an available client selection is saving', async () => {
  let release; const gate=new Promise(resolve=>{release=resolve;});
  const app=launcher({checkUpdatesOnStartup:false},{selectionGate:gate,status:{characters:[{accountId:'character',displayName:'Player'}],selectedCharacter:'character'}});
  await settle(); assert.equal(app.element('play').disabled,false);
  const selection=app.element('client-officialRs3').listeners.click();
  assert.equal(app.element('play').disabled,false);
  await app.poll(); assert.equal(app.element('play').disabled,false);
  release(); await selection;
  assert.equal(app.element('play').disabled,false);
});

test('close launcher after Play loads and saves as a single choice', async () => {
  const app=launcher({afterPlay:'closeLauncher',checkUpdatesOnStartup:false}); await settle();
  assert.equal(app.element('preferences-form:afterPlay').value,'closeLauncher');
  await app.element('preferences-form').listeners.submit({preventDefault(){},currentTarget:app.element('preferences-form')});
  assert.equal(app.calls.find(call=>call.command==='save_preferences').args.preferences.afterPlay,'closeLauncher');
});

test('AppImage offers installation while development previews stay untouched', async () => {
  const preview = launcher({ checkUpdatesOnStartup: false }); await settle();
  assert.equal(preview.element('launcher-installation').hidden, true);
  assert.ok(!preview.element('install-launcher-dialog').open);
  const image = launcher({ checkUpdatesOnStartup: false }, { installation: { installed: false, canInstall: true, offerInstall: true } }); await settle();
  assert.equal(image.element('install-launcher-dialog').open, true);
  assert.ok(!image.calls.some(call => call.command === 'install_launcher'));
  await image.element('use-portable').listeners.click();
  assert.ok(image.calls.some(call => call.command === 'keep_portable'));
  assert.equal(image.element('install-launcher-dialog').open, false);
});

test('a downloaded build can replace an existing installation', async () => {
  const app = launcher({ checkUpdatesOnStartup: false }, { installation: { installed: true, runningInstalled: false, canInstall: true, offerInstall: true } });
  await settle();
  assert.equal(app.element('install-launcher').hidden, false);
  assert.equal(app.element('install-launcher-dialog').open, true);
  assert.ok(!app.calls.some(call => call.command === 'install_launcher'));
  await app.element('confirm-install-launcher').listeners.click();
  assert.ok(app.calls.some(call => call.command === 'install_launcher'));
  const installed = launcher({ checkUpdatesOnStartup: false }, { installation: { installed: true, runningInstalled: true, canInstall: true, offerInstall: false } });
  await settle();
  assert.equal(installed.element('install-launcher').hidden, true);
  assert.ok(!installed.element('install-launcher-dialog').open);
});
