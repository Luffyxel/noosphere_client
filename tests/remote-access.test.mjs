import assert from 'node:assert/strict';
import test from 'node:test';
import {
  groupMachines,
  editablePermissions,
  machineOnline,
  remoteUsers,
  samePrincipal,
  userGrants,
} from '../lib/remote-access.ts';

const principal = (account, machine, key = machine) => ({
  githubUserId: account,
  machineId: Array(16).fill(machine),
  identityKey: [5, ...Array(32).fill(key)],
});
void test('un compte conserve plusieurs machines, seule la machine courante est exclue', () => {
  const owner = principal(42, 1);
  const current = { principal: owner };
  const second = { principal: principal(42, 2) };
  const third = { principal: principal(42, 3) };
  const friend = { principal: principal(84, 4) };
  assert.deepEqual(groupMachines([current, second, third, friend], owner), {
    own: [second, third],
    friends: [friend],
  });
});
void test('les permissions restent liées au compte, à la clé et à la machine', () => {
  const original = principal(42, 1);
  assert.ok(samePrincipal(original, structuredClone(original)));
  assert.equal(samePrincipal(original, principal(42, 2)), false);
  assert.equal(samePrincipal(original, principal(42, 1, 3)), false);
  assert.equal(samePrincipal(original, principal(43, 1)), false);
});
void test('une annonce ancienne ne présente pas une machine comme connectée', () => {
  assert.equal(machineOnline({ lastSeen: 100 }, 219_999), true);
  assert.equal(machineOnline({ lastSeen: 100 }, 220_000), false);
  assert.equal(machineOnline({ lastSeen: 0 }, 900_000), false);
});

void test('tous les amis sont visibles, même sans hôte actif ou machine enregistrée', () => {
  const viewer = { id: 42, login: 'me', name: 'Me', avatarUrl: '' };
  const alice = { id: 84, login: 'alice', name: 'Alice', avatarUrl: '' };
  const bob = { id: 85, login: 'bob', name: 'Bob', avatarUrl: '' };
  const guest = {
    principal: principal(84, 2),
    hostEnabled: false,
    lastSeen: 0,
  };
  const second = {
    principal: principal(84, 3),
    hostEnabled: false,
    lastSeen: 0,
  };
  const removed = { principal: principal(86, 4), hostEnabled: true };
  assert.deepEqual(
    remoteUsers(
      viewer,
      [{ peer: alice }, { peer: bob }, { peer: alice }],
      [guest, second, removed],
    ),
    [
      { ...viewer, own: true, machines: [] },
      { ...alice, own: false, machines: [guest, second] },
      { ...bob, own: false, machines: [] },
    ],
  );
  assert.deepEqual(remoteUsers(viewer, [], [guest, removed]), [
    { ...viewer, own: true, machines: [] },
  ]);
});

void test('les réglages utilisateur résument toutes ses machines', () => {
  const machines = [
    { principal: principal(84, 2) },
    { principal: principal(84, 3) },
  ];
  const permissions = {
    screen: true,
    keyboard: true,
    mouse: false,
    audio: false,
    clipboard: false,
    gamepad: false,
  };
  const grants = machines.map((machine) => ({
    principal: machine.principal,
    permissions,
    unattended: true,
  }));
  assert.deepEqual(userGrants({ id: 84, machines }, grants), {
    permissions,
    unattended: true,
    allowedMachines: 2,
    configured: true,
  });
  grants.pop();
  const partial = userGrants({ id: 84, machines }, grants);
  assert.equal(partial.allowedMachines, 1);
  assert.equal(partial.permissions.screen, false);
  assert.equal(partial.unattended, false);
  assert.equal(partial.configured, false);
});

void test('un accès utilisateur peut être préparé avant sa première machine', () => {
  const permissions = {
    screen: true,
    keyboard: false,
    mouse: true,
    audio: false,
    clipboard: false,
    gamepad: false,
  };
  assert.deepEqual(
    userGrants(
      { id: 85, machines: [] },
      [],
      [{ githubUserId: 85, permissions, unattended: true }],
    ),
    {
      permissions,
      unattended: true,
      allowedMachines: 0,
      configured: true,
    },
  );
});

void test('une demande ne peut pas faire approuver des permissions masquées dans la fenêtre', () => {
  const request = {
    screen: true,
    keyboard: false,
    mouse: true,
    audio: true,
    clipboard: true,
    gamepad: true,
  };
  const approved = editablePermissions(request);
  assert.equal(approved.screen, true);
  assert.equal(approved.mouse, true);
  assert.equal(approved.keyboard, false);
  assert.equal(approved.audio, false);
  assert.equal(approved.clipboard, false);
  assert.equal(approved.gamepad, false);
  assert.equal(request.clipboard, true);
});
