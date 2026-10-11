import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  rm,
  writeFile,
} from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

void test(
  'AppImage relaunch removes old mounts while preserving host libraries and arguments',
  { skip: process.platform !== 'linux' },
  async () => {
    const root = await mkdtemp(path.join(os.tmpdir(), 'noosphere-launcher-'));
    try {
      const app = path.join(root, 'new.AppDir');
      const old = path.join(root, 'old.AppDir');
      const cache = path.join(root, 'cache');
      await mkdir(path.join(app, 'usr/lib/Noosphere'), { recursive: true });
      await copyFile(
        new URL('../packaging/linux/AppRun', import.meta.url),
        path.join(app, 'AppRun'),
      );
      await chmod(path.join(app, 'AppRun'), 0o755);
      await writeFile(
        path.join(app, 'usr/lib/Noosphere/noosphere-version'),
        '0.2.0\n',
      );
      await writeFile(
        path.join(app, 'AppRun.bin'),
        '#!/usr/bin/env node\nconsole.log(JSON.stringify({args:process.argv.slice(2),appdir:process.env.APPDIR,ld:process.env.LD_LIBRARY_PATH,path:process.env.PATH,preload:process.env.LD_PRELOAD,scanner:process.env.GST_PLUGIN_SCANNER}));\n',
        { mode: 0o755 },
      );
      const hostPath = process.env.PATH;
      const result = spawnSync(
        path.join(app, 'AppRun'),
        ['--verbose', 'argument with spaces'],
        {
          encoding: 'utf8',
          env: {
            ...process.env,
            APPDIR: old,
            XDG_CACHE_HOME: cache,
            LD_LIBRARY_PATH: `${old}/usr/lib:/tmp/.mount_noosphOld/usr/lib:${cache}/appimage-run/old/usr/lib:/opt/host/lib`,
            LD_PRELOAD: '',
            PATH: `${old}/usr/bin:${hostPath}`,
            GST_PLUGIN_SCANNER: `${old}/usr/lib/gst-plugin-scanner`,
          },
        },
      );
      assert.equal(result.status, 0, result.stderr);
      const report = JSON.parse(result.stdout);
      assert.deepEqual(report.args, ['--verbose', 'argument with spaces']);
      assert.equal(report.appdir, app);
      assert.equal(report.ld, '/opt/host/lib');
      assert.equal(report.path, hostPath);
      assert.equal(report.scanner, undefined);
      assert.match(result.stderr, /Noosphere 0\.1\.54/);
      assert.match(
        result.stderr,
        /removed inherited AppImage paths from LD_LIBRARY_PATH \(3\)/,
      );
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  },
);
