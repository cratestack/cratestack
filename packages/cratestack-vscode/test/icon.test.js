const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const pkg = require("../package.json");

const extensionRoot = path.resolve(__dirname, "..");

// cratestack#782. The extension shipped without an `icon` for its whole
// life, so the Extensions sidebar and both registry listings rendered the
// generic grey placeholder box.
//
// Two things can silently undo that, and this file guards both. The field
// can be dropped from `package.json` (a merge resolution, a manifest
// rewrite), or the file it names can be moved or renamed while the field
// stays put — neither of which breaks a build, a lint, or a `vsce
// package`. `vsce` is not a backstop here: it only validates the icon
// when it can find one, so a dangling path is a listing regression that
// ships green.
//
// Deliberately NOT asserted here: that the icon is inside the built
// `.vsix`. `.vscodeignore` is a denylist, so a future entry could exclude
// a file that exists on disk and passes every assertion below — the
// source tree is the wrong place to look for that. `unzip -l ./*.vsix`
// after `pnpm run package:vsix` is what checks it, per the ticket's test
// plan, and that needs a release build of `cratestack-lsp` (via
// `stage-server`), so it is not something `node --test` can do offline.

test("package.json declares an icon", () => {
  assert.ok(
    typeof pkg.icon === "string" && pkg.icon.length > 0,
    "package.json must declare a non-empty `icon` — without it the Marketplace, " +
      "Open VSX, and the in-editor Extensions sidebar all fall back to the generic " +
      "placeholder box (cratestack#782)",
  );
});

test("the declared icon resolves to a file that exists", () => {
  const iconPath = path.join(extensionRoot, pkg.icon);
  assert.ok(
    fs.existsSync(iconPath),
    `package.json declares "icon": "${pkg.icon}", but no file exists at ${iconPath}. ` +
      "A dangling icon path does not fail the build or `vsce package` — it just " +
      "ships a placeholder listing.",
  );
  assert.ok(fs.statSync(iconPath).isFile(), `${pkg.icon} exists but is not a file`);
});

test("the icon is a PNG, square, and at least 128x128", () => {
  const iconPath = path.join(extensionRoot, pkg.icon);
  const buf = fs.readFileSync(iconPath);

  // The Marketplace rejects SVG outright and enforces a size floor, and a
  // rejected publish is expensive to discover late. Read the PNG header
  // directly rather than taking a dependency on an image library for
  // eight bytes of signature and eight of IHDR: bytes 0-7 are the PNG
  // signature, 12-15 the "IHDR" chunk type, then width and height as
  // big-endian uint32s.
  assert.equal(
    buf.subarray(0, 8).toString("hex"),
    "89504e470d0a1a0a",
    `${pkg.icon} is not a PNG. The Marketplace rejects SVG icons, so this must be a raster PNG.`,
  );
  assert.equal(
    buf.subarray(12, 16).toString("ascii"),
    "IHDR",
    `${pkg.icon} has a PNG signature but no IHDR chunk where one is required`,
  );

  const width = buf.readUInt32BE(16);
  const height = buf.readUInt32BE(20);
  assert.equal(
    width,
    height,
    `${pkg.icon} is ${width}x${height} — the icon must be square, or listings letterbox it`,
  );
  assert.ok(
    width >= 128,
    `${pkg.icon} is ${width}x${height} — the Marketplace's documented minimum is 128x128`,
  );
});
