import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import Home from '@/app/page';
import '@/app/globals.css';
import { installTauriBridge } from '@/lib/tauri-bridge';
import { runTauriSmokeTest } from '@/lib/tauri-smoke';

installTauriBridge();
void runTauriSmokeTest();

const root = document.getElementById('root');
if (!root) throw new Error('Point de montage React introuvable.');

createRoot(root).render(
  <StrictMode>
    <Home />
  </StrictMode>,
);
