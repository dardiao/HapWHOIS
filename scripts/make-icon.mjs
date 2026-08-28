// 生成 HapWHOIS 应用图标（1024x1024 PNG）：蓝色圆角方块 + 白色放大镜
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const SIZE = 1024;
const OUT = resolve(dirname(fileURLToPath(import.meta.url)), "app-icon.png");

function crc32(buf) {
  let c, table = [];
  for (let n = 0; n < 256; n++) {
    c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  let crc = 0xffffffff;
  for (const b of buf) crc = table[(crc ^ b) & 0xff] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

// 蓝色渐变背景
const bgTop = [37, 99, 235];    // #2563eb
const bgBottom = [29, 78, 216]; // #1d4ed8

// 放大镜几何参数
const cx = 430, cy = 420, r = 205, ring = 78;
const hx = 670, hy = 660, hw = 105, hlen = 310, hAngle = Math.PI / 4;
const cosA = Math.cos(hAngle), sinA = Math.sin(hAngle);

function inHandle(px, py) {
  // 相对句柄中心旋转 -45°，转为轴对齐矩形判断
  const dx = px - hx, dy = py - hy;
  const rx = dx * cosA + dy * sinA;
  const ry = -dx * sinA + dy * cosA;
  return Math.abs(rx) <= hw / 2 && Math.abs(ry) <= hlen / 2;
}

const raw = Buffer.alloc(SIZE * (SIZE * 4 + 1));
for (let y = 0; y < SIZE; y++) {
  const row = y * (SIZE * 4 + 1);
  raw[row] = 0; // filter: none
  const t = y / SIZE;
  const bg = [
    Math.round(bgTop[0] + (bgBottom[0] - bgTop[0]) * t),
    Math.round(bgTop[1] + (bgBottom[1] - bgTop[1]) * t),
    Math.round(bgTop[2] + (bgBottom[2] - bgTop[2]) * t),
  ];
  for (let x = 0; x < SIZE; x++) {
    const i = row + 1 + x * 4;
    // 圆角矩形 mask
    const r = 190;
    const nx = Math.min(Math.max(x, r), SIZE - 1 - r);
    const ny = Math.min(Math.max(y, r), SIZE - 1 - r);
    const dx = x - nx, dy = y - ny;
    const inRoundRect = Math.hypot(dx, dy) <= r || (x > r && x < SIZE - 1 - r) || (y > r && y < SIZE - 1 - r);

    if (!inRoundRect) {
      raw[i] = raw[i + 1] = raw[i + 2] = raw[i + 3] = 0;
      continue;
    }

    // 放大镜：白圈 + 白柄
    const dist = Math.hypot(x - cx, y - cy);
    const inLens = dist >= r - ring && dist <= r;
    const inStick = inHandle(x, y);

    if (inLens || inStick) {
      raw[i] = raw[i + 1] = raw[i + 2] = 255;
      raw[i + 3] = 255;
    } else {
      raw[i] = bg[0];
      raw[i + 1] = bg[1];
      raw[i + 2] = bg[2];
      raw[i + 3] = 255;
    }
  }
}

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8;  // bit depth
ihdr[9] = 6;  // RGBA

const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw)),
  chunk("IEND", Buffer.alloc(0)),
]);

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, png);
console.log(`icon written: ${OUT} (${png.length} bytes)`);

