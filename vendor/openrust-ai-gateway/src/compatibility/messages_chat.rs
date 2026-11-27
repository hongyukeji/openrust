use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::{chat, messages, responses};

pub(crate) fn request(mut request: Map<String, Value>) -> Result<Value, &'static str> {
    let stop = request
        .remove("stop_sequences")
        .filter(|stops| !stops.is_null());
    if stop.as_ref().is_some_and(|stops| {
        stops.as_array().is_none_or(|items| {
            items
                .iter()
                .any(|item| item.as_str().is_none_or(str::is_empty))
        })
    }) {
        return Err("stop_sequences 必须是非空文本组成的数组");
    }
    let Value::Object(request) = messages::to_responses(request)? else {
        return Err("无法构造 Chat Completions 请求");
    };
    let mut chat = chat::from_responses(request, false)?;
    if let Some(stop) = stop.filter(|stops| stops.as_array().is_some_and(|stops| !stops.is_empty()))
    {
        chat["stop"] = stop;
    }
    Ok(chat)
}

pub(crate) fn response(model: String, allows_tools: bool, data: &Value) -> Option<Value> {
    let response = chat::response(model.clone(), None, allows_tools, false, data)?;
    messages::Message::new(model, allows_tools).buffered(&response)
}

pub(crate) struct StreamState {
    upstream: chat::StreamState,
    message: messages::Message,
    downstream: messages::StreamState,
    next_output: u64,
    later_outputs: BTreeMap<u64, Vec<responses::Frame>>,
}

impl StreamState {
    pub(crate) fn new(model: String, allows_tools: bool) -> Self {
        Self {
            upstream: chat::StreamState::new(model.clone(), None, allows_tools, false),
            message: messages::Message::new(model, allows_tools),
            downstream: messages::StreamState::default(),
            next_output: 0,
            later_outputs: BTreeMap::new(),
        }
    }

    pub(crate) fn convert(&mut self, data: &Value) -> Option<(Vec<String>, bool)> {
        let (frames, terminal) = self.upstream.convert(data)?;
        self.emit(frames, terminal)
    }

    pub(crate) fn finish(&mut self) -> Option<(Vec<String>, bool)> {
        let (frames, terminal) = self.upstream.finish()?;
        self.emit(frames, terminal)
    }

    fn emit(
        &mut self,
        frames: Vec<responses::Frame>,
        terminal: bool,
    ) -> Option<(Vec<String>, bool)> {
        let mut output = Vec::new();
        let mut stopped = false;
        for frame in frames {
            if let Some(index) = frame.payload["output_index"].as_u64() {
                if index > self.next_output {
                    // Chat 允许多个调用交错，Messages 客户端以一个活动内容块读取。
                    self.later_outputs.entry(index).or_default().push(frame);
                    continue;
                }
                if index < self.next_output {
                    return None;
                }
            }
            self.forward(frame, &mut output, &mut stopped)?;
            while let Some(next) = self.later_outputs.remove(&self.next_output) {
                for frame in next {
                    self.forward(frame, &mut output, &mut stopped)?;
                }
            }
        }
        if terminal != stopped || (terminal && !self.later_outputs.is_empty()) {
            return None;
        }
        Some((output, terminal))
    }

    fn forward(
        &mut self,
        frame: responses::Frame,
        output: &mut Vec<String>,
        stopped: &mut bool,
    ) -> Option<()> {
        let (chunks, terminal) = self.downstream.convert(&self.message, &frame.payload)?;
        if frame.kind == "response.output_item.done" {
            self.next_output = self.next_output.checked_add(1)?;
        }
        *stopped |= terminal;
        output.extend(chunks);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::StreamState;

    #[test]
    fn anthropic_tool_blocks_close_in_order_when_chat_arguments_interleave() {
        let mut state = StreamState::new("public".into(), true);
        let (first, _) = state.convert(&json!({"choices":[{"delta":{"tool_calls":[
            {"index":0,"id":"call_one","type":"function","function":{"name":"lookup","arguments":"{\"sku\":"}},
            {"index":1,"id":"call_two","type":"function","function":{"name":"lookup","arguments":"{\"sku\":"}}
        ]}}]})).unwrap();
        let first = first.concat();
        assert!(first.contains("call_one") && !first.contains("call_two"));
        let (second, _) = state
            .convert(&json!({"choices":[{"delta":{"tool_calls":[
            {"index":1,"function":{"arguments":"2}"}},
            {"index":0,"function":{"arguments":"1}"}}
        ]},"finish_reason":"tool_calls"}]}))
            .unwrap();
        assert_eq!(second.len(), 1);
        assert!(second.concat().contains("1}"));
        state
            .convert(&json!({"choices":[],"usage":{"prompt_tokens":5,"completion_tokens":2}}))
            .unwrap();
        let (last, terminal) = state.finish().unwrap();
        assert!(terminal);
        let last = last.concat();
        let closed = last.find("event: content_block_stop").unwrap();
        let opened = last.find("call_two").unwrap();
        assert!(closed < opened);
        assert!(last.contains("\"partial_json\":\"2}\""));
        assert!(last.contains("\"stop_reason\":\"tool_use\""));
        assert!(last.contains("event: message_stop"));
    }
}
