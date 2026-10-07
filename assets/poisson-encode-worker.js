import init, { run_poisson_encode_png, run_poisson_encode_glb } from './target/app.js';

const ready = init();

self.onmessage = async event => {
  try {
    await ready;
    const request = event.data;
    const bytes = request.kind === 'png'
      ? run_poisson_encode_png(request.pixels, request.size)
      : run_poisson_encode_glb(request.pages);
    self.postMessage(bytes, [bytes.buffer]);
  } catch (error) {
    self.postMessage(String(error));
  }
};
