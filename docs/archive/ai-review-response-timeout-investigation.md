# AI Review Response Timeout Investigation / AI Review 响应超时排查记录

> Document status: historical investigation evidence, not the current behavior
> specification. Use [AI Code Review](../specs/ai-code-review.md) for the active
> timeout contract and regression requirements.
>
> 文档状态：历史排障证据，不是当前行为规格。现行超时契约与回归要求以
> [AI Code Review](../specs/ai-code-review.md) 为准。

Date / 日期：2026-08-26

## Symptom / 现象

AI Review showed `AI code review timed out. Please try again.` after about 92
seconds. A desktop network graph showed no obvious upload or download activity
during most of the wait.

AI Review 在约 92 秒后显示 `AI code review timed out. Please try again.`。等待期间，
桌面网络曲线大部分时间看不到明显的上传或下载流量。

## Evidence / 证据

The affected request used `deepseek-v4-flash`, two non-streaming completion
messages, 8,858 bytes of message content, and a 9,463-byte serialized JSON body.
The provider returned HTTP/2 `200 application/json` in 165-251 ms. The small
request upload was therefore easy to miss in a coarse network graph; the open
connection then waited for a non-streaming response.

受影响请求使用 `deepseek-v4-flash`，包含两条非流式 completion 消息、8,858 字节消息
内容和 9,463 字节序列化 JSON body。Provider 在 165-251 ms 内返回 HTTP/2
`200 application/json`。这类小请求上传很容易被粗粒度网络曲线忽略；连接随后等待
非流式响应完成。

| Task | Idle limit | Whitespace-only heartbeats | First meaningful body | Result |
| --- | ---: | --- | ---: | --- |
| `review-1a03d790472-1` | 90 s | 59.967 s | none before deadline | Failed at 90.001 s body idle; 92.124 s total |
| `review-1a03d8e15db-1` | 90 s | 59.923 s | none before deadline | Failed at 90.001 s body idle; 91.212 s total |
| `review-1a03dc53ecd-1` | 300 s | 59.998 s, 119.999 s, 180.273 s | 188.576 s | Completed in 189.619 s |

The successful run received 47 body chunks and 93,919 transport bytes after
meaningful output began. The decoded review content was 1,998 bytes. Logs did
not capture request, source, rule, or response content, so the remaining JSON
fields were intentionally not inspected.

成功任务在有效输出开始后收到 47 个 body chunk，共 93,919 个传输字节；解码后的 Review
正文为 1,998 字节。日志没有记录请求、源码、规则或响应正文，因此其余 JSON 字段也没有
被检查。

## Conclusion and resolution / 结论与处理

The 90-second client idle limit was shorter than the observed 188.576-second
non-streaming inference. The provider was not proven to be offline or stuck: it
accepted the request, emitted approximately 60-second whitespace heartbeats,
and eventually returned a valid completion. At the time, whitespace was not
treated as semantic JSON progress. The current streaming implementation instead
resets the transport idle timer for every received chunk and applies a separate
600-second wall-clock limit to each review chunk.

客户端原 90 秒正文空闲阈值短于本次实测的 188.576 秒非流式推理。没有证据表明 Provider
离线或永久卡住：它接受了请求，约每 60 秒发送一次空白心跳，最后返回了有效 completion。
当时空白不属于有效 JSON 进度。当前 streaming 实现已改为任何收到的传输 chunk 都重置
空闲计时，并对每个 Review chunk 另设 600 秒墙钟上限。

`REVIEW_READ_IDLE_TIMEOUT` in `src/ai/agent.rs` was raised to 300 seconds for
AI Review and review-scoped Ask only. The shared 10-second connect timeout,
ordinary AI completion deadlines and explicit Stop cancellation remained
unchanged in that fix. The later SSE migration superseded the body-progress
policy as described above.

`src/ai/agent.rs` 中的 `REVIEW_READ_IDLE_TIMEOUT` 已针对 AI Review 和 Review 专属 Ask
提高到 300 秒。该次修复保持共享的 10 秒连接超时、普通 AI completion 截止时间和显式
Stop 取消能力不变；后续 SSE 迁移已按上文所述替换正文进度策略。

## Future threshold adjustment / 后续阈值调整

When the same symptom recurs, do not increase the shared HTTP defaults first.
Use the request-scoped diagnostics to distinguish connection, status, decode,
and response-body idle failures:

同类问题再次出现时，不要先提高共享 HTTP 默认值。应使用请求级诊断区分连接、状态码、
解码和响应体空闲失败：

1. Correlate one attempt by `task=...` across `[AI Review][Task]`,
   `[AI Agent][Completion]`, `[HTTP][Transfer]`, and `[HTTP][Body]` records.
2. Confirm `request_body_bytes`, response status, response-header latency,
   heartbeat intervals, `whitespace_only`, `counts_as_progress`, first meaningful
   chunk time, and terminal task status.
3. Collect multiple representative runs for the configured model and normal PR
   sizes. A single provider outage or one extreme prompt is not enough evidence
   for a permanent threshold increase.
4. If valid completions repeatedly arrive near or after 300 seconds, adjust only
   `REVIEW_READ_IDLE_TIMEOUT`, update both languages in the AI Review spec, and
   update its focused unit test. Retain cancellation and the separate per-chunk
   wall-clock limit.
5. Choose a bounded threshold above the observed slow-completion range with an
   explicit margin. A larger value reduces false timeouts but also delays real
   stall detection. If latency keeps growing, prefer investigating streaming or
   provider/model behavior instead of extending the deadline indefinitely.
6. Keep credentials, authorization headers, complete prompts, source/rule
   payloads, and response content out of logs and issue reports.

Useful log filter / 推荐日志筛选：

```sh
rg -n '\[AI Review\]\[Task\]|\[AI Agent\]\[Completion\]|\[HTTP\]\[Transfer\]|\[HTTP\]\[Body\].*operation=AI code review' logs/<date>/usher.log
```
