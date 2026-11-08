#!/usr/bin/env python3
"""Mock OpenAI-compatible LLM server for E2E testing (no real API keys needed).

Endpoints:
  POST /v1/chat/completions — scripted streaming responses:
      user asks about time     -> tool_call current_time
      user asks to write file  -> tool_call fs_write (triggers approval flow)
      user asks to read file   -> tool_call fs_read
      user says echo           -> tool_call mock_echo (DB-configured http tool)
      last message is a tool result -> plain text confirming
      otherwise                -> plain text reply
  GET  /echo?text=...       -> {"echo": ...}   (target of the http tool test)
  GET  /health              -> ok

Run: python3 scripts/mock-llm.py            # port 11435
"""
import json
import os
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

PORT = int(os.environ.get("MOCK_LLM_PORT", "11435"))


def sse_chunk(delta=None, finish=None, usage=None):
    payload = {"id": "chatcmpl-mock", "object": "chat.completion.chunk", "created": int(time.time()), "model": "mock-model", "choices": [{"index": 0, "delta": delta or {}, "finish_reason": finish}]}
    if usage:
        payload["usage"] = usage
    return f"data: {json.dumps(payload)}\n\n"


def tool_call_chunks(call_id, name, arguments: dict):
    args = json.dumps(arguments)
    chunks = [sse_chunk({"role": "assistant", "tool_calls": [{"index": 0, "id": call_id, "type": "function", "function": {"name": name, "arguments": ""}}]})]
    for i in range(0, len(args), 8):
        chunks.append(sse_chunk({"tool_calls": [{"index": 0, "function": {"arguments": args[i : i + 8]}}]}))
    chunks.append(sse_chunk({}, finish="tool_calls"))
    return chunks


def text_chunks(text):
    out = []
    for word in text.split(" "):
        out.append(sse_chunk({"content": word + " "}))
    out.append(sse_chunk({}, finish="stop", usage={"prompt_tokens": 12, "completion_tokens": len(text.split(" "))}))
    return out


def reasoning_chunks(lines):
    """DeepSeek-style reasoning deltas (delta.reasoning_content), one line per chunk."""
    out = []
    for line in lines:
        out.append(sse_chunk({"reasoning_content": line + "\n"}))
    return out


REASONING_LINES = [
    "Step 1: parse what the user is asking for.",
    "Step 2: consider which tool or answer fits best.",
    "Step 3: check constraints like sandbox paths and approvals.",
    "Step 4: draft the reply and keep it concise.",
    "Step 5: sanity-check the final answer once more.",
    "Step 6: done thinking, ready to answer.",
]

MARKDOWN_DEMO = """## Markdown render demo

Here is a **table** with some *emphasis*, `inline code` and a [link](https://example.com):

| Feature | Status | Notes |
|---|---|---|
| Tables | ✅ | scrollable wrapper |
| Math | ✅ | KaTeX: $E = mc^2$ and $$\\int_0^1 x^2 dx = \\frac{1}{3}$$ |
| Highlight | ✅ | shiki dual theme |

> Blockquote: rendering should look consistent in **light and dark** mode.

- [x] typography
- [x] code highlight + copy
- [ ] you reviewing it

And an ordered list:

1. First item with some text
2. Second item that is long enough to wrap across the width of the message area to test marker alignment and clipping behaviour
3. Third item with `inline code`
4. Fourth

Rust example:

```rust
/// Computes the nth Fibonacci number.
fn fib(n: u64) -> u64 {
    match n {
        0 | 1 => n,
        _ => fib(n - 1) + fib(n - 2),
    }
}
```

A sequence diagram:

```mermaid
sequenceDiagram
    participant U as User
    participant A as Assistant
    U->>A: markdown demo
    A-->>U: rendered beautifully
"""



class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):  # quieter logs
        pass

    def _send_sse(self, chunks, delay=0.03):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        for c in chunks:
            self.wfile.write(c.encode())
            self.wfile.flush()
            time.sleep(delay)
        self.wfile.write(b"data: [DONE]\n\n")

    def do_GET(self):
        if self.path.startswith("/echo"):
            from urllib.parse import urlparse, parse_qs, unquote

            q = parse_qs(urlparse(self.path).query)
            body = json.dumps({"echo": unquote(q.get("text", [""])[0]), "source": "mock-llm"})
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(body.encode())
            return
        if self.path == "/health":
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b"ok")
            return
        self.send_response(404)
        self.end_headers()

    def do_POST(self):
        if not self.path.startswith("/v1/chat/completions"):
            self.send_response(404)
            self.end_headers()
            return
        length = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(length) or b"{}")
        messages = body.get("messages", [])

        last = messages[-1] if messages else {}
        last_user = next((m for m in reversed(messages) if m.get("role") == "user"), {})
        user_text = ""
        for part in (last_user.get("content") if isinstance(last_user.get("content"), list) else [{"type": "text", "text": str(last_user.get("content", ""))}]):
            if part.get("type") == "text":
                user_text += part.get("text", "")
        user_text = user_text.lower()

        # Echo the reasoning effort the backend sent us — visible in the reply.
        effort_note = f" (reasoning_effort={body.get('reasoning_effort', 'unset')})"

        if last.get("role") == "tool":
            content = str(last.get("content", ""))[:120]
            self._send_sse(text_chunks(f"Done. The tool result was: {content}"))
            return

        # Multimodal probe: report what content blocks actually arrived.
        last_user_content = last_user.get("content")
        if isinstance(last_user_content, list):
            kinds = [p.get("type") for p in last_user_content]
            image_urls = [p.get("image_url", {}).get("url", "") for p in last_user_content if p.get("type") == "image_url"]
            summary = ", ".join(kinds)
            detail = ""
            if image_urls:
                first = image_urls[0]
                detail = f" first image starts with: {first[:32]}…"
            self._send_sse(text_chunks(f"Your message arrived as content blocks: [{summary}].{detail}"))
            return

        # Text-inline probe: attachments inlined into the plain string content.
        if isinstance(last_user_content, str) and "--- Attached file:" in last_user_content:
            self._send_sse(text_chunks("I can see the attached file content inline in your message."))
            return

        if "markdown demo" in user_text:
            self._send_sse(text_chunks(MARKDOWN_DEMO))
            return

        call_id = "call_" + str(abs(hash(user_text)) % 10**8)
        if "think" in user_text:
            self._send_sse(reasoning_chunks(REASONING_LINES), delay=0.35)
            self._send_sse(text_chunks("That is what I concluded after thinking." + effort_note))
        elif "time" in user_text:
            self._send_sse(tool_call_chunks(call_id, "current_time", {}))
        elif "write" in user_text:
            self._send_sse(tool_call_chunks(call_id, "fs_write", {"path": "/tmp/demo-project/hello.txt", "content": "Hello from the assistant!\n"}))
        elif "read" in user_text:
            self._send_sse(tool_call_chunks(call_id, "fs_read", {"path": "/tmp/demo-project/hello.txt"}))
        elif "save artifact file" in user_text:
            self._send_sse(tool_call_chunks(call_id, "save_artifact", {"path": "reports/summary.txt"}))
        elif "create artifact" in user_text:
            self._send_sse(tool_call_chunks(call_id, "save_artifact", {"filename": "report.md", "content": "# Mock report\n\nGenerated by the assistant."}))
        elif "secure" in user_text:
            self._send_sse(tool_call_chunks(call_id, "mock.secure", {}))
        elif "echo" in user_text:
            self._send_sse(tool_call_chunks(call_id, "mock_echo", {"text": "hello from e2e"}))
        else:
            self._send_sse(text_chunks("Hi! I am the mock assistant. Ask me about the weather, the time, or writing a file." + effort_note))


if __name__ == "__main__":
    print(f"mock LLM listening on http://localhost:{PORT}")
    HTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
