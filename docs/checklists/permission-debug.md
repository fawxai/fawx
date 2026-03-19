# Permission Approval Debug — Next Session

## The Bug
Permission modal does not appear in the Swift GUI when the server sends a permission_prompt SSE event. The tool blocks waiting for a response, times out after 300s.

## What we know works
- Server sends permission_prompt SSE events (confirmed by tool timeout behavior)
- The Swift permission approval code exists (commit 6bfb868f)
- The permission respond endpoint works (POST /v1/permissions/prompts/{id}/respond)
- The modal DID appear once earlier today (on the Frankenstein branch before reset)

## What needs debugging
1. Is the SSE parser in the Swift app handling `permission_prompt` event type?
2. Is the event name matching? (server sends "permission_prompt" vs Swift expects "permissionPrompt" or vice versa?)
3. Is the SSE stream for the session the same stream that receives permission events?
4. Is there a timing issue — does the prompt fire before the SSE connection is established?

## How to debug
1. Add logging to the Swift SSE parser — print every event type received
2. Check server logs for permission_prompt emission
3. Compare the SSE event format the server sends vs what the Swift parser expects
4. Test with curl: `curl -N http://localhost:8400/v1/sessions/{id}/stream` and trigger a permission prompt from another terminal

## Files involved
- Server: engine/crates/fx-kernel/src/permission_prompt.rs
- Server SSE: engine/crates/fx-api/src/handlers/sessions.rs (stream callback)
- Swift parser: app/Fawx/Networking/SSEStream.swift
- Swift UI: app/Fawx/Views/Shared/PermissionApprovalView.swift (or similar)
