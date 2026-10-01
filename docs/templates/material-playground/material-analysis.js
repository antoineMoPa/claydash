/* Reusable image-space diagnostics for material shader studies. */
window.MaterialAnalysis = (() => {
  const size = 128;
  const fftSize = 64;
  const bins = 32;
  const clamp01 = value => Math.max(0, Math.min(1, value));
  function pixels(source, crop) {
    const width = source.naturalWidth || source.width;
    const height = source.naturalHeight || source.height;
    if (!width || !height) return null;
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = size;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    const fraction = crop === 'center' ? 0.6 : 1;
    const edge = Math.min(width, height) * fraction;
    context.drawImage(source, (width - edge) / 2, (height - edge) / 2, edge, edge, 0, 0, size, size);
    return context.getImageData(0, 0, size, size).data;
  }
  function channels(data) {
    const light = new Float32Array(size * size);
    const saturation = new Float32Array(size * size);
    for (let i = 0; i < light.length; i++) {
      const r = data[i * 4] / 255, g = data[i * 4 + 1] / 255, b = data[i * 4 + 2] / 255;
      const high = Math.max(r, g, b), low = Math.min(r, g, b);
      light[i] = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      saturation[i] = high > 0 ? (high - low) / high : 0;
    }
    return { light, saturation };
  }
  function crevasses(light, radius) {
    const stride = size + 1;
    const sum = new Float64Array(stride * stride);
    for (let y = 1; y <= size; y++) for (let x = 1; x <= size; x++) {
      const k = y * stride + x;
      sum[k] = light[(y - 1) * size + x - 1] + sum[k - 1] + sum[k - stride] - sum[k - stride - 1];
    }
    const result = new Float32Array(size * size);
    let energy = 0;
    for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
      const x0 = Math.max(0, x - radius), x1 = Math.min(size - 1, x + radius);
      const y0 = Math.max(0, y - radius), y1 = Math.min(size - 1, y + radius);
      const average = (sum[(y1 + 1) * stride + x1 + 1] - sum[y0 * stride + x1 + 1] - sum[(y1 + 1) * stride + x0] + sum[y0 * stride + x0]) / ((x1 - x0 + 1) * (y1 - y0 + 1));
      const deficit = Math.max(0, average - light[y * size + x]);
      result[y * size + x] = deficit;
      energy += deficit;
    }
    return { values: result, mean: energy / result.length };
  }
  function fft(real, imaginary) {
    const n = real.length;
    for (let i = 1, j = 0; i < n; i++) {
      let bit = n >> 1;
      for (; j & bit; bit >>= 1) j ^= bit;
      j ^= bit;
      if (i < j) { [real[i], real[j]] = [real[j], real[i]]; [imaginary[i], imaginary[j]] = [imaginary[j], imaginary[i]]; }
    }
    for (let length = 2; length <= n; length <<= 1) {
      const angle = -2 * Math.PI / length;
      for (let start = 0; start < n; start += length) for (let j = 0; j < length / 2; j++) {
        const cosine = Math.cos(angle * j), sine = Math.sin(angle * j);
        const a = start + j, b = a + length / 2;
        const tr = real[b] * cosine - imaginary[b] * sine;
        const ti = real[b] * sine + imaginary[b] * cosine;
        real[b] = real[a] - tr; imaginary[b] = imaginary[a] - ti;
        real[a] += tr; imaginary[a] += ti;
      }
    }
  }
  function spectrum(light) {
    const real = new Float64Array(fftSize * fftSize), imaginary = new Float64Array(real.length);
    let mean = 0;
    for (const value of light) mean += value;
    mean /= light.length;
    for (let y = 0; y < fftSize; y++) for (let x = 0; x < fftSize; x++) {
      const window = Math.sin(Math.PI * (x + 0.5) / fftSize) * Math.sin(Math.PI * (y + 0.5) / fftSize);
      const k = (y * 2) * size + x * 2;
      const average = 0.25 * (light[k] + light[k + 1] + light[k + size] + light[k + size + 1]);
      real[y * fftSize + x] = (average - mean) * window;
    }
    for (let y = 0; y < fftSize; y++) {
      const rowR = real.slice(y * fftSize, (y + 1) * fftSize), rowI = new Float64Array(fftSize);
      fft(rowR, rowI); real.set(rowR, y * fftSize); imaginary.set(rowI, y * fftSize);
    }
    for (let x = 0; x < fftSize; x++) {
      const columnR = new Float64Array(fftSize), columnI = new Float64Array(fftSize);
      for (let y = 0; y < fftSize; y++) { columnR[y] = real[y * fftSize + x]; columnI[y] = imaginary[y * fftSize + x]; }
      fft(columnR, columnI);
      for (let y = 0; y < fftSize; y++) { real[y * fftSize + x] = columnR[y]; imaginary[y * fftSize + x] = columnI[y]; }
    }
    const values = new Float64Array(bins), counts = new Uint32Array(bins);
    for (let y = 0; y < fftSize; y++) for (let x = 0; x < fftSize; x++) {
      const dx = Math.min(x, fftSize - x), dy = Math.min(y, fftSize - y);
      const distance = Math.hypot(dx, dy);
      const band = Math.floor(distance / Math.SQRT2);
      if (band > 0 && band < bins) { const k = y * fftSize + x; values[band] += real[k] ** 2 + imaginary[k] ** 2; counts[band]++; }
    }
    for (let i = 1; i < bins; i++) values[i] = Math.log1p(values[i] / Math.max(1, counts[i]));
    return values;
  }
  function histogram(values) {
    const result = new Float64Array(bins);
    for (const value of values) result[Math.min(bins - 1, Math.floor(clamp01(value) * bins))]++;
    for (let i = 0; i < bins; i++) result[i] /= values.length;
    return result;
  }
  function chart(canvas, values, maximum, label) {
    const context = canvas.getContext('2d'), width = canvas.width, height = canvas.height;
    context.clearRect(0, 0, width, height);
    context.fillStyle = '#eeeae3'; context.fillRect(0, 0, width, height);
    context.strokeStyle = '#beb8ae'; context.beginPath(); context.moveTo(28, 8); context.lineTo(28, height - 24); context.lineTo(width - 8, height - 24); context.stroke();
    context.fillStyle = '#4c6375';
    for (let i = 0; i < values.length; i++) {
      const barWidth = (width - 40) / values.length;
      const barHeight = (height - 36) * values[i] / Math.max(maximum, 0.00001);
      context.fillRect(29 + i * barWidth, height - 25 - barHeight, Math.max(1, barWidth - 1), barHeight);
    }
    context.fillStyle = '#554f47'; context.font = '11px system-ui'; context.fillText(label, 32, height - 6);
  }
  function heatmap(canvas, values) {
    const scratch = document.createElement('canvas'); scratch.width = scratch.height = size;
    const context = scratch.getContext('2d'); const frame = context.createImageData(size, size);
    for (let i = 0; i < values.length; i++) {
      const v = clamp01(values[i] * 5);
      frame.data[i * 4] = 246 - 189 * v;
      frame.data[i * 4 + 1] = 241 - 170 * v;
      frame.data[i * 4 + 2] = 231 - 136 * v;
      frame.data[i * 4 + 3] = 255;
    }
    context.putImageData(frame, 0, 0);
    const target = canvas.getContext('2d'); target.imageSmoothingEnabled = true;
    target.clearRect(0, 0, canvas.width, canvas.height);
    target.drawImage(scratch, 0, 0, canvas.width, canvas.height);
  }
  function create({ render, reference }) {
    const mode = document.getElementById('analysis-mode');
    const crop = document.getElementById('analysis-crop');
    const radius = document.getElementById('analysis-radius');
    const radiusValue = document.getElementById('analysis-radius-value');
    const output = [document.getElementById('analysis-render'), document.getElementById('analysis-reference')];
    const summary = document.getElementById('analysis-summary');
    const requestedMode = new URLSearchParams(location.search).get('analysis');
    if (['creases', 'fft', 'saturation', 'intensity'].includes(requestedMode)) mode.value = requestedMode;
    let pending = null;
    function update() {
      if (!reference.complete || !reference.naturalWidth || !render.width) return;
      try {
        const pair = [channels(pixels(render, crop.value)), channels(pixels(reference, crop.value))];
        if (mode.value === 'creases') {
          const maps = pair.map(item => crevasses(item.light, Number(radius.value)));
          maps.forEach((item, index) => heatmap(output[index], item.values));
          summary.textContent = 'Mean local dark deficit — render ' + maps[0].mean.toFixed(3) + ' · reference ' + maps[1].mean.toFixed(3) + '. Higher values indicate stronger small dark crevasses.';
        } else {
          const values = pair.map(item => mode.value === 'fft' ? spectrum(item.light) : histogram(item[mode.value === 'saturation' ? 'saturation' : 'light']));
          const maximum = Math.max(...values[0], ...values[1]);
          values.forEach((item, index) => chart(output[index], item, maximum, mode.value === 'fft' ? 'low → high spatial frequency' : 'dark / low → bright / high'));
          summary.textContent = mode.value === 'fft' ? '2D FFT radial power, log scale; both plots share one vertical axis.' : 'Pixel distribution; both plots share one vertical axis.';
        }
      } catch (error) { summary.textContent = 'Comparison unavailable: ' + error.message; }
    }
    function queue() { clearTimeout(pending); pending = setTimeout(update, 220); }
    for (const element of [mode, crop, radius]) element.addEventListener('input', () => { radiusValue.textContent = radius.value + ' px'; queue(); });
    reference.addEventListener('load', queue);
    return { update, queue };
  }
  return { create };
})();
