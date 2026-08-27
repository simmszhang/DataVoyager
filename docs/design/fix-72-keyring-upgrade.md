# #72 Windows Keyring Bug 修复方案

## 问题总结

**症状**：Windows 用户双击重连已保存连接时报错：
```
database error: Server error: `ERROR 1045 (28000): Access denied for user 'root'@'localhost' (using password: NO)'
```

**根本原因**：`keyring 3.6.3` 在 Windows 上存在严重 bug：
- ✅ 用**同一个** `Entry` 实例写入后立即读取 → 成功
- ❌ 用**新的** `Entry` 实例读取同一个 key → 返回 `NoEntry`

**影响路径**：
1. 用户 `connect` → `store_secrets` 创建 `Entry` 写入密码 → 成功
2. 用户断开连接，后续 `reconnect` → 创建**新** `Entry` 读取密码 → 失败（`NoEntry`）
3. 密码变成 `None` → MySQL 认证失败

**测试证据**：
```bash
cargo test -p dby --test keyring_test test_keyring_roundtrip -- --ignored --nocapture
```
输出：
```
✓ Password retrieved with same instance: test_password_123
✗ NoEntry error with new instance - THIS IS THE BUG!
```

## 解决方案

### 方案 A：升级 keyring 到 v4（推荐）

`keyring` 已发布 v4.1.6，可能修复了此 bug。

**步骤**：
1. 修改 `src-tauri/Cargo.toml`：
   ```toml
   keyring = "4"
   ```

2. 更新依赖：
   ```bash
   cargo update -p keyring
   ```

3. 测试：
   ```bash
   cargo test -p dby --test keyring_test test_keyring_roundtrip -- --ignored --nocapture
   ```

4. 如果测试通过，验证实际应用：
   ```bash
   pnpm tauri dev
   ```
   - 连接一个 MySQL 数据库并保存
   - 断开连接
   - 双击重连 → 应该成功

**API 兼容性检查**：
keyring v3 → v4 可能有 breaking changes，需要检查：
- `Entry::new(service, user)` 签名是否变化
- `set_password/get_password/delete_credential` 是否变化
- 错误类型是否变化

### 方案 B：Workaround - 复用 Entry 实例（不推荐）

如果 v4 仍有问题，可以尝试在 AppState 中缓存 Entry 实例：

```rust
// src-tauri/src/state.rs
pub struct AppState {
    // ... existing fields ...
    
    // Cache Entry instances per config_id (Windows workaround for keyring v3 bug)
    #[cfg(target_os = "windows")]
    keyring_entries: Arc<Mutex<HashMap<String, keyring::Entry>>>,
}
```

**缺点**：
- 增加内存占用
- 复杂化生命周期管理
- 不是真正的修复

### 方案 C：替换 keyring 库

如果 keyring v4 仍不能解决问题，考虑：
- Windows：直接调用 Win32 Credential Manager API
- 跨平台：使用 `secret-service`（Linux）+ `security-framework`（macOS）+ Win32 API

**缺点**：
- 大幅增加工作量
- 失去跨平台统一抽象

## 执行步骤（方案 A）

1. **确保没有 cargo 进程持有锁**：
   ```powershell
   Get-Process | Where-Object {$_.ProcessName -like "*cargo*"} | Stop-Process -Force
   ```

2. **更新 Cargo.toml**（已完成）

3. **更新依赖**：
   ```bash
   cargo update -p keyring
   cargo build -p dby
   ```

4. **运行测试**：
   ```bash
   cargo test -p dby --test keyring_test test_keyring_roundtrip -- --ignored --nocapture
   ```

5. **如果测试通过，运行完整 CI**：
   ```bash
   cargo test -p dby-core -p dby-driver-mysql
   pnpm build
   cargo clippy --workspace --all-targets -- -D warnings
   ```

6. **手工验证**：
   - `pnpm tauri dev`
   - 连接 → 保存 → 断开 → 重连

7. **更新文档**：
   - 在 `docs/design/defects.md` 中标记 #72 为已修复
   - 在 `AGENTS.md` 中更新状态

## 回滚计划

如果 keyring v4 引入新问题：

```bash
# 恢复 Cargo.toml
git checkout src-tauri/Cargo.toml

# 清理 Cargo.lock
rm Cargo.lock
cargo build

# 临时禁用重连功能，提示用户手动重新连接
```

## 相关资源

- keyring v3 → v4 changelog: https://github.com/hwchen/keyring-rs/blob/master/CHANGELOG.md
- Windows Credential Manager API: https://docs.microsoft.com/en-us/windows/win32/api/wincred/
- Issue tracker: 如果 v4 仍有问题，向 keyring-rs 提交 bug report

---

**状态**：等待 cargo 锁释放后执行方案 A
