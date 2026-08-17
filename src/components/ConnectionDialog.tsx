import { useState } from "react";
import { api, ConnectParams, DriverInfo } from "../api";

interface Props {
  drivers: DriverInfo[];
  projectId: string | null;
  onConnected: () => void;
  onClose: () => void;
}

export default function ConnectionDialog({ drivers, projectId, onConnected, onClose }: Props) {
  const [form, setForm] = useState({
    driver: drivers[0]?.id ?? "mysql",
    host: "127.0.0.1",
    port: "3306",
    user: "root",
    password: "",
    database: "",
    ssl: false,
    verifyCert: true,
  });
  const [busy, setBusy] = useState<null | "test" | "connect">(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  const params = (): ConnectParams => ({
    driver: form.driver,
    host: form.host.trim(),
    port: Number(form.port) || 3306,
    user: form.user.trim(),
    password: form.password,
    database: form.database.trim() || null,
    ssl: form.ssl ? { enabled: true, verify_cert: form.verifyCert } : null,
  });

  const set = (key: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) =>
    setForm((f) => ({ ...f, [key]: e.target.value }));

  const setBool =
    (key: "ssl" | "verifyCert") => (e: React.ChangeEvent<HTMLInputElement>) =>
      setForm((f) => ({ ...f, [key]: e.target.checked }));

  async function handleTest() {
    setBusy("test");
    setMessage(null);
    try {
      const version = await api.testConnection(params());
      setMessage({ ok: true, text: `连接成功，服务器版本 ${version}` });
    } catch (e) {
      setMessage({ ok: false, text: String(e) });
    } finally {
      setBusy(null);
    }
  }

  async function handleConnect() {
    setBusy("connect");
    setMessage(null);
    try {
      await api.connect(params(), projectId);
      onConnected();
    } catch (e) {
      setMessage({ ok: false, text: String(e) });
      setBusy(null);
    }
  }

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h2>新建连接</h2>
          <button className="icon-btn" onClick={onClose} aria-label="关闭">
            ✕
          </button>
        </div>

        <div className="modal-body">
          <div className="form-grid">
            <label>
              <span>驱动</span>
              <select value={form.driver} onChange={set("driver")}>
                {drivers.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.display_name}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>主机</span>
              <input value={form.host} onChange={set("host")} placeholder="127.0.0.1" />
            </label>
            <label>
              <span>端口</span>
              <input value={form.port} onChange={set("port")} placeholder="3306" />
            </label>
            <label>
              <span>用户名</span>
              <input value={form.user} onChange={set("user")} placeholder="root" />
            </label>
            <label>
              <span>密码</span>
              <input type="password" value={form.password} onChange={set("password")} />
            </label>
            <label>
              <span>数据库（可选）</span>
              <input value={form.database} onChange={set("database")} placeholder="默认数据库" />
            </label>
            <div className="span-2 checkbox-row">
              <label className="checkbox">
                <input type="checkbox" checked={form.ssl} onChange={setBool("ssl")} />
                <span>使用 SSL/TLS</span>
              </label>
              {form.ssl && (
                <label className="checkbox">
                  <input type="checkbox" checked={form.verifyCert} onChange={setBool("verifyCert")} />
                  <span>校验证书</span>
                </label>
              )}
            </div>
          </div>

          {message && (
            <div className={`form-message ${message.ok ? "ok" : "err"}`}>{message.text}</div>
          )}
        </div>

        <div className="modal-footer">
          <button className="btn" onClick={handleTest} disabled={busy !== null}>
            {busy === "test" ? "测试中…" : "测试连接"}
          </button>
          <button className="btn primary" onClick={handleConnect} disabled={busy !== null}>
            {busy === "connect" ? "连接中…" : "连接"}
          </button>
        </div>
      </div>
    </div>
  );
}
