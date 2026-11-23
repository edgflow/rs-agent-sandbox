//! Lesson 4: the agent's work comes back as a unified diff against the
//! real crate, made here on the host. The real crate is never written.

use std::path::Path;

use anyhow::Result;
use similar::TextDiff;

use crate::sandbox::Changed;

/// One unified diff for all changed files. Files that end up exactly as
/// they were (or were made and removed again) are left out.
pub fn diff(krate: &Path, changes: &[Changed]) -> Result<String> {
    let mut patch = String::new();
    for file in changes {
        let original = std::fs::read_to_string(krate.join(&file.path)).ok();
        let before = original.as_deref().unwrap_or("");
        let after = file.content.as_deref().unwrap_or("");
        if original.is_none() && file.content.is_none() || before == after {
            continue;
        }
        let a = format!("a/{}", file.path);
        let b = format!("b/{}", file.path);
        let text = TextDiff::from_lines(before, after)
            .unified_diff()
            .context_radius(3)
            .header(&a, &b)
            .to_string();
        patch.push_str(&text);
    }
    Ok(patch)
}
