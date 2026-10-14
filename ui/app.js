const invoke = window.__TAURI__?.core.invoke;
document.addEventListener('contextmenu', event => event.preventDefault());
let status;
let busy = false;
let checkingUpdates = false;
let updateFeedback = false;
let toastTimer;
let pendingUninstall;
let requirementClient;
let menuClient;
let selectingClient = false;
let selectionRevision = 0;
let selectionQueue = Promise.resolve();
const clientLabels = {officialOsrs:"OSRS",runelite:"RuneLite",officialRs3:"RS3"};
const $ = (id) => document.getElementById(id);
function setText(id, text) { const node = $(id); if (node.textContent !== text) node.textContent = text; }

function page(name) {
  if (!['home', 'updates', 'settings'].includes(name)) return;
  document.querySelectorAll('.page').forEach((element) => { element.hidden = element.id !== name; });
  document.querySelectorAll('[data-page]').forEach((element) => element.classList.toggle('active', element.dataset.page === name));
  $('page-name').textContent = name[0].toUpperCase() + name.slice(1);
}

function toast(message, error = false) {
  clearTimeout(toastTimer);
  $('toast').textContent = String(message);
  $('toast').classList.toggle('error', error);
  $('toast').hidden = false;
  toastTimer = setTimeout(() => { $('toast').hidden = true; }, 7000);
}

async function action(command, args = {}) {
  if (busy) return;
  if (!invoke) { toast('Open the desktop launcher to use this action.', true); return; }
  busy = true;
  render();
  document.querySelectorAll('button').forEach((button) => { if (!button.dataset.page && !button.dataset.go) button.disabled = true; });
  try {
    const result = await invoke(command, args);
    if (typeof result === 'string') toast(result);
    else if (command === 'save_preferences') toast('Preferences saved.');
    await refresh();
    return result;
  } catch (error) {
    toast(error, true);
    if (command === 'check_updates' || command === 'install_update') {
      updateFeedback = true;
      $('update-message').textContent = String(error);
      if (command === 'check_updates') $('release-summary').textContent = 'Update check failed. Try again.';
    }
  }
  finally { busy = false; render(); }
}

function render() {
  document.querySelectorAll('button').forEach((button) => { if (button.id !== 'play') button.disabled = busy && !button.dataset.page && !button.dataset.go; });

  setText('check-updates', checkingUpdates ? 'Checking…' : 'Check for updates');
  if (!status) { $('play').disabled = true; return; }
  const selectedGame = status.settings.selectedGame;
  const choices = status.clients.filter(client => client.game === selectedGame);
  setText('launch-title', selectedGame === 'oldschool' ? 'Old School RuneScape' : 'RuneScape');
  const clientName = choices.find(client => client.id === status.selectedClient)?.name || '';
  const character = status.characters.some(item => item.accountId === status.selectedCharacter);
  const playDisabled = Boolean(busy || !status.clientAvailable || status.clientBusy || status.clientRunning || !character || status.needsSignIn || status.signingIn);
  if ($('play').disabled !== playDisabled) $('play').disabled = playDisabled;
  setText('play', status.clientBusy ? 'Please wait…' : 'Play');
  const activity = status.clients.find(client => client.id === status.selectedClient && client.busy) || status.clients.find(client => client.busy);
  const preparing = Boolean(status.clientBusy || activity || status.requirements?.busy);
  const progressMessage = status.requirements?.busy ? status.requirements.message : activity?.message || status.clientMessage || '';
  const percentMatch = progressMessage.match(/(\d+(?:\.\d+)?)%/);
  const progress = $('client-progress');
  progress.hidden = !preparing;
  progress.classList.toggle('indeterminate', preparing && !percentMatch);
  progress.setAttribute('aria-label', progressMessage || 'Client preparation');
  if (preparing && percentMatch) {
    const percent = Math.min(100, Math.max(0, Number(percentMatch[1])));
    $('client-progress-fill').style.width = `${percent}%`;
    progress.setAttribute('aria-valuenow', String(percent));
  } else {
    $('client-progress-fill').style.width = '';
    progress.removeAttribute('aria-valuenow');
  }
  $('requirement-actions').hidden = !(status.requirements?.required || status.requirements?.message);
  $('install-requirements').hidden = !status.requirements?.required || status.requirements.canInstall === false;
  setText('play-hint', status.requirements?.message || status.clientMessage || (character ? '' : status.accounts.length ? 'Choose a character to play.' : 'Add an account to play.'));
  setText('client-version', status.clientVersion ? `${clientName} ${status.clientVersion}` : clientName);
  for (const client of status.clients) {
    const card = $(`client-${client.id}`); if (!card) continue;
    card.disabled = busy; card.classList.toggle('selected', status.selectedClient === client.id);
    card.setAttribute('aria-pressed', String(status.selectedClient === client.id));
    const state = $(`status-${client.id}`);
    setText(`status-${client.id}`, client.busy ? client.message || 'Preparing…' : client.inUse ? 'Running' : !client.available ? client.unavailableReason || 'Unavailable' : client.filesPresent ? 'Installed' : 'Not installed');
    state.classList.toggle('running', Boolean(client.inUse));
  }
  const target = status.clients.find(client => client.id === menuClient);
  const blocked = busy || !target || target.busy || target.inUse || !target.filesPresent;
  $('repair-client').disabled = blocked || !target?.available;
  $('uninstall-client').disabled = blocked;
  setText('installed-version', status.installedVersion || '');
  setText('platform', status.platform === 'linux' ? 'Linux' : status.platform);
  $('sign-in').disabled = busy || status.signingIn;
  setText('sign-in', status.signingIn ? 'Waiting for Jagex…' : 'Add account');
  $('reauthenticate').hidden = !status.needsSignIn;
  $('reauthenticate').disabled = busy || status.signingIn;
  $('cancel-login').hidden = !status.signingIn;
  $('account-actions').hidden = !status.signedIn;
  setText('account-message', status.accountMessage || '');
  const accountSignature = JSON.stringify([status.accounts, status.selectedAccount]);
  if ($('accounts').dataset.signature !== accountSignature) {
    $('accounts').replaceChildren();
    if (!status.accounts.length) {
      const option = document.createElement('option'); option.value = ''; option.textContent = 'No accounts added'; $('accounts').append(option);
    }
    for (const account of status.accounts) {
      const option = document.createElement('option'); option.value = account.id; option.textContent = account.name || 'Jagex account'; $('accounts').append(option);
    }
    $('accounts').value = status.selectedAccount || '';
    $('accounts').dataset.signature = accountSignature;
  }
  $('accounts').disabled = busy || !status.accounts.length;
  const signature = JSON.stringify([status.characters, status.selectedCharacter]);
  if ($('characters').dataset.signature !== signature) {
    $('characters').replaceChildren();
    if (!status.characters.length) {
      const empty = document.createElement('p'); empty.className = 'character-empty';
      empty.textContent = status.signedIn ? 'No characters on this account.' : 'Add an account to choose a character.';
      $('characters').append(empty);
    }
    for (const character of status.characters) {
      const button = document.createElement('button'); button.type = 'button'; button.className = 'character-row';
      const selected = character.accountId === status.selectedCharacter;
      button.classList.toggle('selected', selected); button.setAttribute('aria-pressed', String(selected));
      const avatar = document.createElement('span'); avatar.className = 'character-avatar'; avatar.setAttribute('aria-hidden', 'true');
      const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); svg.setAttribute('viewBox', '0 0 24 24');
      const head = document.createElementNS('http://www.w3.org/2000/svg', 'circle'); head.setAttribute('cx', '12'); head.setAttribute('cy', '8'); head.setAttribute('r', '4');
      const shoulders = document.createElementNS('http://www.w3.org/2000/svg', 'path'); shoulders.setAttribute('d', 'M4 22v-3a8 8 0 0 1 16 0v3');
      svg.append(head, shoulders); avatar.append(svg);
      const name = document.createElement('span'); name.className = 'character-name'; name.textContent = character.displayName;
      const marker = document.createElement('span'); marker.className = 'character-selected'; marker.textContent = selected ? '✓' : ''; marker.setAttribute('aria-hidden', 'true');
      button.append(avatar, name, marker); button.addEventListener('click', () => action('select_character', { id: character.accountId }));
      $('characters').append(button);
    }
    $('characters').dataset.signature = signature;
  }
  $('characters').querySelectorAll('button').forEach((button) => { button.disabled = busy; });
}

async function refresh() {
  if (!invoke) return;
  try {
    const revision = selectionRevision;
    const nextStatus = await invoke('launcher_status');
    if (selectingClient || revision !== selectionRevision) return;
    status = nextStatus;
    $('dedicated-gpu-preference').hidden = status.platform !== 'linux';
    if (!$('preferences-form').dataset.loaded) {
      for (const [key, value] of Object.entries(status.settings.preferences)) {
        const field = $('preferences-form').elements.namedItem(key);
        if (!field) continue;
        if (key === 'afterPlay') field.value = value; else field.checked = value;
      }
      $('preferences-form').dataset.loaded = 'true';
    }
    render();
  } catch (error) { if (!status) toast(error, true); }
}

document.querySelectorAll('[data-page], [data-go]').forEach((button) => button.addEventListener('click', () => page(button.dataset.page || button.dataset.go)));
$('install-requirements').addEventListener('click', () => {
  if (busy || !status?.requirements?.required) return;
  if (status.platform === 'linux') return action('install_requirements', {client:status.selectedClient});
  requirementClient = status.selectedClient;
  $('requirements-dialog').showModal();
});
$('cancel-requirements').addEventListener('click', () => { requirementClient=undefined; $('requirements-dialog').close(); });
$('requirements-dialog').addEventListener('cancel', () => { requirementClient=undefined; });
$('confirm-requirements').addEventListener('click', () => {
  if (busy || !requirementClient) return;
  const client=requirementClient; requirementClient=undefined; $('requirements-dialog').close();
  return action('install_requirements', {client});
});
$('recheck-requirements').addEventListener('click', () => action('recheck_requirements'));
$('play').addEventListener('click', () => { if (!busy && !selectingClient && status?.clientAvailable && !status?.clientBusy && !status?.clientRunning && !status?.needsSignIn && status?.characters.some(item => item.accountId === status.selectedCharacter)) return action('launch_client'); });
function selectClient(client) {
  if (busy || !invoke || !client || (status.selectedClient === client.id && !selectingClient)) return;
  const revision = ++selectionRevision; selectingClient = true;
  status.settings.selectedGame = client.game;
  if (client.game === 'oldschool') status.settings.osrsClient = client.id;
  status.selectedClient = client.id; status.clientAvailable = client.available;
  status.clientBusy = client.busy; status.clientVersion = undefined;
  status.clientMessage = client.message || client.unavailableReason || '';
  render();
  selectionQueue = selectionQueue.catch(() => {}).then(() => invoke('select_client', {game:client.game,client:client.id}));
  return selectionQueue.catch(error => { if (revision === selectionRevision) toast(error,true); })
    .finally(async () => { if (revision === selectionRevision) { selectingClient = false; await refresh(); } });
}
function closeClientMenu() { $('client-menu').hidden = true; menuClient = undefined; }
function openClientMenu(id, x, y) {
  if (busy) return;
  menuClient = id; const menu = $('client-menu'); menu.hidden = false; render();
  const rect = menu.getBoundingClientRect();
  menu.style.left = `${Math.max(8, Math.min(x, window.innerWidth - rect.width - 8))}px`;
  menu.style.top = `${Math.max(8, Math.min(y, window.innerHeight - rect.height - 8))}px`;
  for (const id of ['repair-client','uninstall-client']) if (!$(id).disabled) { $(id).focus(); break; }
}
for (const id of ['officialOsrs','runelite','officialRs3']) {
  $(`client-${id}`).addEventListener('click', () => {
    closeClientMenu(); const client = status?.clients.find(client => client.id === id);
    if (client) return selectClient(client);
  });
  $(`client-${id}`).addEventListener('contextmenu', event => { event.preventDefault(); openClientMenu(id,event.clientX,event.clientY); });
  $(`client-${id}`).addEventListener('keydown', event => {
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect(); openClientMenu(id,rect.left,rect.bottom);
    }
  });
}
document.addEventListener('pointerdown', event => { if (!$('client-menu').contains(event.target)) closeClientMenu(); });
document.addEventListener('keydown', event => {
  if (event.key === 'Escape') { const id = menuClient; closeClientMenu(); if (id) $(`client-${id}`).focus(); }
  if (!$('client-menu').hidden && ['ArrowDown','ArrowUp'].includes(event.key)) {
    event.preventDefault(); const entries = ['repair-client','uninstall-client'].map($).filter(item => !item.disabled);
    if (entries.length) { const current = entries.indexOf(document.activeElement); entries[(current + (event.key === 'ArrowDown' ? 1 : entries.length-1) + entries.length) % entries.length].focus(); }
  }
});
window.addEventListener('resize', closeClientMenu);
document.addEventListener('scroll', closeClientMenu, true);
$('repair-client').addEventListener('click', () => {
  const client = status?.clients.find(client => client.id === menuClient);
  if (!busy && client?.available && client.filesPresent && !client.busy && !client.inUse) {
    closeClientMenu(); return action('maintain_client', {client:client.id,operation:'repair'});
  }
});
$('uninstall-client').addEventListener('click', () => {
  const client = status?.clients.find(client => client.id === menuClient);
  if (busy || !client?.filesPresent || client.busy || client.inUse) return;
  pendingUninstall = client.id; closeClientMenu();
  $('uninstall-title').textContent = `Uninstall ${clientLabels[client.id]}?`;
  $('uninstall-dialog').showModal();
});
$('cancel-uninstall').addEventListener('click', () => { pendingUninstall = undefined; $('uninstall-dialog').close(); });
$('uninstall-dialog').addEventListener('cancel', () => { pendingUninstall = undefined; });
$('confirm-uninstall').addEventListener('click', () => {
  if (!pendingUninstall || busy) return;
  const client = pendingUninstall; pendingUninstall = undefined; $('uninstall-dialog').close();
  return action('maintain_client', {client,operation:'uninstall'});
});
$('accounts').addEventListener('change', (event) => action('select_account', { id: event.target.value }));
$('remove-account').addEventListener('click', () => { if (status?.selectedAccount) action('remove_account', { id: status.selectedAccount }); });
for (const [id, command] of [['sign-in', 'begin_login'], ['cancel-login', 'cancel_login']]) $(id).addEventListener('click', () => action(command));
$('preferences-form').addEventListener('submit', async (event) => {
  event.preventDefault();
  const preferences = Object.fromEntries(['preferDedicatedGpu', 'checkUpdatesOnStartup', 'includeBetaUpdates'].map((key) => [key, event.currentTarget.elements.namedItem(key).checked]));
  preferences.afterPlay = event.currentTarget.elements.namedItem('afterPlay').value || 'keepOpen';
  await action('save_preferences', { preferences });
  $('install-update').hidden = true;
  $('release-name').hidden = true;
  $('release-notes').hidden = true;
  updateFeedback = true;
  $('update-message').textContent = 'Check for updates to use the saved preferences.';
  $('release-summary').textContent = `Installed: ${status.installedVersion || ''}`;
});
async function checkUpdates() {
  if (busy) return;
  checkingUpdates = true;
  updateFeedback = true;
  $('install-update').hidden = true;
  $('release-name').hidden = true;
  $('release-notes').hidden = true;
  const channel = status?.settings.preferences.includeBetaUpdates ? 'Beta' : 'Stable';
  $('release-summary').textContent = `Installed: ${status?.installedVersion || ''} · Channel: ${channel}`;
  $('update-message').textContent = 'Checking for updates…';
  render();
  try {
    const release = await action('check_updates');
    if (!release) return;
    $('release-name').textContent = release.name || release.tag; $('release-name').hidden = false;
    $('update-message').textContent = release.message || '';
    $('install-update').hidden = !release.available || !(await invoke('installation_status')).canInstall;
    $('release-summary').textContent = `Installed: ${release.installedVersion || status.installedVersion || ''} · Channel: ${channel} · Latest: ${release.tag}${release.publishedAt ? ` · ${new Date(release.publishedAt).toLocaleDateString()}` : ''}`;
    $('release-notes').textContent = release.notes || 'No release notes were provided.'; $('release-notes').hidden = false;
  } finally {
    checkingUpdates = false;
    render();
  }
}
$('install-update').addEventListener('click', async () => {
  if (busy) return;
  const timer = setInterval(async () => {
    try {
      const progress = await invoke('update_progress');
      $('update-message').textContent = progress.message;
      $('update-progress').hidden = !progress.busy;
      if (progress.percent == null) $('update-progress').removeAttribute('value');
      else $('update-progress').value = progress.percent;
    } catch {}
  }, 250);
  try {
    await action('install_update');
  } finally { clearInterval(timer); $('update-progress').hidden = true; }
});
$('check-updates').addEventListener('click', checkUpdates);
async function initialize() {
  await refresh();
  if (status && invoke) {
    try { const recovery = await invoke('acknowledge_update'); if (recovery) toast(recovery, true); }
    catch (error) { toast(error, true); }
  }
  await refreshInstallation(true);
  if (!status) { $('play').disabled = true; return; }
  if (invoke) {
    try { await invoke('load_accounts'); } catch (error) { toast(error, true); }
  }
  await refresh();
  if (status.settings.preferences.checkUpdatesOnStartup) await checkUpdates();
}
initialize(); setInterval(refresh, 2000);

$('reauthenticate').addEventListener('click', () => action('begin_login'));

async function refreshInstallation(offer = false) {
  if (!invoke) return;
  try {
    const installation = await invoke('installation_status');
    $('launcher-installation').hidden = !installation.canInstall && !installation.installed;
    setText('install-launcher-description', status?.platform === 'windows' ? 'Add Cabbage to your Start Menu.' : 'Add Cabbage to your application menu.');
    $('install-launcher').hidden = !installation.canInstall || installation.runningInstalled;
    $('uninstall-launcher').hidden = !installation.installed;
    setText('installation-message', installation.installed ? 'Cabbage is installed for your user account.' : 'Run portably or add Cabbage to your application menu.');
    if (offer && installation.offerInstall) $('install-launcher-dialog').showModal();
  } catch (error) { toast(error, true); }
}
$('install-launcher').addEventListener('click', () => $('install-launcher-dialog').showModal());
$('use-portable').addEventListener('click', async () => {
  await action('keep_portable'); $('install-launcher-dialog').close();
});
$('install-launcher-dialog').addEventListener('cancel', event => {
  event.preventDefault(); $('use-portable').click();
});
$('confirm-install-launcher').addEventListener('click', async () => {
  await action('install_launcher'); $('install-launcher-dialog').close(); await refreshInstallation();
});
$('uninstall-launcher').addEventListener('click', () => $('uninstall-launcher-dialog').showModal());
$('cancel-uninstall-launcher').addEventListener('click', () => $('uninstall-launcher-dialog').close());
$('confirm-uninstall-launcher').addEventListener('click', async () => {
  await action('uninstall_launcher'); $('uninstall-launcher-dialog').close(); await refreshInstallation();
});
