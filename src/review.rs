//! The agent's work comes back as a unified diff against the real crate,
//! made here on the host (Lesson 4). Lesson 7: a report built from the
//! audit log, and the patch is applied only if a person says yes.

use std::io::Write;
use std::path::Path;

use anyhow::Result;
use similar::TextDiff;

use crate::audit::Event;
use crate::sandbox::Changed;

/// The files whose text really differs from the real crate. An editor's
/// temporary file (made and removed again) or a file saved unchanged is
/// left out.
fn real_changes<'a>(krate: &Path, changes: &'a [Changed]) -> Vec<&'a Changed> {
    changes
        .iter()
        .filter(|file| {
            let original = std::fs::read_to_string(krate.join(&file.path)).ok();
            original != file.content
        })
        .collect()
}

/// The paths of the files that really changed, for the audit log.
pub fn changed_files(krate: &Path, changes: &[Changed]) -> Vec<String> {
    real_changes(krate, changes)
        .into_iter()
        .map(|file| file.path.clone())
        .collect()
}

/// One unified diff for all changed files.
pub fn diff(krate: &Path, changes: &[Changed]) -> String {
    let mut patch = String::new();
    for file in real_changes(krate, changes) {
        let original = std::fs::read_to_string(krate.join(&file.path)).ok();
        let before = original.as_deref().unwrap_or("");
        let after = file.content.as_deref().unwrap_or("");
        let a = format!("a/{}", file.path);
        let b = format!("b/{}", file.path);
        let text = TextDiff::from_lines(before, after)
            .unified_diff()
            .context_radius(3)
            .header(&a, &b)
            .to_string();
        patch.push_str(&text);
    }
    patch
}

/// What happened, counted from the audit log.
pub fn report(events: &[Event], max_steps: usize) {
    let mut steps = 0;
    let mut killed = 0;
    let mut refused = Vec::new();
    let mut allowed = Vec::new();
    let mut connects = 0;
    let mut files = Vec::new();
    for event in events {
        match event {
            Event::ToolCall { step, .. } => steps = *step,
            Event::CommandDone {
                exit_code: None, ..
            } => killed += 1,
            Event::Lookup { name, allowed: a } => {
                if *a { &mut allowed } else { &mut refused }.push(name.as_str())
            }
            Event::ConnectRefused { .. } => connects += 1,
            Event::Changes { files: f } => files = f.clone(),
            _ => {}
        }
    }
    println!("\n── review ──────────────────────────────────────────");
    println!(" steps          {steps} of {max_steps} allowed");
    println!(" killed         {killed} commands (time or output limit)");
    println!(" lookups        {} refused: {:?}", refused.len(), refused);
    println!("                {} allowed: {:?}", allowed.len(), allowed);
    println!(" connections    {connects} refused");
    println!(" files changed  {files:?}");
}

/// Asks a yes/no question on the terminal; anything but "y" is no.
pub fn ask(question: &str) -> Result<bool> {
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("y"))
}

/// Writes the agent's changes into the real crate. Called only after a
/// person said yes.
pub fn apply(krate: &Path, changes: &[Changed]) -> Result<()> {
    for file in real_changes(krate, changes) {
        let path = krate.join(&file.path);
        match &file.content {
            Some(text) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&path, text)?;
            }
            None => std::fs::remove_file(&path)?,
        }
        println!("  wrote {}", file.path);
    }
    Ok(())
}
