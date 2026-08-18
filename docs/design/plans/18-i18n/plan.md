# #18 i18n — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 接入 i18next/react-i18next，前端文案 key 化，提供 zh-CN/en-US 两套资源，语言可切换，统一错误提示。

**Architecture:** `src/i18n.ts` 初始化 + `src/locales/{zh-CN,en-US}.json`；组件 `t()`/`<Trans>`；`errToString` + 错误 kind 映射。

**Tech Stack:** React 19 + TypeScript + Vite、i18next、react-i18next。

**Spec:** `docs/design/plans/18-i18n/design.md`

## Global Constraints

- 全前端禁止新增硬编码中文文案（资源文件除外）。
- key 约定 `域.作用域.标签`；两套资源同 key 对齐。
- 语言持久化 `localStorage["dby.lang"]`，默认跟随 `navigator.language`。
- CI 门禁：`pnpm build`（`tsc && vite build`）。

---

### Task 1: 依赖 + 初始化 + 语言切换

**Files:**
- Create: `src/i18n.ts`、`src/locales/zh-CN.json`、`src/locales/en-US.json`
- Modify: `src/main.tsx`（引入 i18n）、`package.json`（依赖）

- [ ] **Step 1: 安装依赖**

Run: `pnpm add i18next react-i18next`

- [ ] **Step 2: 写初始化**

按 design 4.1 写 `src/i18n.ts` 与两套资源骨架（先放 `app.empty.noConnection` 一个 key 验证链路）。

- [ ] **Step 3: 运行确认通过**

Run: `pnpm build`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/i18n.ts src/locales/zh-CN.json src/locales/en-US.json src/main.tsx package.json pnpm-lock.yaml
git commit -m "feat(i18n): scaffold i18next with zh-CN/en-US and language switch (#18)"
```

---

### Task 2: `App.tsx` 文案 key 化

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: 最小实现**

状态栏/按钮/弹窗标题等中文 → `t("...")`；`"已连接"/"查询失败"/"危险操作确认"` 等进资源文件。

- [ ] **Step 2: 运行确认通过**

Run: `pnpm build`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/App.tsx src/locales/zh-CN.json src/locales/en-US.json
git commit -m "feat(i18n): extract App strings (#18)"
```

---

### Task 3: 组件文案 key 化

**Files:**
- Modify: `src/components/ConnectionDialog.tsx`、`SchemaTree.tsx`、`QueryEditor.tsx`、`ResultsGrid.tsx`、`ExportDialog.tsx`、`HistoryPanel.tsx`、`CreateTableDialog.tsx`、`SchemaPanel.tsx`

- [ ] **Step 1: 最小实现**

逐组件替换中文为 `t()`/`<Trans>`，同步补两套资源。**遗漏点**：`ResultsGrid.tsx:17/:124` 两处独立 `"NULL"`（走 `useTranslation().t("grid.null")`，非 `displayCell`）；`window.prompt/confirm` 文案（`App.tsx:307/320/332`、`SchemaTree:77/86/94`）；`title/aria-label/placeholder` 属性文案。

- [ ] **Step 2: 运行确认通过**

Run: `pnpm build`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/components src/locales/zh-CN.json src/locales/en-US.json
git commit -m "feat(i18n): extract component strings (#18)"
```

---

### Task 4: `displayCell` + 错误提示统一（#19/#55）

**Files:**
- Modify: `src/api.ts`（`displayCell` 的 `"NULL"`）、`src/utils.ts`（或 `src/i18n.ts`，新增 `errToString`）
- Modify: `src/App.tsx`（catch 里 `String(e)` → `errToString(e)`）

**Interfaces:**
- Produces: `export function errToString(e: unknown): string`

- [ ] **Step 1: 最小实现**

- **`errToString` 是纯函数，用 `import i18n from "./i18n"; i18n.t(...)`**（非 `useTranslation`/裸 `t`）；**绝不对非字符串对象 `String()`**（避免 `[object Object]`）：`{kind}` → `i18n.t(`error.${kind}`, { message, defaultValue: i18n.t("error.other") })`（未知 kind/缺 message 兜底 `error.other`）；`{message}` → 返回 message；未知 → `i18n.t("error.other")`。
- `error.*` 8 kind key 进资源（另加 `error.other` 兜底）。
- **`displayCell`（`api.ts`）不本地化**：保持返回 `"NULL"` 作为数据标记；`ResultsGrid` 用 `useTranslation().t("grid.null")` 渲染 null（订阅 `languageChanged`，切语言即时更新）。
- `App.tsx` 等 25 处 `String(e)` → `errToString(e)`。

- [ ] **Step 2: 运行确认通过**

Run: `pnpm build`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/api.ts src/utils.ts src/App.tsx src/locales/zh-CN.json src/locales/en-US.json
git commit -m "fix(frontend): unified errToString + localized errors (#19/#55)"
```

---

### Task 5: 键对齐校验 + 全量回归

**Files:**
- Create: `src/locales/check-keys.mjs`（或并入构建脚本）

**Interfaces:**
- 无

- [ ] **Step 1: 写键对齐脚本**

```js
// 1) 比较 zh-CN.json 与 en-US.json 的顶层 key 集合，不一致则退出非 0
// 2) CJK 正则 grep src/**/*.{ts,tsx}，断言除 locales/ 外无硬编码中文（对齐成功标准）
// 3) 提取代码中 t("...")/i18n.t("...") 的 key，断言在两套资源均存在
```

- [ ] **Step 2: 运行确认通过**

Run: `node src/locales/check-keys.mjs` && `pnpm build`
Expected: PASS（两套资源 key 一致）

- [ ] **Step 3: Commit**

```bash
git add src/locales/check-keys.mjs package.json
git commit -m "test(i18n): add key-alignment check (#18)"
```
