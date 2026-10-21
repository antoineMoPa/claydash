import init, {
  parse_gaussian_ply_worker,
  take_gaussian_ply_worker_batch,
  clear_gaussian_ply_worker,
} from './target/app.js';

const ready = init();

self.onmessage = async event => {
  try {
    await ready;
    let bytes = new Uint8Array(await event.data.arrayBuffer());
    const total = parse_gaussian_ply_worker(bytes);
    bytes = null;
    const batchSize = 2048;
    for (let start = 0; start < total; start += batchSize) {
      const count = Math.min(batchSize, total - start);
      const data = take_gaussian_ply_worker_batch(start, count);
      self.postMessage({ kind: 'batch', data: data.buffer, count, loaded: start + count, total }, [data.buffer]);
      // Let the receiving thread paint and process batches before producing more.
      await new Promise(resolve => setTimeout(resolve, 0));
    }
    clear_gaussian_ply_worker();
    self.postMessage({ kind: 'complete', total });
  } catch (error) {
    try { clear_gaussian_ply_worker(); } catch (_) { /* Wasm initialization may have failed. */ }
    self.postMessage({ kind: 'error', message: String(error) });
  }
};
