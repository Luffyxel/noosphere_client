import { useEffect, useState } from 'react';
import { Activity, Play, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import type { RemoteDiagnostic, RemoteSettings } from '@/lib/remote-access';
import { accentButton } from './controls';

export function LocalRemoteTest({
  settings,
  initial,
}: {
  settings: RemoteSettings;
  initial: RemoteDiagnostic;
}) {
  const [state, setState] = useState(initial);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const running = state.state === 'running';

  useEffect(() => {
    if (!running) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        const status = await window.noosphereDesktop?.remoteAccess.status();
        if (active && status) setState(status.diagnostic);
      } catch (error) {
        if (active) setError(String(error));
      }
      if (active) timer = setTimeout(() => void poll(), 750);
    }
    timer = setTimeout(() => void poll(), 750);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [running]);

  async function toggle() {
    const api = window.noosphereDesktop?.remoteAccess;
    if (!api || busy) return;
    setBusy(true);
    setError('');
    try {
      if (running) await api.stopLocalTest();
      else {
        await api.startLocalTest(settings);
        setState({ state: 'running' });
      }
    } catch (error) {
      setError(String(error));
    } finally {
      setBusy(false);
    }
  }
  const report = state.state === 'complete' ? state.report : null;
  const failure = error || (state.state === 'failed' ? state.error : '');
  return (
    <section className="space-y-4 rounded-2xl border border-white/[.07] bg-[#242426] p-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h3 className="flex items-center gap-2 text-sm font-medium">
          <Activity className="size-4 text-[var(--noosphere-accent)]" /> Tester
          sur ce PC
        </h3>
        <Button
          className={accentButton}
          disabled={busy || (!running && settings.video.codec !== 'h264')}
          onClick={() => void toggle()}
        >
          {running ? (
            <Square className="size-3" />
          ) : (
            <Play className="size-3" />
          )}
          {running ? 'Arrêter le test' : 'Ouvrir le bureau'}
        </Button>
      </div>
      <p className="text-xs leading-5 text-[#a1a1aa]">
        15 s · Gardez la fenêtre active · Échap pour fermer.
      </p>
      {settings.video.codec !== 'h264' && (
        <p className="text-xs text-amber-200">H.264 requis.</p>
      )}
      {running && (
        <output className="block text-xs text-[var(--noosphere-accent)]">
          Test en cours…
        </output>
      )}
      {failure && (
        <p
          role="alert"
          className="rounded-xl bg-red-500/10 p-3 text-xs text-red-300"
        >
          {failure}
        </p>
      )}
      {report && (
        <>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            {[
              ['Images affichées', String(report.presentedFrames)],
              [
                'Cadence observée',
                `${((report.presentedFrames * 1000) / report.durationMs).toFixed(1)} FPS`,
              ],
              [
                'Encodage médian',
                `${(report.encodeP50Us / 1000).toFixed(1)} ms`,
              ],
              [
                'Transport médian',
                `${(report.networkP50Us / 1000).toFixed(1)} ms`,
              ],
            ].map(([label, value]) => (
              <div key={label} className="rounded-xl bg-black/15 p-3">
                <p className="text-[10px] text-[#92929b]">{label}</p>
                <p className="mt-1 text-sm font-medium tabular-nums">{value}</p>
              </div>
            ))}
          </div>
          <output className="block text-xs leading-5 text-[#a1a1aa]">
            {report.width} × {report.height} · {report.droppedFrames} image(s)
            abandonnée(s)
            <br />
            Clavier : {report.inputKeyObserved ? 'vérifié' : 'non observé'}
            <br />
            Souris : {report.inputMouseObserved ? 'vérifiée' : 'non observée'}
            <br />
            Révocation :{' '}
            {report.revocationVerified ? 'vérifiée' : 'non vérifiée'}
          </output>
        </>
      )}
    </section>
  );
}
