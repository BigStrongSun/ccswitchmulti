const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const zlib = require("node:zlib");
const { spawnSync } = require("node:child_process");

const repo = path.resolve(__dirname, "../..");
const tempRoot = path.join(repo, ".tmp");
fs.mkdirSync(tempRoot, { recursive: true });

function fixture() {
  const root = fs.mkdtempSync(path.join(tempRoot, "production-assets-"));
  const dist = path.join(root, "dist");
  const target = path.join(root, "target");
  const cache = path.join(
    target,
    "release/build/cc-switch-fixture/out/tauri-codegen-assets",
  );
  fs.mkdirSync(path.join(dist, "assets"), { recursive: true });
  fs.mkdirSync(cache, { recursive: true });
  fs.writeFileSync(
    path.join(dist, "index.html"),
    '<script type="module" src="/assets/app.js"></script><div id="root"></div>',
  );
  const script = Buffer.from('console.log("current production frontend");');
  const compressed = zlib.brotliCompressSync(script);
  fs.writeFileSync(path.join(dist, "assets/app.js"), script);
  fs.writeFileSync(path.join(cache, "content.js"), compressed);
  const html = zlib.brotliCompressSync(
    fs.readFileSync(path.join(dist, "index.html")),
  );
  const icon = Buffer.from('<svg><path d="M0 0"/></svg>');
  const svg = zlib.brotliCompressSync(icon);
  fs.writeFileSync(path.join(dist, "icon.svg"), icon);
  fs.writeFileSync(path.join(cache, "content.html"), html);
  fs.writeFileSync(path.join(cache, "content.svg"), svg);
  const executable = path.join(target, "release/cc-switch.exe");
  fs.writeFileSync(
    executable,
    Buffer.concat([
      Buffer.from(
        "MZ__TAURI_BUNDLE_TYPE_VAR_UNK/assets/app.js/index.html/icon.svg",
      ),
      compressed,
      html,
      svg,
    ]),
  );
  return { root, dist, target, executable };
}

function verify(f) {
  return spawnSync(
    process.execPath,
    [
      path.join(repo, "scripts/verify-production-assets.cjs"),
      "--dist",
      f.dist,
      "--target",
      f.target,
      "--executable",
      f.executable,
    ],
    { encoding: "utf8" },
  );
}

function cleanup(f) {
  assert.equal(path.dirname(path.resolve(f.root)), tempRoot);
  fs.rmSync(f.root, { recursive: true, force: true });
}

test("verifies the shipped compressed frontend against current dist", () => {
  const f = fixture();
  try {
    const result = verify(f);
    assert.equal(result.status, 0, result.stderr);
    const proof = JSON.parse(result.stdout);
    assert.equal(proof.embedded, true);
    assert.equal(proof.assets[0].path, "assets/app.js");
  } finally {
    cleanup(f);
  }
});

test("rejects dist whose HTML points at a missing entry point", () => {
  const f = fixture();
  try {
    fs.writeFileSync(
      path.join(f.dist, "index.html"),
      '<script src="/assets/missing.js"></script>',
    );
    const result = verify(f);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /entry point/);
  } finally {
    cleanup(f);
  }
});

test("rejects a development executable without embedded frontend", () => {
  const f = fixture();
  try {
    fs.writeFileSync(
      f.executable,
      "MZ__TAURI_BUNDLE_TYPE_VAR_UNKhttp://localhost:3000",
    );
    const result = verify(f);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /not embedded/);
  } finally {
    cleanup(f);
  }
});

test("rejects an executable from an older frontend build", () => {
  const f = fixture();
  try {
    fs.writeFileSync(
      path.join(f.dist, "assets/app.js"),
      'console.log("new frontend");',
    );
    const result = verify(f);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /not embedded/);
  } finally {
    cleanup(f);
  }
});

test("rejects a placeholder dist directory", () => {
  const f = fixture();
  try {
    fs.writeFileSync(path.join(f.dist, "index.html"), "<html></html>");
    const result = verify(f);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /entry point/);
  } finally {
    cleanup(f);
  }
});

test("rejects a stale embedded HTML document with unchanged JavaScript", () => {
  const f = fixture();
  try {
    fs.writeFileSync(
      path.join(f.dist, "index.html"),
      '<script type="module" src="/assets/app.js"></script><div id="different-root"></div>',
    );
    const result = verify(f);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /not embedded/);
  } finally {
    cleanup(f);
  }
});

test("rejects a stale embedded static resource with unchanged HTML and JavaScript", () => {
  const f = fixture();
  try {
    fs.writeFileSync(path.join(f.dist, "icon.svg"), "<svg>changed</svg>");
    const result = verify(f);
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /not embedded/);
  } finally {
    cleanup(f);
  }
});

test("accepts Tauri HTML serialization and generated nonce changes", () => {
  const f = fixture();
  try {
    fs.writeFileSync(
      path.join(f.dist, "index.html"),
      '<!doctype html><html><head><style data-name="theme">body{color:red}</style><script type="module" src="/assets/app.js"></script></head><body></body></html>',
    );
    const mapped = Buffer.from(
      '<!DOCTYPE html><html><head><style nonce="__TAURI_STYLE_NONCE__" data-name="theme">body{color:red}</style><script src="/assets/app.js" type="module"></script></head><body></body></html>',
    );
    const compressed = zlib.brotliCompressSync(mapped);
    fs.writeFileSync(
      path.join(
        f.target,
        "release/build/cc-switch-fixture/out/tauri-codegen-assets/mapped.html",
      ),
      compressed,
    );
    fs.appendFileSync(f.executable, compressed);
    const result = verify(f);
    assert.equal(result.status, 0, result.stderr);
  } finally {
    cleanup(f);
  }
});
