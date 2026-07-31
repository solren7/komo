# 会话历史折叠工具活动，细节外置为 Turn Trace 文件

会话历史里不存真实的 tool call / tool result：turn 内部模型看到完整的工具往返，turn 结束落库时折叠成 assistant 消息上的 Tool Note（摘要索引），完整轨迹写成 Turn Trace 文件（复用 tool-output 存储及其 7 天保留），后续 turn 用 read/grep 按需回捞。

这与同类 harness（jcode / pi / grok-build / codex / opencode）全部相反——它们都在历史里保留真实工具消息，然后各花数百行做悬空 tool call 修复和工具输出修剪。komo 选折叠的理由：(1) 悬空 tool call 这一整类 bug 结构上不存在；(2) 历史按聊天速率而非工具调用速率增长，因此**不需要对话压缩**——撞上下文上限时缩窗重试即可，被裁剪处插标记告知模型；(3) 编码类任务的环境状态（文件内容、测试结果）本身可再生，跨 turn 真正不可再生的细节由 Turn Trace 补齐。

代价是模型跨 turn 看不到上一 turn 的工具结果原文，必须重读或回捞。已评估并否决的替代方案：按会话类型分叉历史模式（下游每个决策都要分叉两份，复杂度接近翻倍）；加厚 Tool Note 为决策摘要（预先猜模型需要什么，不如惰性回捞）。若"跨 turn 迭代式编码"成为高频场景且回捞体验不足，重新评估。
