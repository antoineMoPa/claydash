import init, { run_voxel_worker } from './target/app.js';

const ready = init();

self.onmessage = async event => {
  try {
    await ready;
    self.postMessage(run_voxel_worker(event.data));
  } catch (error) {
    self.postMessage(JSON.stringify({ kind: 'Error', message: String(error) }));
  }
};
