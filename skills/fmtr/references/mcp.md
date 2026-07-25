# MCP tools and resources

All tools are read-only and idempotent.

| Tool | Use |
| --- | --- |
| `list_meetings` | Find recent meetings by title or ID fragment. |
| `get_meeting` | Read metadata, canonical note, summaries, and action items. |
| `get_meeting_transcript` | Read a transcript page. Start with `limit: 200`; continue from `pagination.next_offset` only as needed. |

Transcript limits are measured in words. The default is 200 and the maximum is 500.

Available resources:

- `fmtr://meetings/{meeting_id}`
- `fmtr://meetings/{meeting_id}/transcript{?offset,limit}`

Prefer tools when the workflow needs structured JSON. Use resources when the client needs concise Markdown or plain-text context.
