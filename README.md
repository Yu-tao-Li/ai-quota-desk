# AI Quota Desk

桌面悬浮小组件（Tauri 2 + Rust + TypeScript）：实时显示三家 AI 编程套餐的剩余额度。

## 支持的套餐

| 套餐 | 数据来源 | 认证方式 |
|---|---|---|
| 智谱 GLM Coding Plan | `GET {base}/api/monitor/usage/quota/limit`（与官方 glm-plan-usage 插件一致） | API Key（自动读取 ZCode / Claude 配置，或手动填） |
| Kimi for Coding | `GET https://api.kimi.com/coding/v1/usages` | API Key（手动填） |
| ChatGPT (Codex) | `GET https://chatgpt.com/backend-api/wham/usage` | 自动读取 `~/.codex/auth.json`，401 时自动刷新 token 并回写 |

## 功能

- 无边框置顶悬浮窗，可拖动，关闭即隐藏到系统托盘（托盘左键显隐，右键菜单）
- 每家显示各额度窗口（5小时 / 每周等）的剩余百分比、用量、重置倒计时
- 剩余 <60% 绿 / <25% 黄 / 其余红 的进度条
- 自动刷新（间隔可调），托盘菜单可立即刷新
- 窗口名称按"距重置时长"推断（厂商返回的 unit 枚举不可靠）

## 开发

```bash
npm install
npm run tauri dev    # 开发运行
npm run tauri build  # 打包 nsis 安装包
```

配置文件：`%APPDATA%\ai-quota-desk\config.json`

## 参考

- 官方插件 [zai-org/zai-coding-plugins](https://github.com/zai-org/zai-coding-plugins)（GLM 接口）
- [VicBilibily/GCMP](https://github.com/VicBilibily/GCMP)（Kimi 接口与响应解析）
- [Mai0313/VibeCodingTracker](https://github.com/Mai0313/VibeCodingTracker) 的 `src/core/src/quota/wham.rs`（wham/usage 与 token 刷新）
- [steipete/CodexBar](https://github.com/steipete/CodexBar) 的 `docs/codex.md`（字段映射）
- [borawong/AiMaMi](https://github.com/borawong/AiMaMi)（Tauri 桌面伴侣，本项目图标来源）
