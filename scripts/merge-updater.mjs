// scripts/merge-updater.mjs
// Merge per-platform updater JSON files into a single artifacts/latest.json
// that tauri-plugin-updater can consume. Tolerant to Tauri v1 (latest.json)
// and Tauri v2 (<bundle>.<ext>.json) naming, and to partially-failed builds.
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const artifactsDir = path.join(process.cwd(), "artifacts");
const latestJsonPath = path.join(artifactsDir, "latest.json");
const currentTag = process.env.CURRENT_TAG || "";
const repo = process.env.GITHUB_REPOSITORY || "";

// ---------- 1. Recursive scan ----------
const allFiles = [];
(function scanDir(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) scanDir(full);
    else allFiles.push({ name: entry.name, path: full });
  }
})(artifactsDir);
console.log(`Scanned ${allFiles.length} files under ${artifactsDir}`);
console.log("All .sig files:", allFiles.filter(f => f.name.endsWith(".sig")).map(f => f.name));
console.log("All .json files:", allFiles.filter(f => f.name.endsWith(".json")).map(f => f.name));
console.log("Files in any bundle/updater/ subtree:",
  allFiles.filter(f => f.path.includes("bundle/updater")).map(f => f.path));

// ---------- 2. Map platform -> bundle filename ----------
const platformFiles = {};
for (const f of allFiles) {
  const n = f.name;
  if (n.endsWith(".sig")) continue;
  if (n.endsWith(".app.tar.gz")) {
    if (/aarch64|arm64/.test(n)) platformFiles["darwin-aarch64"] = n;
    else if (/x64|x86_64/.test(n)) platformFiles["darwin-x86_64"] = n;
  } else if (n.endsWith(".msi")) {
    if (/x64|x86_64|setup/.test(n)) platformFiles["windows-x86_64"] = n;
  } else if (n.endsWith(".AppImage")) {
    if (/x86_64|amd64/.test(n)) platformFiles["linux-x86_64"] = n;
  }
}
console.log("Mapped platform files:", platformFiles);

// ---------- 3. Discover updater JSON files (Tauri v1 + v2) ----------
function looksLikeUpdaterJson(p) {
  try {
    const j = JSON.parse(fs.readFileSync(p, "utf8"));
    if (!j || typeof j !== "object") return false;
    if (typeof j.version !== "string") return false;
    if (j.platforms !== undefined) return true;
    if (typeof j.url === "string" && typeof j.signature === "string") return true;
    return false;
  } catch {
    return false;
  }
}

const updaterJsonFiles = [];
for (const f of allFiles) {
  if (!f.name.endsWith(".json")) continue;
  if (f.path === latestJsonPath) continue;
  if (!looksLikeUpdaterJson(f.path)) continue;
  updaterJsonFiles.push(f.path);
}
console.log("Updater JSON files found:", updaterJsonFiles);

if (updaterJsonFiles.length === 0) {
  console.warn("No Tauri-generated updater JSON files found.");
  console.warn("Tauri v2 emits them at bundle/updater/<target>/<bundle>.<ext>.json");
  console.warn("This usually means TAURI_SIGNING_PRIVATE_KEY / _PASSWORD secrets are wrong or missing,");
  console.warn("or none of the built targets are updater-enabled (DMG is NOT updater-enabled;");
  console.warn("use the 'app' target on macOS to get a .app.tar.gz that produces updater JSON).");
  console.warn("Falling back to a MINIMAL latest.json built from the .sig files we have.");

  // Build latest.json from .sig + bundle files only (no version from JSON).
  const mergedFallback = { platforms: {} };
  for (const f of allFiles) {
    if (!f.name.endsWith(".sig")) continue;
    const base = f.name.slice(0, -4);
    // Try to match a known bundle name -> platform
    for (const [platform, fname] of Object.entries(platformFiles)) {
      if (fname === base) {
        const sigContent = fs.readFileSync(f.path, "utf8").trim();
        // .sig files often have a leading "untrusted comment: ..." line; keep the
        // actual signature on the last line.
        const sigLines = sigContent.split("\n").filter(l => l && !l.startsWith("untrusted comment"));
        mergedFallback.platforms[platform] = {
          signature: sigLines[sigLines.length - 1] || sigContent,
          url: ""
        };
        break;
      }
    }
  }
  // Stamp version from env tag (v0.3.118 -> 0.3.118) so client-side compare works.
  const tagVer = currentTag.replace(/^v/, "");
  if (tagVer) mergedFallback.version = tagVer;
  mergedFallback.pub_date = new Date().toISOString();
  mergedFallback.notes = "";

  // Fill URLs
  for (const platform of Object.keys(mergedFallback.platforms)) {
    const bundleName = platformFiles[platform];
    if (bundleName) {
      mergedFallback.platforms[platform].url = `https://github.com/${repo}/releases/download/${currentTag}/${bundleName}`;
    }
  }
  if (Object.keys(mergedFallback.platforms).length === 0) {
    console.error("FATAL: no bundles found at all. Aborting release publish.");
    process.exit(1);
  }
  console.warn("Falling back to writing minimal latest.json with platforms:",
    Object.keys(mergedFallback.platforms));
  fs.writeFileSync(latestJsonPath, JSON.stringify(mergedFallback, null, 2) + "\n", "utf8");
  console.log("Wrote", latestJsonPath);
  console.log("Fallback latest.json:", JSON.stringify(mergedFallback, null, 2));
  process.exit(0);
}

// ---------- 4. Read .sig files ----------
const sigs = {};
for (const f of allFiles) {
  if (f.name.endsWith(".sig")) {
    const base = f.name.slice(0, -4);
    sigs[base] = fs.readFileSync(f.path, "utf8").trim();
  }
}

// ---------- 5. Map JSON file -> platform key ----------
const jsonToPlatform = {};
for (const jp of updaterJsonFiles) {
  const base = path.basename(jp, ".json");
  let matched = false;
  for (const [platform, fname] of Object.entries(platformFiles)) {
    if (fname === base) {
      jsonToPlatform[jp] = platform;
      matched = true;
      break;
    }
  }
  if (matched) continue;
  // Fallback: look at parent dir for target triple
  const parent = path.basename(path.dirname(jp));
  if (/aarch64|arm64/.test(parent)) jsonToPlatform[jp] = "darwin-aarch64";
  else if (/darwin|apple-darwin/.test(parent)) jsonToPlatform[jp] = "darwin-x86_64";
  else if (/x86_64|amd64|linux/.test(parent)) jsonToPlatform[jp] = "linux-x86_64";
  else if (/windows|msvc|pc-windows/.test(parent)) jsonToPlatform[jp] = "windows-x86_64";
}
console.log("JSON -> platform mapping:", jsonToPlatform);

// ---------- 6. Build merged latest.json ----------
const merged = { platforms: {} };
let firstVersion = null;

for (const jp of updaterJsonFiles) {
  const j = JSON.parse(fs.readFileSync(jp, "utf8"));
  if (typeof j.version === "string" && firstVersion === null) firstVersion = j.version;
  if (j.platforms && typeof j.platforms === "object") {
    Object.assign(merged.platforms, j.platforms);
    continue;
  }
  const platform = jsonToPlatform[jp];
  if (!platform) {
    console.warn(`Skipping ${jp} - could not determine platform`);
    continue;
  }
  const bundleName = platformFiles[platform];
  if (!bundleName) {
    console.warn(`Skipping ${jp} - no matching bundle for platform ${platform}`);
    continue;
  }
  const sig = j.signature || sigs[bundleName] || "";
  merged.platforms[platform] = { signature: sig, url: "" };
}

if (firstVersion) merged.version = firstVersion;
merged.pub_date = new Date().toISOString();
merged.notes = "";

for (const platform of Object.keys(merged.platforms)) {
  const bundleName = platformFiles[platform];
  if (bundleName) {
    merged.platforms[platform].url = `https://github.com/${repo}/releases/download/${currentTag}/${bundleName}`;
  } else {
    console.warn(`No bundle file matched platform ${platform} - updater will be broken for this platform`);
  }
  if (!merged.platforms[platform].signature) {
    console.warn(`No signature for platform ${platform} - updater will be broken for this platform`);
  }
}

fs.writeFileSync(latestJsonPath, JSON.stringify(merged, null, 2) + "\n", "utf8");
console.log("Wrote", latestJsonPath);
console.log("Merged latest.json:", JSON.stringify(merged, null, 2));
