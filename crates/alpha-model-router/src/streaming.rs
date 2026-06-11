//! Newline-delimited JSON stream parser for Ollama chat responses.
//!
//! Ollama streams chat responses as newline-delimited JSON (NDJSON).
//! Each line is a complete JSON object representing a `ChatStreamChunk`.
//!
//! This module handles:
//! - Buffering partial data across TCP frames
//! - Splitting on newline boundaries
//! - Parsing each JSON line into `ChatStreamChunk`
//! - Gracefully handling malformed JSON lines

use bytes::{Bytes, BytesMut};
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use tracing::{debug, warn};

use alpha_common::AlphaError;

use crate::types::ChatStreamChunk;

/// Parse a `reqwest` byte stream into a stream of `ChatStreamChunk`.
///
/// Takes ownership of the byte stream from the HTTP response and yields
/// parsed chunks. Handles:
/// - Partial JSON split across TCP frames
/// - Multiple JSON objects in a single frame
/// - Empty lines (skipped)
/// - Malformed JSON (yields error)
pub fn parse_chat_stream(
    byte_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
) -> Pin<Box<dyn Stream<Item = Result<ChatStreamChunk, AlphaError>> + Send>> {
    Box::pin(NdjsonStream::new(byte_stream))
}

/// Internal stream adapter that buffers bytes and emits parsed JSON chunks.
struct NdjsonStream<S> {
    inner: Pin<Box<S>>,
    buffer: BytesMut,
    /// Whether the underlying byte stream has finished.
    finished: bool,
}

impl<S> NdjsonStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    fn new(stream: S) -> Self {
        Self {
            inner: Box::pin(stream),
            buffer: BytesMut::with_capacity(4096),
            finished: false,
        }
    }

    /// Try to extract and parse one complete JSON line from the buffer.
    ///
    /// Returns:
    /// - `Some(Ok(chunk))` if a complete line was parsed
    /// - `Some(Err(e))` if a line was found but JSON parsing failed
    /// - `None` if no complete line is available yet
    fn try_parse_line(&mut self) -> Option<Result<ChatStreamChunk, AlphaError>> {
        // Find the next newline in the buffer.
        let newline_pos = self.buffer.iter().position(|&b| b == b'\n')?;

        // Split the buffer at the newline.
        let line_bytes = self.buffer.split_to(newline_pos + 1);
        let line = std::str::from_utf8(&line_bytes)
            .unwrap_or("")
            .trim();

        // Skip empty lines.
        if line.is_empty() {
            return None;
        }

        // Parse the JSON line.
        match serde_json::from_str::<ChatStreamChunk>(line) {
            Ok(chunk) => {
                debug!(
                    model = %chunk.model,
                    done = chunk.done,
                    content_len = chunk.message.content.len(),
                    "Stream chunk parsed"
                );
                Some(Ok(chunk))
            }
            Err(e) => {
                warn!(
                    error = %e,
                    line = %line,
                    "Failed to parse stream chunk"
                );
                Some(Err(AlphaError::Other(format!(
                    "Malformed stream chunk: {e}"
                ))))
            }
        }
    }
}

impl<S> Stream for NdjsonStream<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    type Item = Result<ChatStreamChunk, AlphaError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            // First, try to yield a chunk from buffered data.
            if let Some(result) = this.try_parse_line() {
                return Poll::Ready(Some(result));
            }

            // If the inner stream is done and buffer has no more lines, we're done.
            if this.finished {
                // Check for any remaining non-newline-terminated data.
                if !this.buffer.is_empty() {
                    let remaining = std::str::from_utf8(&this.buffer)
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    this.buffer.clear();

                    if remaining.is_empty() {
                        return Poll::Ready(None);
                    }

                    // Try to parse the remaining data as a final chunk.
                    match serde_json::from_str::<ChatStreamChunk>(&remaining) {
                        Ok(chunk) => return Poll::Ready(Some(Ok(chunk))),
                        Err(e) => {
                            warn!(error = %e, "Trailing data could not be parsed");
                            return Poll::Ready(Some(Err(AlphaError::Other(
                                format!("Malformed trailing chunk: {e}"),
                            ))));
                        }
                    }
                }
                return Poll::Ready(None);
            }

            // Poll the inner byte stream for more data.
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    this.buffer.extend_from_slice(&bytes);
                    // Loop back to try_parse_line with the new data.
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(AlphaError::Other(format!(
                        "Stream read error: {e}"
                    )))));
                }
                Poll::Ready(None) => {
                    this.finished = true;
                    // Loop back to drain any remaining buffer.
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use tokio_stream::iter as stream_iter;

    /// Helper: create a byte stream from string slices.
    fn mock_byte_stream(
        chunks: Vec<&str>,
    ) -> impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static {
        stream_iter(
            chunks
                .into_iter()
                .map(|s| Ok(Bytes::from(s.to_string())))
                .collect::<Vec<_>>(),
        )
    }

    #[tokio::test]
    async fn test_single_chunk_stream() {
        let data = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"Hello"},"done":true,"total_duration":5000000000,"prompt_eval_count":10,"eval_count":1}"#;
        let input = format!("{data}\n");

        let stream = parse_chat_stream(mock_byte_stream(vec![&input]));
        let chunks: Vec<_> = stream.collect().await;

        assert_eq!(chunks.len(), 1);
        let chunk = chunks[0].as_ref().unwrap();
        assert_eq!(chunk.model, "llama3.1:8b");
        assert_eq!(chunk.message.content, "Hello");
        assert!(chunk.done);
        assert_eq!(chunk.total_duration, Some(5_000_000_000));
        assert_eq!(chunk.eval_count, Some(1));
    }

    #[tokio::test]
    async fn test_multi_chunk_stream() {
        let chunk1 = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"The"},"done":false}"#;
        let chunk2 = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":" answer"},"done":false}"#;
        let chunk3 = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":" is 42."},"done":false}"#;
        let final_chunk = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":""},"done":true,"total_duration":3000000000,"eval_count":3}"#;

        let input = format!("{chunk1}\n{chunk2}\n{chunk3}\n{final_chunk}\n");
        let stream = parse_chat_stream(mock_byte_stream(vec![&input]));
        let chunks: Vec<_> = stream.collect().await;

        assert_eq!(chunks.len(), 4);

        // Verify content of each chunk.
        assert_eq!(chunks[0].as_ref().unwrap().message.content, "The");
        assert!(!chunks[0].as_ref().unwrap().done);

        assert_eq!(chunks[1].as_ref().unwrap().message.content, " answer");
        assert!(!chunks[1].as_ref().unwrap().done);

        assert_eq!(chunks[2].as_ref().unwrap().message.content, " is 42.");
        assert!(!chunks[2].as_ref().unwrap().done);

        assert_eq!(chunks[3].as_ref().unwrap().message.content, "");
        assert!(chunks[3].as_ref().unwrap().done);
        assert_eq!(chunks[3].as_ref().unwrap().total_duration, Some(3_000_000_000));
    }

    #[tokio::test]
    async fn test_split_json_across_buffers() {
        // Simulate a JSON object split across two TCP frames.
        let full_line = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"split"},"done":true}"#;
        let (part1, part2) = full_line.split_at(30);
        let part2_with_newline = format!("{part2}\n");

        let stream = parse_chat_stream(mock_byte_stream(vec![part1, &part2_with_newline]));
        let chunks: Vec<_> = stream.collect().await;

        assert_eq!(chunks.len(), 1);
        let chunk = chunks[0].as_ref().unwrap();
        assert_eq!(chunk.message.content, "split");
        assert!(chunk.done);
    }

    #[tokio::test]
    async fn test_multiple_json_in_single_buffer() {
        // Two complete JSON objects arrive in one TCP frame.
        let line1 = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"A"},"done":false}"#;
        let line2 = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"B"},"done":true}"#;
        let input = format!("{line1}\n{line2}\n");

        let stream = parse_chat_stream(mock_byte_stream(vec![&input]));
        let chunks: Vec<_> = stream.collect().await;

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].as_ref().unwrap().message.content, "A");
        assert_eq!(chunks[1].as_ref().unwrap().message.content, "B");
    }

    #[tokio::test]
    async fn test_malformed_json() {
        let good = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"ok"},"done":false}"#;
        let bad = r#"{"broken json"#;
        let input = format!("{good}\n{bad}\n");

        let stream = parse_chat_stream(mock_byte_stream(vec![&input]));
        let chunks: Vec<_> = stream.collect().await;

        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].is_ok());
        assert!(chunks[1].is_err());
        assert!(
            chunks[1].as_ref().unwrap_err().to_string().contains("Malformed"),
            "error message: {}",
            chunks[1].as_ref().unwrap_err()
        );
    }

    #[tokio::test]
    async fn test_final_chunk_detection() {
        let non_final = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"token"},"done":false}"#;
        let final_chunk = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":""},"done":true,"total_duration":1234567890,"prompt_eval_count":5,"eval_count":10}"#;
        let input = format!("{non_final}\n{final_chunk}\n");

        let stream = parse_chat_stream(mock_byte_stream(vec![&input]));
        let chunks: Vec<_> = stream.collect().await;

        // Verify we can detect the final chunk.
        let final_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| c.as_ref().map(|c| c.done).unwrap_or(false))
            .collect();
        assert_eq!(final_chunks.len(), 1);

        let fc = final_chunks[0].as_ref().unwrap();
        assert!(fc.done);
        assert_eq!(fc.total_duration, Some(1_234_567_890));
        assert_eq!(fc.prompt_eval_count, Some(5));
        assert_eq!(fc.eval_count, Some(10));
    }

    #[tokio::test]
    async fn test_parser_empty_stream() {
        let stream = parse_chat_stream(mock_byte_stream(vec![]));
        let chunks: Vec<_> = stream.collect().await;
        assert!(chunks.is_empty());
    }

    #[tokio::test]
    async fn test_empty_lines_skipped() {
        let line = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"hi"},"done":true}"#;
        let input = format!("\n\n{line}\n\n");

        let stream = parse_chat_stream(mock_byte_stream(vec![&input]));
        let chunks: Vec<_> = stream.collect().await;

        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_ok());
    }

    #[tokio::test]
    async fn test_trailing_data_without_newline() {
        // Data that doesn't end with a newline (stream terminated early).
        let line = r#"{"model":"llama3.1:8b","message":{"role":"assistant","content":"end"},"done":true}"#;

        let stream = parse_chat_stream(mock_byte_stream(vec![line]));
        let chunks: Vec<_> = stream.collect().await;

        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_ok());
        assert_eq!(chunks[0].as_ref().unwrap().message.content, "end");
    }
}
