// Extract fresh kimi-auth cookie from Kimi desktop (Electron) cookie DB,
// decrypt (Chromium v10: DPAPI-protected AES-256-GCM key), verify expiry,
// write into ai-quota-desk config, and test GetSubscriptionStats.
const fs = require("fs");
const path = require("path");
const os = require("os");
const cp = require("child_process");
const crypto = require("crypto");
const { DatabaseSync } = require("node:sqlite");

const base = path.join(process.env.APPDATA, "kimi-desktop");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "kimi-auth-"));

// 1) copy cookie DBs (plus -wal/-journal so recent rows are visible)
const sources = [
  "C:\\Users\\Administrator\\AppData\\Local\\Temp\\kimicopy\\c0.db",
  "C:\\Users\\Administrator\\AppData\\Local\\Temp\\kimicopy\\c1.db",
];
const dbs = [];
for (const [i, s] of sources.entries()) {
  if (!fs.existsSync(s)) continue;
  const dest = path.join(tmp, `c${i}.db`);
  fs.copyFileSync(s, dest);
  for (const ext of ["-wal", "-journal"]) {
    if (fs.existsSync(s + ext)) fs.copyFileSync(s + ext, dest + ext);
  }
  dbs.push({ dest, src: s });
}
console.log("db copies:", dbs.map((d) => path.basename(d.src)).join(", "));

// 2) os_crypt AES key: Local State -> DPAPI unprotect via PowerShell
const ls = JSON.parse(fs.readFileSync(path.join(base, "Local State"), "utf8"));
const encKey = Buffer.from(ls.os_crypt.encrypted_key, "base64");
if (encKey.slice(0, 5).toString() !== "DPAPI") throw new Error("unexpected os_crypt prefix: " + encKey.slice(0, 5));
const blobFile = path.join(tmp, "blob.bin");
fs.writeFileSync(blobFile, encKey.slice(5));
const ps1 = path.join(tmp, "unprotect.ps1");
fs.writeFileSync(
  ps1,
  `Add-Type -AssemblyName System.Security\r\n` +
    `$b = [IO.File]::ReadAllBytes('${blobFile.replace(/\\/g, "\\")}')\r\n` +
    `$dec = [Security.Cryptography.ProtectedData]::Unprotect($b, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)\r\n` +
    `[Convert]::ToBase64String($dec)\r\n`
);
const key = Buffer.from(cp.execSync(`powershell -NoProfile -ExecutionPolicy Bypass -File "${ps1}"`).toString().trim(), "base64");
console.log("aes key bytes:", key.length);

// 3) decrypt helper (Chromium v10/v11: 3-byte ver + 12-byte nonce + ct + 16-byte tag)
function decrypt(buf, host) {
  const ver = buf.slice(0, 3).toString("latin1");
  if (ver !== "v10" && ver !== "v11") return { err: "unsupported version: " + ver };
  const nonce = buf.slice(3, 15);
  const tag = buf.slice(buf.length - 16);
  const ct = buf.slice(15, buf.length - 16);
  for (const aad of [null, crypto.createHash("sha256").update(host).digest()]) {
    try {
      const d = crypto.createDecipheriv("aes-256-gcm", key, nonce);
      d.setAuthTag(tag);
      if (aad) d.setAAD(aad);
      return { val: Buffer.concat([d.update(ct), d.final()]).toString("utf8") };
    } catch (e) {
      /* try next aad */
    }
  }
  return { err: "gcm auth failed" };
}

// 4) find kimi-auth rows, pick the freshest unexpired
const nowSec = Date.now() / 1000;
let best = null;
for (const { dest, src } of dbs) {
  try {
    const db = new DatabaseSync(dest, { readOnly: true });
    const rows = db.prepare("SELECT host_key, name, encrypted_value FROM cookies WHERE name = 'kimi-auth'").all();
    db.close();
    for (const r of rows) {
      const buf = Buffer.from(r.encrypted_value ?? new Uint8Array());
      if (!buf.length) continue;
      const out = decrypt(buf, String(r.host_key));
      if (out.err) {
        console.log(`[${path.basename(src)}] ${r.host_key}: ${out.err}`);
        continue;
      }
      const parts = out.val.split(".");
      if (parts.length !== 3) continue;
      const claims = JSON.parse(Buffer.from(parts[1], "base64").toString("utf8"));
      console.log(`[${path.basename(src)}] ${r.host_key}: token app=${claims.app_id} exp=${new Date(claims.exp * 1000).toISOString()} expired=${nowSec > claims.exp}`);
      if (claims.app_id === "kimi" && claims.exp > nowSec && (!best || claims.exp > best.exp)) {
        best = { token: out.val, exp: claims.exp };
      }
    }
  } catch (e) {
    console.log(`[${src}] sqlite error: ${e.message}`);
  }
}

if (!best) {
  console.log("RESULT: no fresh kimi-auth found");
  process.exit(1);
}

// 5) write into ai-quota-desk config (without printing the token)
const cfgPath = path.join(process.env.APPDATA, "ai-quota-desk", "config.json");
let cfg = {};
try {
  cfg = JSON.parse(fs.readFileSync(cfgPath, "utf8"));
} catch (e) {}
cfg.kimi_web_token = best.token;
fs.writeFileSync(cfgPath, JSON.stringify(cfg, null, 2));
console.log("config updated, token exp:", new Date(best.exp * 1000).toISOString());

// 6) live test GetSubscriptionStats
const https = require("https");
const req = https.request(
  {
    hostname: "www.kimi.com",
    path: "/apiv2/kimi.gateway.membership.v2.MembershipService/GetSubscriptionStats",
    method: "POST",
    headers: { Authorization: "Bearer " + best.token, "Content-Type": "application/json" },
  },
  (res) => {
    let d = "";
    res.on("data", (c) => (d += c));
    res.on("end", () => {
      console.log("GetSubscriptionStats HTTP", res.statusCode);
      console.log(d.slice(0, 500));
    });
  }
);
req.on("error", (e) => console.log("ERR", e.message));
req.end("{}");
