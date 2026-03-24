use crate::types::LlmError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SseFrame {
    Data(String),
    Done,
}

#[derive(Default)]
pub(crate) struct SseFramer {
    buffer: Vec<u8>,
    event_data: Vec<String>,
    done: bool,
}

impl SseFramer {
    pub(crate) fn push_bytes(&mut self, bytes: &[u8]) -> Result<Vec<SseFrame>, LlmError> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();

        while let Some(newline_index) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line_bytes = self.buffer.drain(..=newline_index).collect::<Vec<_>>();
            let line_bytes = &line_bytes[..line_bytes.len().saturating_sub(1)];
            let line = std::str::from_utf8(line_bytes).map_err(|error| {
                LlmError::Streaming(format!("stream was not valid UTF-8: {error}"))
            })?;
            self.process_line(line, &mut frames)?;
            if self.done {
                self.buffer.clear();
                break;
            }
        }

        Ok(frames)
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<SseFrame>, LlmError> {
        if self.done {
            return Ok(Vec::new());
        }

        if !self.buffer.is_empty() {
            let remaining = std::mem::take(&mut self.buffer);
            let line = std::str::from_utf8(&remaining).map_err(|error| {
                LlmError::Streaming(format!("stream was not valid UTF-8: {error}"))
            })?;
            let mut frames = Vec::new();
            self.process_line(line, &mut frames)?;
            self.flush_event(&mut frames);
            return Ok(frames);
        }

        let mut frames = Vec::new();
        self.flush_event(&mut frames);
        Ok(frames)
    }

    fn process_line(&mut self, line: &str, frames: &mut Vec<SseFrame>) -> Result<(), LlmError> {
        let line = line.trim_start().trim_end_matches('\r');
        if line.is_empty() {
            self.flush_event(frames);
            return Ok(());
        }

        let Some(data) = line.strip_prefix("data:") else {
            return Ok(());
        };

        let data = data.trim_start();
        if data.is_empty() {
            return Ok(());
        }

        if data == "[DONE]" {
            self.flush_event(frames);
            self.done = true;
            frames.push(SseFrame::Done);
            return Ok(());
        }

        self.event_data.push(data.to_string());
        Ok(())
    }

    fn flush_event(&mut self, frames: &mut Vec<SseFrame>) {
        if self.event_data.is_empty() {
            return;
        }

        let data = self.event_data.join("\n");
        self.event_data.clear();
        frames.push(SseFrame::Data(data));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_framer_parses_data_and_done() {
        let mut framer = SseFramer::default();
        let frames = framer.push_bytes(b"data: hello\n\ndata: [DONE]\n").unwrap();

        assert_eq!(
            frames,
            vec![SseFrame::Data("hello".to_string()), SseFrame::Done]
        );
    }

    #[test]
    fn sse_framer_joins_multiline_events() {
        let mut framer = SseFramer::default();
        let frames = framer
            .push_bytes(b"data: {\"a\":1}\ndata: {\"b\":2}\n\n")
            .unwrap();

        assert_eq!(
            frames,
            vec![SseFrame::Data("{\"a\":1}\n{\"b\":2}".to_string())]
        );
    }

    #[test]
    fn sse_framer_handles_fragmented_input() {
        let mut framer = SseFramer::default();
        assert!(framer.push_bytes(b"data: hel").unwrap().is_empty());
        let frames = framer.push_bytes(b"lo\n\n").unwrap();
        assert_eq!(frames, vec![SseFrame::Data("hello".to_string())]);
    }

    #[test]
    fn sse_framer_flushes_on_finish() {
        let mut framer = SseFramer::default();
        assert!(framer.push_bytes(b"data: tail").unwrap().is_empty());
        let frames = framer.finish().unwrap();
        assert_eq!(frames, vec![SseFrame::Data("tail".to_string())]);
    }
}
