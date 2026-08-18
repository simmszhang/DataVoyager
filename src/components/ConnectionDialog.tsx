import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, ConnectParams, DriverInfo } from "../api";
import { errToString } from "../i18n";

interface Props {
  drivers: DriverInfo[];
  projectId: string | null;
  onConnected: () => void;
  onClose: () => void;
}

export default function ConnectionDialog({ drivers, projectId, onConnected, onClose }: Props) {
  const { t } = useTranslation();
  const [form, setForm] = useState({
    driver: drivers[0]?.id ?? "mysql",
    host: "127.0.0.1",
    port: "3306",
    user: "root",
    password: "",
    database: "",
    ssl: false,
    verifyCert: true,
    ssh: false,
    sshHost: "",
    sshPort: "22",
    sshUser: "",
    sshPassword: "",
    sshFingerprint: "",
    save: true,
    rememberPassword: true,
  });
  const [busy, setBusy] = useState<null | "test" | "connect">(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [pendingFingerprint, setPendingFingerprint] = useState<{
    fp: string;
    params: ConnectParams;
    mode: "test" | "connect";
  } | null>(null);

  const params = (): ConnectParams => ({
    driver: form.driver,
    host: form.host.trim(),
    port: Number(form.port) || 3306,
    user: form.user.trim(),
    password: form.password,
    database: form.database.trim() || null,
    ssl: form.ssl ? { enabled: true, verify_cert: form.verifyCert } : null,
    ssh: form.ssh
      ? {
          enabled: true,
          host: form.sshHost.trim(),
          port: Number(form.sshPort) || 22,
          user: form.sshUser.trim(),
          password: form.sshPassword || null,
          host_key_fingerprint: form.sshFingerprint || undefined,
        }
      : null,
  });

  const set = (key: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) =>
    setForm((f) => {
      const next = { ...f, [key]: e.target.value };
      // SSH 参数变更后，已确认的指纹不再适用（TOFU 以 host:port 为锚）
      if (key.startsWith("ssh") && key !== "sshFingerprint") next.sshFingerprint = "";
      return next;
    });

  const setBool =
    (key: "ssl" | "verifyCert" | "ssh" | "save" | "rememberPassword") =>
    (e: React.ChangeEvent<HTMLInputElement>) =>
      setForm((f) => ({ ...f, [key]: e.target.checked }));

  /// 前置 TOFU：SSH 开启且尚无已信任指纹时，先探针取指纹，弹窗确认后由
  /// `confirmFingerprint` 携带指纹重试；返回 null 表示等待用户确认。
  async function ensureFingerprint(
    p: ConnectParams,
    mode: "test" | "connect",
  ): Promise<ConnectParams | null> {
    if (p.ssh?.enabled && !p.ssh.host_key_fingerprint) {
      const fp = await api.probeHostKey(p);
      setPendingFingerprint({ fp, params: p, mode });
      return null;
    }
    return p;
  }

  async function handleTest() {
    setBusy("test");
    setMessage(null);
    try {
      const p = params();
      const ready = await ensureFingerprint(p, "test");
      if (!ready) return; // 等待指纹确认
      const version = await api.testConnection(ready);
      setMessage({ ok: true, text: t("connection.dialog.testSuccess", { version }) });
    } catch (e) {
      setMessage({ ok: false, text: errToString(e) });
    } finally {
      setBusy(null);
    }
  }

  async function handleConnect() {
    setBusy("connect");
    setMessage(null);
    try {
      const p = params();
      const ready = await ensureFingerprint(p, "connect");
      if (!ready) return; // 等待指纹确认
      await api.connect(ready, projectId, form.save, form.rememberPassword);
      onConnected();
    } catch (e) {
      setMessage({ ok: false, text: errToString(e) });
    } finally {
      setBusy(null);
    }
  }

  /// 用户确认指纹：同一会话内回填表单避免重复确认，并按原意图（测试/连接）重试。
  async function confirmFingerprint() {
    if (!pendingFingerprint) return;
    const { fp, params: p, mode } = pendingFingerprint;
    setPendingFingerprint(null);
    setForm((f) => ({ ...f, sshFingerprint: fp }));
    const ready: ConnectParams = {
      ...p,
      ssh: p.ssh ? { ...p.ssh, host_key_fingerprint: fp } : p.ssh,
    };
    setBusy(mode);
    setMessage(null);
    try {
      if (mode === "test") {
        const version = await api.testConnection(ready);
        setMessage({ ok: true, text: t("connection.dialog.testSuccess", { version }) });
      } else {
        await api.connect(ready, projectId, form.save, form.rememberPassword);
        onConnected();
      }
    } catch (e) {
      setMessage({ ok: false, text: errToString(e) });
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>{t("connection.dialog.title")}</h2>
          <button className="icon-btn" onClick={onClose} aria-label={t("connection.dialog.close")}>
            ✕
          </button>
        </div>

        <div className="modal-body">
          <div className="form-grid">
            <label>
              <span>{t("connection.dialog.driver")}</span>
              <select value={form.driver} onChange={set("driver")}>
                {drivers.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.display_name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>{t("connection.dialog.host")}</span>
              <input value={form.host} onChange={set("host")} placeholder="127.0.0.1" />
            </label>
            <label>
              <span>{t("connection.dialog.port")}</span>
              <input value={form.port} onChange={set("port")} placeholder="3306" />
            </label>
            <label>
              <span>{t("connection.dialog.user")}</span>
              <input value={form.user} onChange={set("user")} placeholder="root" />
            </label>
            <label>
              <span>{t("connection.dialog.password")}</span>
              <input type="password" value={form.password} onChange={set("password")} />
            </label>
            <label>
              <span>{t("connection.dialog.databaseOptional")}</span>
              <input
                value={form.database}
                onChange={set("database")}
                placeholder={t("connection.dialog.databasePlaceholder")}
              />
            </label>
            <div className="span-2 checkbox-row">
              <label className="checkbox">
                <input type="checkbox" checked={form.ssl} onChange={setBool("ssl")} />
                <span>{t("connection.dialog.useSsl")}</span>
              </label>
              {form.ssl && (
                <label className="checkbox">
                  <input type="checkbox" checked={form.verifyCert} onChange={setBool("verifyCert")} />
                  <span>{t("connection.dialog.verifyCert")}</span>
                </label>
              )}
              <label className="checkbox">
                <input type="checkbox" checked={form.ssh} onChange={setBool("ssh")} />
                <span>{t("connection.dialog.useSsh")}</span>
              </label>
              <label className="checkbox">
                <input type="checkbox" checked={form.save} onChange={setBool("save")} />
                <span>{t("connection.dialog.save")}</span>
              </label>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={form.rememberPassword}
                  onChange={setBool("rememberPassword")}
                />
                <span>{t("connection.dialog.rememberPassword")}</span>
              </label>
            </div>
            {form.ssh && (
              <>
                <label>
                  <span>{t("connection.dialog.sshHost")}</span>
                  <input value={form.sshHost} onChange={set("sshHost")} placeholder="ssh.example.com" />
                </label>
                <label>
                  <span>{t("connection.dialog.sshPort")}</span>
                  <input value={form.sshPort} onChange={set("sshPort")} placeholder="22" />
                </label>
                <label>
                  <span>{t("connection.dialog.sshUser")}</span>
                  <input value={form.sshUser} onChange={set("sshUser")} placeholder="user" />
                </label>
                <label>
                  <span>{t("connection.dialog.sshPassword")}</span>
                  <input type="password" value={form.sshPassword} onChange={set("sshPassword")} />
                </label>
              </>
            )}
          </div>

          {message && (
            <div className={`form-message ${message.ok ? "ok" : "err"}`}>{message.text}</div>
          )}
        </div>

        <div className="modal-footer">
          <button className="btn" onClick={handleTest} disabled={busy !== null}>
            {busy === "test" ? t("connection.dialog.testing") : t("connection.dialog.test")}
          </button>
          <button className="btn primary" onClick={handleConnect} disabled={busy !== null}>
            {busy === "connect" ? t("connection.dialog.connecting") : t("connection.dialog.connect")}
          </button>
        </div>
      </div>

      {pendingFingerprint && (
        <div className="modal-backdrop" onClick={(e) => e.stopPropagation()}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h2>{t("connection.dialog.fingerprintTitle")}</h2>
              <button
                className="icon-btn"
                onClick={() => setPendingFingerprint(null)}
                aria-label={t("connection.dialog.close")}
              >
                ✕
              </button>
            </div>
            <div className="modal-body">
              <p className="danger-hint">{t("connection.dialog.fingerprintBody")}</p>
              <pre className="danger-sql">{pendingFingerprint.fp}</pre>
              <p className="muted">{t("connection.dialog.fingerprintNote")}</p>
            </div>
            <div className="modal-footer">
              <button className="btn" onClick={() => setPendingFingerprint(null)}>
                {t("connection.dialog.cancel")}
              </button>
              <button className="btn primary" onClick={confirmFingerprint}>
                {t(
                  pendingFingerprint.mode === "test"
                    ? "connection.dialog.trustAndTest"
                    : "connection.dialog.trustAndConnect",
                )}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
