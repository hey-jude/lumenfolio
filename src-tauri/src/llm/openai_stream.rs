use tauri::Emitter;

use super::chat::extract_chat_response_text;
use crate::{
    truncate_for_error, AnswerDeltaEventOutput, OpenAiChatChunk, OpenAiChatResponse,
    ReasoningDeltaEventOutput, StreamedChatText,
};

pub(crate) async fn read_openai_answer_stream(
    mut response: reqwest::Response,
    app: &tauri::AppHandle,
    answer_event_id: Option<&str>,
) -> Result<StreamedChatText, String> {
    let mut buffer = Vec::new();
    let mut answer = String::new();
    let mut reasoning_content = String::new();
    let mut chunk_count = 0usize;
    let mut total_bytes = 0usize;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| format!("Failed to read chat stream: {err}"))?
    {
        chunk_count += 1;
        total_bytes += chunk.len();
        log::debug!(
            "chat stream chunk event_id={} chunk={} bytes={} total_bytes={} buffer_before={}",
            answer_event_id.unwrap_or("-"),
            chunk_count,
            chunk.len(),
            total_bytes,
            buffer.len()
        );
        let answer_chars_before = answer.chars().count();
        buffer.extend_from_slice(&chunk);
        drain_openai_sse_buffer(
            &mut buffer,
            &mut answer,
            &mut reasoning_content,
            app,
            answer_event_id,
        )?;
        let answer_chars_after = answer.chars().count();
        if answer_chars_after > answer_chars_before {
            log::debug!(
                "chat stream parsed delta event_id={} chunk={} new_chars={} total_answer_chars={}",
                answer_event_id.unwrap_or("-"),
                chunk_count,
                answer_chars_after - answer_chars_before,
                answer_chars_after
            );
        }
    }
    drain_openai_sse_buffer(
        &mut buffer,
        &mut answer,
        &mut reasoning_content,
        app,
        answer_event_id,
    )?;
    drain_openai_stream_tail(
        &mut buffer,
        &mut answer,
        &mut reasoning_content,
        app,
        answer_event_id,
    )?;
    log::info!(
        "chat stream read complete event_id={} chunks={} total_bytes={} answer_chars={} reasoning_chars={}",
        answer_event_id.unwrap_or("-"),
        chunk_count,
        total_bytes,
        answer.chars().count(),
        reasoning_content.chars().count()
    );
    Ok(StreamedChatText {
        answer,
        reasoning_content,
    })
}

fn drain_openai_sse_buffer(
    buffer: &mut Vec<u8>,
    answer: &mut String,
    reasoning_content: &mut String,
    app: &tauri::AppHandle,
    answer_event_id: Option<&str>,
) -> Result<(), String> {
    while let Some((index, delimiter_len)) = next_sse_frame_boundary(buffer) {
        let frame_bytes = buffer[..index].to_vec();
        buffer.drain(..index + delimiter_len);
        let frame = std::str::from_utf8(&frame_bytes)
            .map_err(|err| format!("Failed to decode chat stream frame: {err}"))?;
        log::debug!(
            "chat stream frame event_id={} bytes={} preview={}",
            answer_event_id.unwrap_or("-"),
            frame_bytes.len(),
            truncate_for_error(frame, 120)
        );
        handle_openai_sse_frame(frame, answer, reasoning_content, app, answer_event_id)?;
    }
    Ok(())
}

fn next_sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    match (
        find_byte_sequence(buffer, b"\n\n"),
        find_byte_sequence(buffer, b"\r\n\r\n"),
    ) {
        (Some(line_feed), Some(carriage_return)) if carriage_return < line_feed => {
            Some((carriage_return, 4))
        }
        (Some(line_feed), _) => Some((line_feed, 2)),
        (None, Some(carriage_return)) => Some((carriage_return, 4)),
        (None, None) => None,
    }
}

fn find_byte_sequence(buffer: &[u8], needle: &[u8]) -> Option<usize> {
    buffer
        .windows(needle.len())
        .position(|window| window == needle)
}

fn handle_openai_sse_frame(
    frame: &str,
    answer: &mut String,
    reasoning_content: &mut String,
    app: &tauri::AppHandle,
    answer_event_id: Option<&str>,
) -> Result<(), String> {
    let mut emit_delta = |delta| emit_answer_delta(app, answer_event_id, delta);
    let mut emit_reasoning_delta = |delta| emit_reasoning_delta(app, answer_event_id, delta);
    handle_openai_sse_frame_with_sink(
        frame,
        answer,
        reasoning_content,
        &mut emit_delta,
        &mut emit_reasoning_delta,
    )
}

fn handle_openai_sse_frame_with_sink<F, G>(
    frame: &str,
    answer: &mut String,
    reasoning_content: &mut String,
    on_delta: &mut F,
    on_reasoning_delta: &mut G,
) -> Result<(), String>
where
    F: FnMut(String),
    G: FnMut(String),
{
    for line in frame.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let chunk = serde_json::from_str::<OpenAiChatChunk>(data)
            .map_err(|err| format!("Failed to decode chat stream chunk: {err}"))?;
        for choice in chunk.choices {
            if let Some(delta) = choice.delta {
                if let Some(reasoning_delta) = delta
                    .reasoning_content
                    .or(delta.reasoning)
                    .filter(|value| !value.is_empty())
                {
                    reasoning_content.push_str(&reasoning_delta);
                    on_reasoning_delta(reasoning_delta);
                }
                if let Some(answer_delta) = delta.content.filter(|value| !value.is_empty()) {
                    answer.push_str(&answer_delta);
                    on_delta(answer_delta);
                }
            }
        }
    }
    Ok(())
}

fn drain_openai_stream_tail(
    buffer: &mut Vec<u8>,
    answer: &mut String,
    reasoning_content: &mut String,
    app: &tauri::AppHandle,
    answer_event_id: Option<&str>,
) -> Result<(), String> {
    let tail = std::str::from_utf8(buffer)
        .map_err(|err| format!("Failed to decode chat stream tail: {err}"))?
        .trim();
    if tail.is_empty() {
        buffer.clear();
        return Ok(());
    }
    if tail.starts_with("data:") {
        let frame = tail.to_string();
        buffer.clear();
        return handle_openai_sse_frame(&frame, answer, reasoning_content, app, answer_event_id);
    }
    if let Ok(response) = serde_json::from_str::<OpenAiChatResponse>(tail) {
        log::warn!(
            "chat stream fallback non_sse_json event_id={} tail_chars={}",
            answer_event_id.unwrap_or("-"),
            tail.chars().count()
        );
        let text = response
            .choices
            .into_iter()
            .next()
            .map(|choice| extract_chat_response_text(&choice.message.content))
            .unwrap_or_default();
        if !text.is_empty() {
            answer.push_str(&text);
            emit_answer_delta(app, answer_event_id, text);
        }
        buffer.clear();
        return Ok(());
    }
    Err("Chat provider returned an incomplete stream frame".to_string())
}

fn emit_answer_delta(app: &tauri::AppHandle, answer_event_id: Option<&str>, delta: String) {
    if let Some(event_id) = answer_event_id {
        let delta_chars = delta.chars().count();
        let delta_preview = truncate_for_error(&delta, 80);
        if let Err(err) = app.emit(
            "lumenfolio://answer-delta",
            AnswerDeltaEventOutput {
                event_id: event_id.to_string(),
                delta,
            },
        ) {
            log::warn!("Failed to emit answer delta: {err}");
        } else {
            log::debug!(
                "chat stream emitted delta event_id={} chars={} preview={}",
                event_id,
                delta_chars,
                delta_preview
            );
        }
    }
}

fn emit_reasoning_delta(app: &tauri::AppHandle, answer_event_id: Option<&str>, delta: String) {
    if let Some(event_id) = answer_event_id {
        let delta_chars = delta.chars().count();
        if let Err(err) = app.emit(
            "lumenfolio://reasoning-delta",
            ReasoningDeltaEventOutput {
                event_id: event_id.to_string(),
                delta,
            },
        ) {
            log::warn!("Failed to emit reasoning delta: {err}");
        } else {
            log::debug!(
                "chat stream emitted reasoning delta event_id={} chars={}",
                event_id,
                delta_chars
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_boundary_waits_for_complete_utf8_frame() {
        let mut buffer = b"data: {\"choices\":[{\"delta\":{\"content\":\"".to_vec();
        buffer.extend_from_slice("中文".as_bytes());

        assert_eq!(next_sse_frame_boundary(&buffer), None);

        buffer.extend_from_slice(b"\"}}]}\n\n");
        assert_eq!(
            next_sse_frame_boundary(&buffer),
            Some((buffer.len() - 2, 2))
        );
    }

    #[test]
    fn sse_boundary_supports_crlf_frames() {
        let buffer = b"data: {\"choices\":[]}\r\n\r\n";

        assert_eq!(next_sse_frame_boundary(buffer), Some((buffer.len() - 4, 4)));
    }

    #[test]
    fn sse_frame_emits_each_delta_incrementally() {
        let frame = concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"你\"}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"好\"}}]}\n"
        );
        let mut answer = String::new();
        let mut reasoning_content = String::new();
        let mut deltas = Vec::new();
        let mut reasoning_deltas = Vec::new();

        handle_openai_sse_frame_with_sink(
            frame,
            &mut answer,
            &mut reasoning_content,
            &mut |delta| {
                deltas.push(delta);
            },
            &mut |delta| {
                reasoning_deltas.push(delta);
            },
        )
        .expect("stream frame should parse");

        assert_eq!(answer, "你好");
        assert_eq!(reasoning_content, "");
        assert_eq!(deltas, vec!["你".to_string(), "好".to_string()]);
        assert!(reasoning_deltas.is_empty());
    }

    #[test]
    fn sse_frame_captures_deepseek_reasoning_content() {
        let frame = concat!(
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"先分析\"}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"答案\"}}]}\n"
        );
        let mut answer = String::new();
        let mut reasoning_content = String::new();
        let mut deltas = Vec::new();
        let mut reasoning_deltas = Vec::new();

        handle_openai_sse_frame_with_sink(
            frame,
            &mut answer,
            &mut reasoning_content,
            &mut |delta| {
                deltas.push(delta);
            },
            &mut |delta| {
                reasoning_deltas.push(delta);
            },
        )
        .expect("stream frame should parse");

        assert_eq!(answer, "答案");
        assert_eq!(reasoning_content, "先分析");
        assert_eq!(deltas, vec!["答案".to_string()]);
        assert_eq!(reasoning_deltas, vec!["先分析".to_string()]);
    }
}
