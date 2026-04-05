
---

## Deployment Architecture

```text
+--------------------------------------------------------------+
|                        Fawx Harness                          |
|                                                              |
|  +------------------+   +-------------------------------+    |
|  | Reasoning / LLM  |   | Planner / Controller          |    |
|  | goal decomposition|  | actions, retries, recovery    |    |
|  +---------+--------+   +---------------+---------------+    |
|            |                            |                    |
|            | query/watch                | action context     |
|            v                            v                    |
|      +--------------------------------------------------+    |
|      | Safety / Permissions / Commit Boundaries         |    |
|      +----------------------+---------------------------+    |
+-----------------------------|--------------------------------+
                              | observer ABI
                              | publish/query/watch/probe
+-----------------------------|--------------------------------+
|                       fawx-eyes sidecar                      |
|                                                              |
|  spatial encoder -> temporal world model -> heads            |
|                      ^                       |               |
|                      |                       v               |
|                recent action          belief state            |
|                                       uncertainty            |
|                                       novelty                |
|                                       affordances            |
|                                       probe requests         |
|                                       probe budget remaining |
+--------------------------------------------------------------+
```

### Transport

JSON over local IPC is a good debug surface and early integration surface.
It may not be the final high-rate transport.

A likely path:

- **JSON / sockets** for debugging, evaluation, and early versions
- **shared memory / binary transport** for the hot path if frame rate or payload size demands it

### Hardware posture

This should be local-first:

- laptop / desktop GPU for development and early product work
- Jetson-class edge hardware for dedicated local assistants and robots
- larger local boxes for more capable multimodal assistants
- optional remote reasoning for non-real-time tasks only

The observer and planner must remain useful even when the network is absent.
