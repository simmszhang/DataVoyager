import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./locales/zh-CN.json";
import enUS from "./locales/en-US.json";

const saved = localStorage.getItem("dby.lang");
const sysLang = (navigator.language || "en").toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";

i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zhCN },
    "en-US": { translation: enUS },
  },
  lng: saved || sysLang,
  fallbackLng: "zh-CN",
  interpolation: { escapeValue: false },
});

// Persist language switches so the choice survives restarts.
i18n.on("languageChanged", (lng) => {
  localStorage.setItem("dby.lang", lng);
});

/// 统一错误转字符串：纯函数用 i18n 实例（非 useTranslation）。
/// 覆盖三态：字符串原样返回 / Tauri 对象 `{kind,message}` 按 kind 本地化 /
/// 未知对象兜底 error.other。绝不 String() 非字符串对象（避免 [object Object]，#19/#55）。
export function errToString(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) {
    const msg = String((e as any).message);
    const kind = (e as any).kind;
    if (typeof kind === "string" && kind) {
      return i18n.t(`error.${kind}`, { message: msg, defaultValue: i18n.t("error.other") });
    }
    return msg;
  }
  return i18n.t("error.other");
}

export default i18n;
