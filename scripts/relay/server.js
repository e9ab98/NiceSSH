// Tauri updater manifest relay:proxies Zealot API → Tauri latest.json
// Run on the reverse proxy box (e.g. 1Panel Node.js service).
//
// Listens on 127.0.0.1:8081, exposed by nginx via /relay/.
//
// For each of the 6 platform channels, query Zealot's
// /api/apps/latest?channel_key=<key>, read the embedded minisign
// signature from custom_fields.tauri_signature, and assemble a
// Tauri-format manifest on demand.

const http = require('http');

const PORT = 8081;
const ZEALOT_INTERNAL = 'http://127.0.0.1:36300';   // 同台机器走内网
const DOWNLOAD_BASE = 'https://e9ab98.jzy.cc';      // Zealot 的公网域名

const PLATFORMS = [
  { id: 'darwin-aarch64',   key: 'e1ba2fc8c83a1af730d3a181e34a3406' },
  { id: 'darwin-x64',       key: '9675db8639b12ef611c6f3a709fc87cf' },
  { id: 'windows-x86_64',   key: 'dd6a4a7722ea0bc4b8a1a4adf98b89ca' },
  { id: 'windows-aarch64',  key: '2e83f2ae765083c567d668fdfa0fc85b' },
  { id: 'linux-x86_64',     key: '8edd65d5970268a3e390e70d210064f3' },
  { id: 'linux-aarch64',    key: '4188685491dd6bdbf5249d318404b8ee' },
];

// minisign signature file format:
//   untrusted comment: <arbitrary text>
//   <base64 ed25519 signature>
//   trusted comment: <arbitrary text>
//   <base64 trusted-comment signature>
// Tauri 端 signature 字段只接受中间两行 base64,带上注释会验签失败。
// 所以从 .sig 文件内容里剥掉两个 comment 行,只留两行 base64。
function stripMinisignComments(raw) {
  const lines = String(raw || '').split(/\r?\n/);
  const b64 = [];
  for (const line of lines) {
    if (/^\s*(untrusted|trusted)\s+comment\s*:/i.test(line)) continue;
    if (line.trim() === '') continue;
    b64.push(line.trim());
  }
  return b64.join('\n');
}

function cmpVer(a, b) {
  const av = a.split('.').map(Number);
  const bv = b.split('.').map(Number);
  for (let i = 0; i < Math.max(av.length, bv.length); i++) {
    const d = (av[i] || 0) - (bv[i] || 0);
    if (d) return d;
  }
  return 0;
}

async function getZealot(channelKey) {
  const r = await fetch(`${ZEALOT_INTERNAL}/api/apps/latest?channel_key=${channelKey}`);
  if (!r.ok) throw new Error(`zealot ${r.status}`);
  return r.json();
}

http.createServer(async (req, res) => {
  if (req.url !== '/latest.json') {
    res.writeHead(404); return res.end();
  }
  try {
    const platforms = {};
    let latestVer = '';
    let pubDate = '';

    const results = await Promise.allSettled(PLATFORMS.map(p => getZealot(p.key)));
    for (let i = 0; i < PLATFORMS.length; i++) {
      const { id } = PLATFORMS[i];
      const r = results[i];
      if (r.status !== 'fulfilled') continue;
      const release = r.value?.releases?.[0];
      if (!release) continue;

      const ver = (release.branch ?? '').replace(/^v/, '');
      if (!ver) continue;

      if (!latestVer || cmpVer(ver, latestVer) > 0) {
        latestVer = ver;
        pubDate = release.created_at || '';
      }

      const sigRaw = release.custom_fields?.tauri_signature || '';
      const signature = stripMinisignComments(sigRaw);

      // Zealot 暴露 install_url(非 iOS 等于 download_url = /download/releases/<id>),
      // 访问 /download/releases/<id> 会 302 跳到 /download/releases/<id>/<filename>。
      // 之前用 release.download_url,但 ReleaseSerializer 没暴露那个字段,拼出
      // "...undefined/170" 这种 URL,改用 install_url。
      platforms[id] = {
        url: `${DOWNLOAD_BASE}${release.install_url}`,
        signature,
      };
    }

    res.writeHead(200, {
      'Content-Type': 'application/json; charset=utf-8',
      'Cache-Control': 'no-cache, no-store, must-revalidate',
    });
    res.end(JSON.stringify({
      version: latestVer,
      notes: '',
      pub_date: pubDate,
      platforms,
    }, null, 2));
  } catch (e) {
    console.error(e);
    res.writeHead(500, { 'Content-Type': 'application/json' });
    res.end(JSON.stringify({ error: e.message }));
  }
}).listen(PORT, '127.0.0.1', () => console.log(`relay on :${PORT}`));
