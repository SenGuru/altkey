//! Add/remove `127.0.0.1 <host>` redirect entries in the OS hosts file.
//! All altkey entries live inside a single marker-bounded block so removal is
//! exact and idempotent. Editing the real hosts file needs admin/root; the pure
//! text transform is unit-tested against an in-memory string.
use anyhow::{Context, Result};
use std::path::Path;

const BEGIN: &str = "# >>> altkey transparent >>>";
const END: &str = "# <<< altkey transparent <<<";

/// Return `content` with the altkey block (re)written to redirect `hosts`.
/// Pure function — no I/O — so it is fully unit-testable.
pub fn apply_block(content: &str, hosts: &[&str]) -> String {
    let stripped = strip_block(content);
    let mut out = stripped.trim_end().to_string();
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(BEGIN);
    out.push('\n');
    for h in hosts {
        out.push_str(&format!("127.0.0.1 {h}\n"));
    }
    out.push_str(END);
    out.push('\n');
    out
}

/// Return `content` with any altkey block removed.
pub fn strip_block(content: &str) -> String {
    let (Some(b), Some(e)) = (content.find(BEGIN), content.find(END)) else {
        return content.to_string();
    };
    if e < b {
        return content.to_string();
    }
    let end_idx = e + END.len();
    let mut result = String::new();
    result.push_str(&content[..b]);
    let rest = &content[end_idx..];
    result.push_str(rest.strip_prefix('\n').unwrap_or(rest));
    result.trim_end().to_string() + "\n"
}

/// Write the redirect block into the real hosts file at `path`.
pub fn enable(path: &Path, hosts: &[&str]) -> Result<()> {
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let next = apply_block(&current, hosts);
    std::fs::write(path, next).with_context(|| format!("writing hosts {}", path.display()))?;
    Ok(())
}

/// Remove the altkey block from the real hosts file at `path`.
pub fn disable(path: &Path) -> Result<()> {
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let next = strip_block(&current);
    std::fs::write(path, next).with_context(|| format!("writing hosts {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_then_strip_is_identity() {
        let original = "127.0.0.1 localhost\n::1 localhost\n";
        let with = apply_block(original, &["api.openai.com", "api.anthropic.com"]);
        assert!(with.contains("127.0.0.1 api.openai.com"));
        assert!(with.contains("127.0.0.1 api.anthropic.com"));
        assert!(with.contains(BEGIN) && with.contains(END));
        assert!(with.contains("127.0.0.1 localhost"));
        let back = strip_block(&with);
        assert!(!back.contains("api.openai.com"));
        assert!(!back.contains(BEGIN));
        assert!(back.contains("127.0.0.1 localhost"));
    }

    #[test]
    fn apply_is_idempotent() {
        let original = "127.0.0.1 localhost\n";
        let once = apply_block(original, &["api.openai.com"]);
        let twice = apply_block(&once, &["api.openai.com"]);
        assert_eq!(once, twice, "applying twice must not stack blocks");
        assert_eq!(twice.matches(BEGIN).count(), 1);
    }
}
