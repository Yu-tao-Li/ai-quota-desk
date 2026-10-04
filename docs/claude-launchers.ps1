# Claude 账号启动函数（配合 AI Quota Desk 的 Claude Reset Calendar）
# 用法：放进 $PROFILE，之后终端里输入 claude1 / claude2 启动对应账号，
#       悬浮窗会自动把该账号的日历标记为“当前”。
function claude1 {
    "claude1" | Out-File "$env:APPDATA\ai-quota-desk\active-claude.txt" -Encoding utf8
    claude
}
function claude2 {
    "claude2" | Out-File "$env:APPDATA\ai-quota-desk\active-claude.txt" -Encoding utf8
    claude
}
