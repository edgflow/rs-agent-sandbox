//! The one OpenAI endpoint the agent needs: a model response
//! (`POST /v1/responses`) with function calling, one tool call per turn.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const URL: &str = "https://api.openai.com/v1/responses";
const MODEL: &str = "gpt-5.4-mini";

pub struct OpenAi {
    http: reqwest::Client,
    key: String,
}

/// One item of the conversation. The API keeps nothing (`store: false`),
/// so every request carries the whole run so far.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Message {
        role: &'static str,
        content: String,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
}

/// The tool call the model chose: which tool, and its arguments as JSON.
#[derive(Deserialize)]
pub struct Call {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

impl Call {
    /// The call itself, to send back with the next request.
    pub fn to_item(&self) -> Item {
        Item::FunctionCall {
            call_id: self.call_id.clone(),
            name: self.name.clone(),
            arguments: self.arguments.clone(),
        }
    }
}

impl OpenAi {
    pub fn new(key: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            key,
        }
    }

    /// Asks the model for its next move. `tool_choice: required`: every
    /// turn is exactly one tool call.
    pub async fn next_call(
        &self,
        instructions: &str,
        input: &[Item],
        tools: &[Value],
    ) -> Result<Call> {
        let body = json!({
            "model": MODEL,
            "instructions": instructions,
            "input": input,
            "tools": tools,
            "tool_choice": "required",
            "parallel_tool_calls": false,
            "reasoning": { "effort": "low" },
            "store": false,
        });
        let response = self
            .http
            .post(URL)
            .bearer_auth(&self.key)
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("OpenAI answered {}", response.status());
        }
        let answer: Value = response.json().await?;
        let call = answer["output"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|item| item["type"] == "function_call")
            .context("the model answered without a tool call")?;
        Ok(serde_json::from_value(call.clone())?)
    }
}
