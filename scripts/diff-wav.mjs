import fs from 'fs';

function readWav(path) {
  const fd = fs.openSync(path, 'r');
  const head = Buffer.alloc(1024);
  fs.readSync(fd, head, 0, 1024, 0);
  if (head.toString('ascii', 0, 4) !== 'RIFF' || head.toString('ascii', 8, 12) !== 'WAVE') {
    throw new Error(`${path}: not a RIFF/WAVE file`);
  }
  let offset = 12;
  let format = null;
  let dataOffset = null;
  let dataBytes = 0;
  while (offset + 8 <= head.length) {
    const id = head.toString('ascii', offset, offset + 4);
    const size = head.readUInt32LE(offset + 4);
    if (id === 'fmt ') {
      format = {
        encoding: head.readUInt16LE(offset + 8),
        channels: head.readUInt16LE(offset + 10),
        sampleRate: head.readUInt32LE(offset + 12),
        bits: head.readUInt16LE(offset + 22),
      };
    } else if (id === 'data') {
      dataOffset = offset + 8;
      dataBytes = size;
      break;
    }
    offset += 8 + size + (size % 2);
  }
  if (format === null || dataOffset === null) throw new Error(`${path}: no fmt or data chunk`);
  const size = fs.fstatSync(fd).size;
  if (dataBytes === 0 || dataOffset + dataBytes > size) dataBytes = size - dataOffset;
  const bytesPerSample = format.bits / 8;
  const total = Math.floor(dataBytes / bytesPerSample);
  const samples = new Float32Array(total);
  const chunkSamples = 1 << 22;
  const buffer = Buffer.alloc(chunkSamples * bytesPerSample);
  let done = 0;
  while (done < total) {
    const want = Math.min(chunkSamples, total - done);
    fs.readSync(fd, buffer, 0, want * bytesPerSample, dataOffset + done * bytesPerSample);
    for (let index = 0; index < want; index++) {
      if (format.encoding === 3 && format.bits === 32) samples[done + index] = buffer.readFloatLE(index * 4);
      else if (format.bits === 16) samples[done + index] = buffer.readInt16LE(index * 2) / 32768;
      else if (format.bits === 24) samples[done + index] = buffer.readIntLE(index * 3, 3) / 8388608;
      else if (format.bits === 32) samples[done + index] = buffer.readInt32LE(index * 4) / 2147483648;
      else throw new Error(`${path}: unsupported ${format.bits}-bit encoding ${format.encoding}`);
    }
    done += want;
  }
  fs.closeSync(fd);
  return { samples, ...format, frames: total / format.channels };
}

function writeWav(path, samples, sampleRate, channels) {
  const header = Buffer.alloc(44);
  header.write('RIFF', 0, 'ascii');
  header.writeUInt32LE(36 + samples.length * 4, 4);
  header.write('WAVEfmt ', 8, 'ascii');
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(3, 20);
  header.writeUInt16LE(channels, 22);
  header.writeUInt32LE(sampleRate, 24);
  header.writeUInt32LE(sampleRate * channels * 4, 28);
  header.writeUInt16LE(channels * 4, 32);
  header.writeUInt16LE(32, 34);
  header.write('data', 36, 'ascii');
  header.writeUInt32LE(samples.length * 4, 40);
  const fd = fs.openSync(path, 'w');
  fs.writeSync(fd, header);
  const buffer = Buffer.alloc(1 << 22);
  let written = 0;
  while (written < samples.length) {
    const want = Math.min(buffer.length / 4, samples.length - written);
    for (let index = 0; index < want; index++) buffer.writeFloatLE(samples[written + index], index * 4);
    fs.writeSync(fd, buffer, 0, want * 4);
    written += want;
  }
  fs.closeSync(fd);
}

const [recorded, rendered, diffPath] = process.argv.slice(2);
if (!recorded || !rendered) {
  console.error('usage: node scripts/diff-wav.mjs <recorded.wav> <rendered.wav> [diff.wav]');
  process.exit(1);
}

const a = readWav(recorded);
const b = readWav(rendered);
if (a.sampleRate !== b.sampleRate) console.warn(`sample rates differ: ${a.sampleRate} vs ${b.sampleRate}`);

const compared = Math.min(a.samples.length, b.samples.length);
const difference = new Float32Array(compared);
let identical = 0;
let worst = 0;
let worstIndex = 0;
for (let index = 0; index < compared; index++) {
  const delta = a.samples[index] - b.samples[index];
  difference[index] = delta;
  if (delta === 0) identical++;
  else if (Math.abs(delta) > worst) {
    worst = Math.abs(delta);
    worstIndex = index;
  }
}

const frames = compared / a.channels;
console.log(`recorded  : ${a.frames} frames, ${a.channels} ch, ${a.sampleRate} Hz, ${a.bits}-bit`);
console.log(`rendered  : ${b.frames} frames, ${b.channels} ch, ${b.sampleRate} Hz, ${b.bits}-bit`);
if (a.frames !== b.frames) console.log(`length    : differs by ${b.frames - a.frames} frames`);
console.log(`compared  : ${compared} samples (${(frames / a.sampleRate).toFixed(2)} s)`);
console.log(`identical : ${identical} (${((100 * identical) / compared).toFixed(4)}%)`);
console.log(`different : ${compared - identical}`);
if (worst === 0) {
  console.log('\nresult: IDENTICAL. Every sample matches.');
} else {
  const at = Math.floor(worstIndex / a.channels) / a.sampleRate;
  console.log(`worst     : ${worst.toExponential(3)} at ${at.toFixed(3)} s`);
  const windowFrames = a.sampleRate;
  const windows = [];
  for (let start = 0; start < frames; start += windowFrames) {
    let peak = 0;
    const end = Math.min(start + windowFrames, frames) * a.channels;
    for (let index = start * a.channels; index < end; index++) {
      const magnitude = Math.abs(difference[index]);
      if (magnitude > peak) peak = magnitude;
    }
    if (peak > 0) windows.push([start / a.sampleRate, peak]);
  }
  windows.sort((one, two) => two[1] - one[1]);
  console.log(`seconds with any difference: ${windows.length} of ${Math.ceil(frames / windowFrames)}`);
  console.log(`worst seconds: ${windows.slice(0, 8).map(([second, peak]) => `${second}s:${peak.toExponential(1)}`).join('  ')}`);
  console.log('\nresult: NOT identical.');
}

if (diffPath) {
  writeWav(diffPath, difference, a.sampleRate, a.channels);
  console.log(`\nwrote ${diffPath}. Silence there means a perfect reconstruction.`);
}
