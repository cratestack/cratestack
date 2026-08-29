const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const pkg = require("../package.json");

const extensionRoot = path.resolve(__dirname, "..");

// The `.cstack` file-explorer icon. Distinct from the extension's
// gallery icon (`icon.png`, guarded by icon.test.js): that one brands
// the Marketplace/Open VSX listing, this one brands the file rows in
// the explorer tree. Different mechanism, different asset, different
// failure mode.
//
// `contributes.languages[].icon` is a *fallback*: VS Code shows it only
// when the active file icon theme has no icon of its own for this
// language or extension, and does not set `showLanguageModeIcons:
// false`. Nothing about a missing or malformed icon here fails a build,
// a lint, or `vsce package` — the explorer just silently falls back to
// the theme's generic file glyph, which is indistinguishable from "no
// icon was ever contributed".
//
// The engines floor is asserted for the same reason. Language icons
// landed in VS Code 1.64 (microsoft/vscode#14662); on anything older the
// contribution parses fine and is simply ignored. Lowering
// `engines.vscode` below that would silently un-ship this feature for
// the users on the versions the lower floor was widened to reach.

const LANGUAGE_ICONS_MINIMUM = { major: 1, minor: 64 };

function cstackLanguage() {
  const languages = pkg.contributes?.languages ?? [];
  return languages.find((l) => l.id === "cstack");
}

test("the cstack language contributes a light and dark file icon", () => {
  const language = cstackLanguage();
  assert.ok(language, "no `cstack` entry in contributes.languages");

  assert.ok(
    language.icon && typeof language.icon === "object",
    "the `cstack` language declares no `icon` — without it the explorer falls back to " +
      "the icon theme's generic file glyph for every .cstack file",
  );
  for (const variant of ["light", "dark"]) {
    assert.ok(
      typeof language.icon[variant] === "string" && language.icon[variant].length > 0,
      `contributes.languages[cstack].icon is missing a non-empty "${variant}" path`,
    );
  }
});

test("both icon variants resolve to files that exist", () => {
  const { icon } = cstackLanguage();

  for (const variant of ["light", "dark"]) {
    const iconPath = path.join(extensionRoot, icon[variant]);
    assert.ok(
      fs.existsSync(iconPath) && fs.statSync(iconPath).isFile(),
      `contributes.languages[cstack].icon.${variant} is "${icon[variant]}", but no file ` +
        `exists at ${iconPath}. A dangling path here is silent: the explorer shows the ` +
        `theme's default glyph and nothing reports an error.`,
    );
  }
});

test("both icon variants are SVG", () => {
  const { icon } = cstackLanguage();

  for (const variant of ["light", "dark"]) {
    const iconPath = path.join(extensionRoot, icon[variant]);
    assert.match(
      icon[variant],
      /\.svg$/,
      `contributes.languages[cstack].icon.${variant} should be an .svg — the explorer ` +
        `renders these at 16x16, where a raster asset sized for anything else blurs`,
    );
    // Strip an XML declaration and any leading comments before looking
    // for the root element, so the licence/derivation comments these
    // files carry don't make the check fail on a valid SVG.
    const source = fs
      .readFileSync(iconPath, "utf8")
      .replace(/^﻿/, "")
      .replace(/<\?xml[\s\S]*?\?>/g, "")
      .replace(/<!--[\s\S]*?-->/g, "")
      .trim();
    assert.ok(
      source.startsWith("<svg"),
      `${icon[variant]} has an .svg extension but its root element is not <svg>`,
    );
  }
});

test("the engines.vscode floor is new enough for language icons", () => {
  const declared = pkg.engines?.vscode;
  assert.ok(declared, "package.json declares no engines.vscode");

  const match = /^\^?(\d+)\.(\d+)/.exec(declared);
  assert.ok(match, `could not parse a major.minor out of engines.vscode "${declared}"`);

  const [major, minor] = [Number(match[1]), Number(match[2])];
  const supported =
    major > LANGUAGE_ICONS_MINIMUM.major ||
    (major === LANGUAGE_ICONS_MINIMUM.major && minor >= LANGUAGE_ICONS_MINIMUM.minor);

  assert.ok(
    supported,
    `engines.vscode is "${declared}", but contributes.languages[].icon requires at least ` +
      `${LANGUAGE_ICONS_MINIMUM.major}.${LANGUAGE_ICONS_MINIMUM.minor} ` +
      `(microsoft/vscode#14662). Below that floor the icon contribution is parsed and ` +
      `silently ignored, so lowering the floor would un-ship the file icon for exactly ` +
      `the users the lower floor was meant to include.`,
  );
});
