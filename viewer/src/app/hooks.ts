import { useCallback, useEffect, useRef, useState } from 'react';
import ProfileWorker from '../worker?worker&inline';
import type { ProfileIndex } from '../model';
import { useProfile } from '../store';

type Target = 'candidate' | 'baseline';

interface WorkerMessage {
  id: number;
  type: 'progress' | 'ready' | 'error';
  value?: number;
  index?: ProfileIndex;
  message?: string;
}

/** Owns the parsing worker; only the latest request per target may publish. */
export function useLoader() {
  const [error, setError] = useState('');
  const [progress, setProgress] = useState<number | null>(null);
  const workerRef = useRef<Worker | null>(null);
  const requestRef = useRef(0);
  const pending = useRef(new Map<number, { target: Target; name: string }>());
  const latest = useRef({ candidate: 0, baseline: 0 });
  const setIndex = useProfile(state => state.setIndex);
  const setBaseline = useProfile(state => state.setBaseline);

  const send = useCallback((target: Target, name: string, payload: { text?: string; file?: File }) => {
    const id = ++requestRef.current;
    latest.current[target] = id;
    pending.current.set(id, { target, name });
    setError('');
    setProgress(0);
    workerRef.current?.postMessage({ id, ...payload });
  }, []);

  useEffect(() => {
    const worker = new ProfileWorker();
    workerRef.current = worker;
    worker.onmessage = (event: MessageEvent<WorkerMessage>) => {
      const request = pending.current.get(event.data.id);
      if (!request || latest.current[request.target] !== event.data.id) return;
      if (event.data.type === 'progress') setProgress(event.data.value ?? 0);
      else if (event.data.type === 'ready' && event.data.index) {
        if (request.target === 'candidate') setIndex(event.data.index, request.name);
        else setBaseline(event.data.index, request.name);
        pending.current.delete(event.data.id);
        setProgress(null);
        setError('');
      } else if (event.data.type === 'error') {
        setError(event.data.message ?? 'Could not read profile.');
        setProgress(null);
      }
    };
    const inline = document.getElementById('spanscope-profile')?.textContent;
    if (inline) send('candidate', 'Candidate profile', { text: inline });
    const baseline = document.getElementById('spanscope-baseline')?.textContent;
    if (baseline) send('baseline', 'Baseline profile', { text: baseline });
    return () => {
      worker.terminate();
      workerRef.current = null;
    };
  }, [send, setBaseline, setIndex]);

  const load = useCallback((file: File, target: Target = 'candidate') => send(target, file.name, { file }), [send]);
  return { load, error, progress };
}

type FilePicker = (options?: unknown) => Promise<FileSystemFileHandle[]>;

/** Chromium File System Access: reload a chosen profile whenever it changes on disk. */
export function useFollow(load: (file: File) => void) {
  const [following, setFollowing] = useState(false);
  const timer = useRef<number | null>(null);
  const picker = (window as Window & { showOpenFilePicker?: FilePicker }).showOpenFilePicker;
  const stop = useCallback(() => {
    if (timer.current !== null) window.clearInterval(timer.current);
    timer.current = null;
    setFollowing(false);
  }, []);
  const toggle = useCallback(async () => {
    if (following) {
      stop();
      return;
    }
    if (!picker) return;
    try {
      const [handle] = await picker({
        multiple: false,
        types: [{ description: 'spanscope profile', accept: { 'application/json': ['.json'], 'application/gzip': ['.gz'] } }],
      });
      if (!handle) return;
      let current = await handle.getFile();
      let signature = `${current.size}:${current.lastModified}`;
      load(current);
      setFollowing(true);
      let checking = false;
      timer.current = window.setInterval(async () => {
        if (checking) return;
        checking = true;
        try {
          current = await handle.getFile();
          const next = `${current.size}:${current.lastModified}`;
          if (next !== signature) {
            signature = next;
            load(current);
          }
        } catch {
          stop();
        } finally {
          checking = false;
        }
      }, 1500);
    } catch (failure) {
      if (failure instanceof DOMException && failure.name === 'AbortError') return;
      stop();
    }
  }, [following, load, picker, stop]);
  useEffect(() => stop, [stop]);
  return { following, available: Boolean(picker), toggle, stop };
}

/** Window-level shortcuts and drag-and-drop loading. */
export function useGlobalInput(load: (file: File) => void, toggleCommands: () => void) {
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault();
        toggleCommands();
      }
    };
    const drag = (event: DragEvent) => event.preventDefault();
    const drop = (event: DragEvent) => {
      event.preventDefault();
      const file = event.dataTransfer?.files[0];
      if (file) load(file);
    };
    window.addEventListener('keydown', keydown);
    window.addEventListener('dragover', drag);
    window.addEventListener('drop', drop);
    return () => {
      window.removeEventListener('keydown', keydown);
      window.removeEventListener('dragover', drag);
      window.removeEventListener('drop', drop);
    };
  }, [load, toggleCommands]);
}
