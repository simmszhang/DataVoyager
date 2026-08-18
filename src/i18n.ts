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

export default i18n;
