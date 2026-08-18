# #18 i18n — 设计文档

> 状态：评审有条件通过（🔴 已修订，🟡 并入 plan.md） · 优先级 P3 · 规模：大 · 关联缺陷：#19/#55（错误提示统一化）· 依赖共享契约：S5（错误形状，非硬阻塞）

## 1. 现状与影响

- 前端全部文案硬编码中文：`App.tsx`（`"已连接"`/`"查询失败"`/`"危险操作确认"` 等）、`components/*.tsx`（`ConnectionDialog`/`SchemaTree`/`QueryEditor`/`ResultsGrid`/`ExportDialog`/`HistoryPanel`/`CreateTableDialog`）、`api.ts:25` 的 `displayCell` 返回 `"NULL"`、`ResultsGrid.tsx:17/:124` 两处独立 `"NULL"`。
- 无 i18n 框架、无语言切换（defects #18）。
- `catch { String(e) }` 共 25 处（`App.tsx:102/111/120/179/197/206/216/227/238/249/278/292/313/326/337/353`、`ConnectionDialog:63/76`、`SchemaTree:57/82/90/98`、`ExportDialog:27/39`、`CreateTableDialog:51`），Tauri 错误对象被串化成 `[object Object]`（#55，review F10）。
- 原生 `window.prompt/confirm` 文案（`App.tsx:307/320/332`、`SchemaTree:77/86/94`）与 `title/aria-label/placeholder` 属性文案同样未纳入。
- **影响**：无法国际化（计划提了首版中/英）；错误提示不可读。

## 2. 目标与成功标准

1. 接入 `i18next` + `react-i18next`，文案 key 化，首版提供 `zh-CN`/`en-US` 两套资源。
2. 语言可切换（localStorage 持久化，默认跟随系统），切换即时生效（含 `displayCell` 产生的 `"NULL"`）。
3. 错误提示统一：`errToString` + 错误 `kind` → 本地化消息（#19/#55）。
4. 成功标准：全前端 grep（含 CJK 正则）无硬编码中文文案（资源文件除外）；中/英切换后 UI 全量切换；错误提示不再是 `[object Object]`。

## 3. 方案对比

### 方案 A：i18next + react-i18next（推荐）
- 社区标准，React 19 兼容（react-i18next v15.0.0 起支持 React 19），`<Trans>`/`t()`/插值/复数完整。
- **优点**：生态成熟、懒加载、类型安全可选。**缺点**：引入依赖。

### 方案 B：自制轻量词典 + Context
- **缺点**：插值/复数/懒加载都要自己造，重复造轮子。

### 方案 C：仅抽常量，不做框架
- **缺点**：不满足「可切换」目标，否决。

**推荐 A**。

## 4. 推荐方案详细设计

### 4.1 初始化（`src/i18n.ts`，`main.tsx` 引入）

```ts
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./locales/zh-CN.json";
import enUS from "./locales/en-US.json";

const saved = localStorage.getItem("dby.lang");
const sysLang = (navigator.language || "en").toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";

i18n.use(initReactI18next).init({
  resources: { "zh-CN": { translation: zhCN }, "en-US": { translation: enUS } },
  lng: saved || sysLang,
  fallbackLng: "en-US",
  interpolation: { escapeValue: false },
});
export default i18n;
```

语言切换：顶栏按钮 `i18n.changeLanguage(next)` + `localStorage.setItem("dby.lang", next)`。

### 4.2 key 命名约定

`域.作用域.标签`：`app.empty.noConnection`、`editor.run`、`connection.dialog.title`、`history.search.placeholder`、`grid.null`、`danger.confirm.title`。中/英两份资源同 key 对齐。

### 4.3 迁移策略（含遗漏点）

逐组件把硬编码中文替换为 `t("...")` / `<Trans>`：`App.tsx`（状态栏/按钮/弹窗）→ `ConnectionDialog` → `SchemaTree` → `QueryEditor` → `ResultsGrid` → `ExportDialog` → `HistoryPanel` → `CreateTableDialog`。

- **`displayCell`（`api.ts`）是纯函数、不可用 `useTranslation`/裸 `t`**：保持返回 `"NULL"` 作为**数据标记**（非本地化文案）；`ResultsGrid` 在渲染 null 单元格时用 `useTranslation().t("grid.null")` 渲染本地化文案（订阅 `languageChanged`，切语言即时更新）。`ResultsGrid.tsx:17/:124` 两处独立 `"NULL"` 同样改 `t("grid.null")`。
- 覆盖 `window.prompt/confirm`（`App.tsx:307/320/332`、`SchemaTree:77/86/94`）→ 文案 key 化（prompt 文案经 `t()` 后传入）。
- 覆盖 `title/aria-label/placeholder` 属性文案。

### 4.4 错误提示统一（#19/#55，纯函数用 `i18n.t`）

```ts
import i18n from "./i18n"; // 纯函数用 i18n 实例，非 useTranslation

export function errToString(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object") {
    const o = e as { message?: string; kind?: string };
    if (o.kind) {
      const message = typeof o.message === "string" && o.message ? o.message : "";
      return i18n.t(`error.${o.kind}`, { message, defaultValue: i18n.t("error.other") }); // 未知 kind 回退
    }
    if (typeof o.message === "string") return o.message;
  }
  return i18n.t("error.other"); // 绝不 String() 非字符串对象（避免 [object Object]）
}
```

`error.*` key 覆盖 S5 的 8 个 kind（`database/driver_not_found/connection_not_found/unsupported/config/storage/cancelled/other`）；另加 `error.other`（未知/兜底）。迁移：`App.tsx`/`ConnectionDialog`/`SchemaTree`/`ExportDialog`/`CreateTableDialog` 的 25 处 `String(e)` 全部替换为 `errToString(e)`。

### 4.5 边界

- 仅前端文案 i18n；`dby-core`/壳层错误消息文本保持不变（后端消息经 kind 由前端映射本地化）。
- 与 #48（CSP）无冲突（bundled JSON，无内联脚本）。
- `Settings.theme` 不扩展 `language` 字段，语言用前端 `localStorage`。

## 5. 错误处理（遵循 S5）

- `errToString` 覆盖字符串 / Tauri 对象（`{kind,message}`）/ 未知三态，**绝不**对非字符串对象调用 `String()`（避免 `[object Object]`）；`Cancelled` 单值变体无 `message` 时 `message` 置空、由 `error.cancelled` key 提供完整文案，未知对象兜底到 `error.other`。

## 6. 测试策略

- **键完整性**：脚本断言 `zh-CN` 与 `en-US` 顶层 key 集合一致。
- **无遗漏**：CJG 正则 grep 前端源码，断言除 `locales/` 外无硬编码中文（与成功标准对齐）。
- **key 存在性**：脚本/`tsc` 校验代码中 `t("...")` 引用的 key 在两套资源均存在。
- **构建**：`pnpm build` 通过。

## 7. 回归风险与影响面

- 纯前端文案改动，无后端契约变化；风险在「遗漏文案」与「key 拼写」——靠 CJG grep + key 对齐/存在性测试兜底。
- 语言切换需全局 rerender（i18next 自带）；`displayCell` 产生的 `"NULL"` 由 `ResultsGrid` 的 `useTranslation` 渲染以随语言切换即时更新。

## 8. 关联缺陷处置

- #18：4.1–4.3；#19/#55：4.4 统一错误提示。

## 9. 与其它方案组的依赖

- 独立方案，可与其它组并行；依赖 S5（错误 kind）。**非硬阻塞**：当前 `DbError` 只序列化 `{"message"}`（无 kind），`errToString` 会走 `message` 分支仍可工作；S5/#29 落地后 `kind` 分支才生效，故不阻塞本方案排期。
