#!/usr/bin/env node
/**
 * check-keys.mjs — i18n key-alignment guard for dby (#18)
 *
 * Three checks; exits non-zero on any failure:
 *   1. Key parity: flattened key sets of zh-CN.json vs en-US.json must match.
 *   2. No hardcoded CJK: scan src/**\/*.{ts,tsx} (locales/ excluded) for CJK
 *      characters outside comments (// line and /* *\/ block comments are
 *      stripped first; CJK inside string literals/JSX is still reported).
 *   3. Key existence: every t("...") / i18n.t("...") / t('...') string-literal
 *      key used in src must exist in BOTH locale files (flattened).
 *
 * Usage: node src/locales/check-keys.mjs
 */

import { readFileSync, readdirSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = fileURLToPath(new URL(".", import.meta.url)); // <root>/src/locales/
const ROOT = join(HERE, "..", "..");
const SRC = join(ROOT, "src");
const LOCALES = join(SRC, "locales");
const ZH = join(LOCALES, "zh-CN.json");
const EN = join(LOCALES, "en-US.json");

const CJK_RE = /[\u4e00-\u9fff]/;
const TKEY_RE = /\bt\(\s*(["'])((?:(?!\1).)+)\1\s*\)/g;

const failures = [];
function fail(msg) {
  failures.push(msg);
  console.error(`FAIL: ${msg}`);
}

/** Flatten a nested object into dotted key paths (leaf values only). */
function flattenKeys(obj, prefix = "", out = []) {
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k;
    if (v && typeof v === "object" && !Array.isArray(v)) flattenKeys(v, path, out);
    else out.push(path);
  }
  return out;
}

/**
 * Remove // line comments and /* *\/ block comments while preserving newlines
 * so reported line numbers stay aligned with the original file. String literals
 * (with backslash escapes) are kept intact, so hardcoded CJK inside strings is
 * still detected and quoted comment markers inside strings are not misparsed.
 */
function stripComments(src) {
  let out = "";
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    const nx = src[i + 1];
    if (c === "/" && nx === "/") {
      while (i < n && src[i] !== "\n") i++; // drop comment, keep the newline
      continue;
    }
    if (c === "/" && nx === "*") {
      i += 2;
      while (i < n && !(src[i] === "*" && src[i + 1] === "/")) {
        if (src[i] === "\n") out += "\n"; // keep line structure
        i++;
      }
      i += 2;
      continue;
    }
    if (c === '"' || c === "'" || c === "`") {
      const q = c;
      out += q;
      i++;
      while (i < n) {
        const sc = src[i];
        if (sc === "\\") {
          out += sc;
          i++;
          if (i < n) {
            out += src[i];
            i++;
          }
          continue;
        }
        out += sc;
        i++;
        if (sc === q) break;
      }
      continue;
    }
    out += c;
    i++;
  }
  return out;
}

/** Recursively collect src/**\/*.{ts,tsx}, excluding the locales/ directory. */
function collectSourceFiles() {
  const files = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const p = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (p === LOCALES) continue; // resources, not source code
        walk(p);
      } else if (/\.(ts|tsx)$/.test(entry.name)) {
        files.push(p);
      }
    }
  };
  walk(SRC);
  return files.sort();
}

const rel = (p) => relative(ROOT, p).split(sep).join("/");

// ---- load locales -----------------------------------------------------------
let zh;
let en;
try {
  zh = JSON.parse(readFileSync(ZH, "utf8"));
  en = JSON.parse(readFileSync(EN, "utf8"));
} catch (e) {
  console.error(`FAIL: cannot read locale files: ${e.message}`);
  process.exit(1);
}

// ---- 1) key parity -----------------------------------------------------------
const zhKeys = flattenKeys(zh);
const enKeys = flattenKeys(en);
const zhSet = new Set(zhKeys);
const enSet = new Set(enKeys);
const onlyZh = zhKeys.filter((k) => !enSet.has(k));
const onlyEn = enKeys.filter((k) => !zhSet.has(k));
if (onlyZh.length || onlyEn.length) {
  onlyZh.forEach((k) => fail(`key "${k}" exists in zh-CN.json but missing in en-US.json`));
  onlyEn.forEach((k) => fail(`key "${k}" exists in en-US.json but missing in zh-CN.json`));
} else {
  console.log(
    `PASS key parity: ${zhKeys.length} keys identical across zh-CN.json / en-US.json`,
  );
}

// ---- 2) no hardcoded CJK outside comments -------------------------------------
const files = collectSourceFiles();
const cjkHits = [];
for (const file of files) {
  const stripped = stripComments(readFileSync(file, "utf8"));
  const lines = stripped.split("\n");
  for (let idx = 0; idx < lines.length; idx++) {
    if (CJK_RE.test(lines[idx])) {
      cjkHits.push(`${rel(file)}:${idx + 1}: ${lines[idx].trim().slice(0, 120)}`);
    }
  }
}
if (cjkHits.length) {
  cjkHits.forEach((h) => fail(`hardcoded CJK outside comments: ${h}`));
} else {
  console.log(
    `PASS no hardcoded CJK: ${files.length} source files scanned, 0 hits outside comments`,
  );
}

// ---- 3) key existence ----------------------------------------------------------
const used = new Map(); // key -> first usage {file, line}
for (const file of files) {
  const stripped = stripComments(readFileSync(file, "utf8"));
  for (const m of stripped.matchAll(TKEY_RE)) {
    const key = m[2];
    const line = stripped.slice(0, m.index).split("\n").length;
    if (!used.has(key)) used.set(key, { file: rel(file), line });
  }
}
const missingKeys = [];
for (const [key, where] of used) {
  if (!zhSet.has(key) || !enSet.has(key)) {
    missingKeys.push(`${key} (used at ${where.file}:${where.line})`);
  }
}
if (missingKeys.length) {
  missingKeys.forEach((k) => fail(`t() key missing in locale files: ${k}`));
} else {
  console.log(
    `PASS key existence: ${used.size} t() keys used, all present in both locales`,
  );
}

// ---- summary -------------------------------------------------------------------
if (failures.length) {
  console.error(
    `\ni18n check FAILED (${failures.length} problem${failures.length > 1 ? "s" : ""})`,
  );
  process.exit(1);
}
console.log("\ni18n check: PASS");
process.exit(0);
