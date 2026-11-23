//! The agent: a loop that asks the model for its next tool call, runs it,
//! and sends back what happened.

use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::openai::{Item, OpenAi};

/// What a command printed (stdout, then stderr), and how it ended.
pub struct Output {
    pub exit_code: Option<i32>,
    pub text: String,
}

/// Where the agent's commands run. The agent never knows which one it has.
pub trait Executor {
    async fn run(&mut self, command: &str) -> Result<Output>;
}

const INSTRUCTIONS: &str = "You are a coding agent working in a Rust \
crate, the current directory. Use run_shell to read files, edit them and \
run cargo test. The machine may be Alpine Linux: a missing tool can be \
installed with `sudo apk add`, if the network allows it. When the tests \
pass, call finish with a one-sentence summary of the fix.";

const TASK: &str = "One of the tests fails. Find the bug, fix it, and run \
cargo test to confirm.";

pub struct Agent<E> {
    llm: OpenAi,
    exec: E,
    max_steps: usize,
}

#[derive(Deserialize)]
struct ShellArgs {
    command: String,
}

#[derive(Deserialize)]
struct FinishArgs {
    summary: String,
}

impl<E: Executor> Agent<E> {
    pub fn new(llm: OpenAi, exec: E, max_steps: usize) -> Self {
        Self {
            llm,
            exec,
            max_steps,
        }
    }

    /// Runs the task to the model's `finish`, then hands the executor back,
    /// so the caller can collect what the agent left in it.
    pub async fn fix(mut self) -> Result<E> {
        let mut input = vec![Item::Message {
            role: "user",
            content: TASK.into(),
        }];
        for step in 1..=self.max_steps {
            let call =
                self.llm.next_call(INSTRUCTIONS, &input, &tools()).await?;
            input.push(call.to_item());
            match call.name.as_str() {
                "run_shell" => {
                    let args: ShellArgs =
                        serde_json::from_str(&call.arguments)?;
                    println!("step {step:>2}  $ {}", args.command);
                    let out = self.exec.run(&args.command).await?;
                    show(&out);
                    let code =
                        out.exit_code.map_or("none".into(), |c| c.to_string());
                    let text = clip(&out.text, MODEL_OUTPUT_CHARS);
                    input.push(Item::FunctionCallOutput {
                        call_id: call.call_id,
                        output: format!("exit code: {code}\n{text}"),
                    });
                }
                "finish" => {
                    let args: FinishArgs =
                        serde_json::from_str(&call.arguments)?;
                    println!("done: {}", args.summary);
                    return Ok(self.exec);
                }
                other => bail!("the model called an unknown tool: {other}"),
            }
        }
        bail!("no finish after {} steps", self.max_steps)
    }
}

/// The tools the model may call, described as JSON schemas.
fn tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "name": "run_shell",
            "description": "Run a shell command in the crate's directory. \
                Returns its exit code and what it printed.",
            "parameters": {
                "type": "object",
                "properties": { "command": { "type": "string" } },
                "required": ["command"],
                "additionalProperties": false
            },
            "strict": true
        }),
        json!({
            "type": "function",
            "name": "finish",
            "description": "Call this once the tests pass.",
            "parameters": {
                "type": "object",
                "properties": { "summary": { "type": "string" } },
                "required": ["summary"],
                "additionalProperties": false
            },
            "strict": true
        }),
    ]
}

/// The first lines of a command's output, indented under its step.
fn show(out: &Output) {
    let lines: Vec<&str> = out.text.lines().collect();
    for line in lines.iter().take(12) {
        println!("          {line}");
    }
    if lines.len() > 12 {
        println!("          ... {} more lines", lines.len() - 12);
    }
}

/// The most of one command's output the model gets back.
const MODEL_OUTPUT_CHARS: usize = 4000;

/// `text` if it is short enough; otherwise its start and its end, with a
/// note of how much was cut between them (the end is where test results
/// and errors usually are).
fn clip(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let half = max / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(count - half).collect();
    let cut = count - 2 * half;
    format!("{head}\n[... {cut} characters cut ...]\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::clip;

    #[test]
    fn short_output_is_kept_whole() {
        assert_eq!(clip("all of it", 20), "all of it");
    }

    #[test]
    fn long_output_keeps_its_start_and_end() {
        let text = format!("start{}end", "x".repeat(100));
        let clipped = clip(&text, 10);
        assert_eq!(clipped, "start\n[... 98 characters cut ...]\nxxend");
    }
}
