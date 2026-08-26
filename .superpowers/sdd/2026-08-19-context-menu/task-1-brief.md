# Task 1: 模块 1 - 全局右键菜单屏蔽

**Files:**
- Modify: `src/App.tsx:457` (添加 `onContextMenu`)
- Modify: `src/components/QueryEditor.tsx:70` (保留编辑器原生菜单)

**Interfaces:**
- Consumes: 无（独立改动）
- Produces: 全局屏蔽生效，自定义菜单不受影响

- [ ] **Step 1: App.tsx 添加全局屏蔽**

在 `src/App.tsx` 第 457 行最外层 `<div className="app">` 加 `onContextMenu` 处理器：

```tsx
return (
  <div className="app" onContextMenu={(e) => e.preventDefault()}>
    <header className="topbar">
```

- [ ] **Step 2: QueryEditor.tsx 保留编辑器原生菜单**

在 `src/components/QueryEditor.tsx` 第 70 行 `<div className="editor">` 加 `onContextMenu` 阻止冒泡：

```tsx
<div className="editor" onContextMenu={(e) => e.stopPropagation()}>
  <CodeMirror
```

- [ ] **Step 3: 手动测试验证**

启动开发服务器：
```bash
pnpm tauri dev
```

验证：
- 空白区域右键 → 无原生菜单
- Schema 树节点右键 → 弹自定义菜单（现有行为保持）
- CodeMirror 编辑器内右键 → 原生菜单保留（可复制/粘贴）

- [ ] **Step 4: Commit**

```bash
git add src/App.tsx src/components/QueryEditor.tsx
git commit -m "feat(ui): 全局屏蔽浏览器原生右键菜单

- App.tsx: 最外层 div 阻止默认右键行为
- QueryEditor.tsx: 编辑器内保留原生菜单（stopPropagation）
- 为自定义右键菜单统一体验提供基础"
```
