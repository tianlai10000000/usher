## DPI Flicker Repro (macOS)

这个小工程用于复现 **Slint + winit** 在 **macOS 跨不同 DPI/Scale 的屏幕** 拖动窗口时可能出现的“明显闪烁”。

### 复现前提

- 两块屏幕（或笔记本内屏 + 外接屏），**DPI/Scale Factor 不同**
  - 例如 MacBook Retina（2.0）+ 普通外接（1.0）

### 运行

在仓库根目录执行：

```bash
cd dpi-flicker-repro
cargo run
```

可选：开启一个很轻量的动画（用于区分“渲染表面重建闪” vs “内容自身重排”）：

```bash
cd dpi-flicker-repro
DPI_REPRO_ANIMATE=1 cargo run
```

### 观察点

- **如果页面结构很简单仍然出现明显闪烁**，基本可以判定问题主要在底层窗口/渲染表面重建链路，而不是业务布局导致。

