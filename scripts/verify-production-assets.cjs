// Verify actual embedded bytes; command-line flags and filenames alone are not evidence.
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const zlib = require("node:zlib");
const { JSDOM } = require("jsdom");

function canonicalHtml(bytes) {
  const dom = new JSDOM(bytes.toString("utf8"));
  for (const element of dom.window.document.querySelectorAll("*")) {
    const token = element.getAttribute("nonce");
    if (
      (element.tagName === "STYLE" && token === "__TAURI_STYLE_NONCE__") ||
      (element.tagName === "SCRIPT" && token === "__TAURI_SCRIPT_NONCE__")
    ) {
      element.removeAttribute("nonce");
    }
    const attributes = [...element.attributes].sort((a, b) =>
      a.name.localeCompare(b.name),
    );
    for (const attribute of [...element.attributes])
      element.removeAttributeNode(attribute);
    for (const attribute of attributes)
      element.setAttributeNS(
        attribute.namespaceURI,
        attribute.name,
        attribute.value,
      );
  }
  const serialized = dom.serialize();
  dom.window.close();
  return serialized;
}

function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest("hex");
}

function files(root, relative = "") {
  return fs
    .readdirSync(path.join(root, relative), { withFileTypes: true })
    .flatMap((entry) => {
      const name = path.posix.join(relative, entry.name);
      if (entry.isSymbolicLink())
        throw new Error(`Frontend/build asset is a link: ${name}`);
      return entry.isDirectory() ? files(root, name) : [name];
    })
    .sort();
}

function verify({ dist, target, executable }) {
  const inventory = files(dist);
  const html = fs.readFileSync(path.join(dist, "index.html"), "utf8");
  const entries = [
    ...html.matchAll(/<script\b[^>]*\bsrc=["']([^"']+\.js)["']/gi),
  ]
    .map((match) => match[1])
    .filter((name) => !/^(?:\w+:|\/\/)/.test(name))
    .map((name) => path.posix.normalize(name.replace(/^\//, "")));
  if (!entries.length || entries.some((name) => !inventory.includes(name))) {
    throw new Error("Production dist has no JavaScript entry point");
  }
  const frontend = inventory.filter((name) => /\.(?:js|css)$/i.test(name));
  if (!frontend.length)
    throw new Error("Production dist has no frontend assets");
  const binary = fs.readFileSync(executable);
  const build = path.join(target, "release/build");
  const cached = [];
  for (const entry of fs.readdirSync(build, { withFileTypes: true })) {
    if (
      !entry.name.startsWith("cc-switch-") ||
      !entry.isDirectory() ||
      entry.isSymbolicLink()
    )
      continue;
    const directory = path.join(build, entry.name, "out/tauri-codegen-assets");
    if (!fs.existsSync(directory)) continue;
    for (const name of files(directory)) {
      const compressed = fs.readFileSync(path.join(directory, name));
      try {
        const decoded = zlib.brotliDecompressSync(compressed);
        cached.push({
          bytes: compressed,
          hash: sha256(decoded),
          html: /\.html$/i.test(name) ? canonicalHtml(decoded) : null,
        });
      } catch {
        // Other cached builds may have compression disabled; they cannot prove this build.
      }
    }
  }
  const assets = inventory.map((name) => {
    const content = fs.readFileSync(path.join(dist, name));
    const hash = sha256(content);
    const normalizedHtml = /\.html$/i.test(name)
      ? canonicalHtml(content)
      : null;
    if (
      !cached.some(
        (asset) =>
          (normalizedHtml !== null
            ? asset.html === normalizedHtml
            : asset.hash === hash) && binary.includes(asset.bytes),
      ) ||
      !binary.includes(Buffer.from(name))
    ) {
      throw new Error(
        `Current frontend is not embedded in executable: ${name}`,
      );
    }
    return { path: name, sha256: hash };
  });
  const digest = inventory
    .map((name) => `${name}\0${sha256(fs.readFileSync(path.join(dist, name)))}`)
    .join("\n");
  return {
    embedded: true,
    executableSha256: sha256(binary),
    distSha256: sha256(Buffer.from(digest)),
    assets,
  };
}

try {
  const args = process.argv.slice(2);
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    const key = args[index].slice(2);
    if (
      !["dist", "target", "executable"].includes(key) ||
      !args[index + 1] ||
      options[key]
    ) {
      throw new Error(
        "Usage: --dist PATH --target CARGO_TARGET --executable EXE",
      );
    }
    options[key] = path.resolve(args[index + 1]);
  }
  if (Object.keys(options).length !== 3)
    throw new Error("dist, target and executable are required");
  process.stdout.write(JSON.stringify(verify(options)) + "\n");
} catch (error) {
  process.stderr.write(
    `Production asset verification failed: ${error.message}\n`,
  );
  process.exitCode = 1;
}
