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
run cargo test. When the tests pass, call finish with a one-sentence \
summary of the fix.";

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
                    input.push(Item::FunctionCallOutput {
                        call_id: call.call_id,
                        output: format!("exit code: {code}\n{}", out.text),
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
