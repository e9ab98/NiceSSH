//! Pure-string splice helpers for `.git/config` files.
//!
//! Every function in this module is a deterministic transformation
//! `&str -> String` (or `&str, &str, ... -> String`). They never touch
//! the filesystem. They are exercised by the relocated `tests` /
//! `rewrite_tests` / `https_splice_tests` modules.
//!
//! After splitting this module out of `commands/git.rs`, callers are:
//!   - [`crate::git::io`] (the IO wrapper layer)
//!   - [`crate::commands::git`] (the thin-shell Tauri IPC layer, for
//!     the global `set_global_git_config` command that calls
//!     `rewrite_global_defaults`)
//!
//! Visibility is `pub(crate)` for every helper.

use crate::config_store::Identity;
use crate::paths;

pub(crate) struct GitConfigSection {
    /// Section name (e.g. "user", "core", `includeIf "gitdir:~/work/"`)
    name: String,
    /// Raw lines belonging to this section (including the header `[name]`).
    /// For sections we want to rewrite, this is the authoritative content.
    raw: String,
    /// Whether this is an `[includeIf ...]` block. Such blocks must be
    /// preserved verbatim — we never touch their directives.
    is_include_if: bool,
}

pub(crate) fn splice_or_append_remote(raw: &str, remote_name: &str, url: &str) -> String {
    let mut lines: Vec<String> = raw.lines().map(|s| s.to_string()).collect();
    let header_re = format!("[remote \"{}\"]", remote_name);
    let mut i = 0usize;
    let mut found = false;
    while i < lines.len() {
        if lines[i].trim() == header_re {
            found = true;
            // Walk through the body of this remote block, looking
            // for the first `url =` line and replacing it in place.
            let mut j = i + 1;
            let mut replaced = false;
            while j < lines.len() {
                let t = lines[j].trim_start();
                if t.starts_with('[') && lines[j].trim_end().ends_with(']') {
                    break;
                }
                if !replaced && t.split_once('=').map(|(k, _)| k.trim().eq_ignore_ascii_case("url")).unwrap_or(false) {
                    lines[j] = format!("    url = {}", url);
                    replaced = true;
                    j += 1;
                    continue;
                }
                j += 1;
            }
            if !replaced {
                // No existing url — insert one as the first body line.
                lines.insert(i + 1, format!("    url = {}", url));
            }
            break;
        }
        i += 1;
    }
    if !found {
        // Trim trailing blank lines, append a fresh block.
        while lines.last().map(|s| s.trim().is_empty()).unwrap_or(false) {
            lines.pop();
        }
        lines.push(header_re);
        lines.push(format!("    url = {}", url));
        lines.push("    fetch = +refs/heads/*:refs/remotes/*/*".to_string());
    }
    let mut joined = lines.join("\n");
    joined.push('\n');
    joined
}

pub(crate) fn splice_identity_into_config(
    raw: &str,
    user_name: &str,
    user_email: &str,
    ssh_cmd: &str,
) -> String {
    fn is_header(s: &str) -> bool {
        let t = s.trim_start();
        t.starts_with('[') && s.trim_end().ends_with(']')
    }
    fn header_name(s: &str) -> Option<String> {
        let t = s.trim_start();
        if !(t.starts_with('[') && s.trim_end().ends_with(']')) {
            return None;
        }
        Some(t[1..t.len() - 1].trim().to_ascii_lowercase())
    }
    // Walk the file once, classifying every line.
    enum LineKind {
        Pass,                         // emit as-is
        MarkerComment,                // standalone `# nicessh-managed` — drop
        ManagedUserHeader,            // [user] — drop, plus its body
        ManagedUserBody,              // body line inside a [user] section
        CoreHeader,                   // [core] — emit header, then handle body
        CoreBodyPass,                 // body line inside [core] that's not sshCommand
        CoreBodySshCommand,           // body line inside [core] that starts with sshCommand — drop
        OtherHeader,                  // [remote "..."] / [branch "..."] / etc. — emit header + body
        OtherBody,                    // body line inside a non-user, non-core section
    }
    let lines: Vec<&str> = raw.lines().collect();
    let mut classified: Vec<LineKind> = Vec::with_capacity(lines.len());
    let mut in_user = false;
    let mut in_core = false;
    let mut in_other = false;
    for line in &lines {
        if is_header(line) {
            in_user = false;
            in_core = false;
            in_other = false;
            match header_name(line).unwrap_or_default().as_str() {
                "user" => {
                    in_user = true;
                    classified.push(LineKind::ManagedUserHeader);
                }
                "core" => {
                    in_core = true;
                    classified.push(LineKind::CoreHeader);
                }
                _ => {
                    in_other = true;
                    classified.push(LineKind::OtherHeader);
                }
            }
            continue;
        }
        if in_user {
            classified.push(LineKind::ManagedUserBody);
        } else if in_core {
            let trimmed = line.trim_start().to_ascii_lowercase();
            if trimmed.starts_with("sshcommand") {
                classified.push(LineKind::CoreBodySshCommand);
            } else {
                classified.push(LineKind::CoreBodyPass);
            }
        } else if in_other {
            classified.push(LineKind::OtherBody);
        } else {
            // File head.
            if line.trim() == "# nicessh-managed" {
                classified.push(LineKind::MarkerComment);
            } else {
                classified.push(LineKind::Pass);
            }
        }
    }
    // Now emit: pass through everything except managed lines, but
    // remember whether we've seen the first [core] header (so we
    // know where to splice the fresh sshCommand).
    let managed_ssh_line = format!("    sshCommand = {}  # nicessh-managed", ssh_cmd);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let mut core_header_seen = false;
    let mut core_spliced = false;
    for (line, kind) in lines.iter().zip(classified.iter()) {
        match kind {
            LineKind::Pass | LineKind::OtherHeader | LineKind::OtherBody
            | LineKind::CoreHeader | LineKind::CoreBodyPass => {
                out.push(line.to_string());
                if matches!(kind, LineKind::CoreHeader) {
                    core_header_seen = true;
                    if !core_spliced {
                        out.push(managed_ssh_line.clone());
                        core_spliced = true;
                    }
                }
            }
            LineKind::MarkerComment
            | LineKind::ManagedUserHeader
            | LineKind::ManagedUserBody
            | LineKind::CoreBodySshCommand => {
                // drop
            }
        }
    }
    if !core_spliced {
        // No [core] survived (or none existed). Append a fresh one.
        out.push("[core]".to_string());
        out.push(managed_ssh_line);
    }
    // Append the new [user] block at the end.
    out.push("[user]".to_string());
    out.push(format!("    name = {}", user_name));
    out.push(format!("    email = {}", user_email));
    // Reference core_header_seen so the compiler doesn't warn about
    // an unused binding if the loop never visits a CoreHeader.
    let _ = core_header_seen;
    // Trim trailing blank lines but keep at least one final newline.
    while out.last().map(|s| s.trim().is_empty()).unwrap_or(false) {
        out.pop();
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

pub(crate) fn splice_user_only_into_config(raw: &str, user_name: &str, user_email: &str) -> String {
    fn is_header(s: &str) -> bool {
        let t = s.trim_start();
        t.starts_with('[') && s.trim_end().ends_with(']')
    }
    fn header_name(s: &str) -> Option<String> {
        let t = s.trim_start();
        if !(t.starts_with('[') && s.trim_end().ends_with(']')) {
            return None;
        }
        Some(t[1..t.len() - 1].trim().to_ascii_lowercase())
    }
    // Single pass. Two outcomes:
    //   - PassThrough: emit the line verbatim.
    //   - ManagedUser{Header,Body}: drop (we are about to append a
    //     fresh [user] at the end of the file).
    // Every other section (including [core] with sshCommand, and
    // [remote "..."], etc.) is left entirely alone.
    enum LineKind { Pass, ManagedUserHeader, ManagedUserBody }
    let lines: Vec<&str> = raw.lines().collect();
    let mut classified: Vec<LineKind> = Vec::with_capacity(lines.len());
    let mut in_user = false;
    for line in &lines {
        if is_header(line) {
            in_user = matches!(
                header_name(line).unwrap_or_default().as_str(),
                "user"
            );
            classified.push(if in_user {
                LineKind::ManagedUserHeader
            } else {
                LineKind::Pass
            });
            continue;
        }
        classified.push(if in_user {
            LineKind::ManagedUserBody
        } else {
            LineKind::Pass
        });
    }
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
    for (line, kind) in lines.iter().zip(classified.iter()) {
        match kind {
            LineKind::Pass => out.push(line.to_string()),
            LineKind::ManagedUserHeader | LineKind::ManagedUserBody => {}
        }
    }
    // Append the fresh [user] block. This intentionally does *not*
    // touch [core], so any pre-existing sshCommand lines survive.
    out.push("[user]".to_string());
    out.push(format!("    name = {}", user_name));
    out.push(format!("    email = {}", user_email));
    while out.last().map(|s| s.trim().is_empty()).unwrap_or(false) {
        out.pop();
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

#[allow(dead_code)]
pub(crate) fn strip_managed_block(raw: &str) -> String {
    // Two-pass approach for clarity: first, scan the file and
    // remember the (start_line, end_line) ranges of every section
    // and whether each section is "managed" (drop it) or "kept"
    // (emit it).
    //
    // A section is the lines from one `[header]` (inclusive) up to
    // the next `[header]` (exclusive) or EOF. A section is "managed"
    // if its header is `user`, OR its header is `core` AND its body
    // contains a `sshCommand =` line. A standalone `# nicessh-
    // managed` comment is folded into the *next* section's body
    // during the scan; if that section is managed, the comment is
    // dropped along with the section.
    struct Section {
        start: usize, // line index of the [header] (or first line if file-head)
        header: Option<String>, // None for the file head
        body: Vec<usize>, // line indices of body lines (including marker comments)
    }
    let lines: Vec<&str> = raw.lines().collect();
    let mut sections: Vec<Section> = Vec::new();
    let mut i = 0;
    // File head: any lines before the first [section] header.
    let mut head_end = 0;
    while head_end < lines.len() {
        let t = lines[head_end].trim_start();
        if t.starts_with('[') && lines[head_end].trim_end().ends_with(']') {
            break;
        }
        head_end += 1;
    }
    if head_end > 0 {
        sections.push(Section { start: 0, header: None, body: (0..head_end).collect() });
    }
    i = head_end;
    while i < lines.len() {
        let t = lines[i].trim_start();
        let t_end = lines[i].trim_end();
        if t.starts_with('[') && t_end.ends_with(']') {
            let inner = t[1..t.len() - 1].trim().to_string();
            let mut body: Vec<usize> = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let nt = lines[j].trim_start();
                if nt.starts_with('[') && lines[j].trim_end().ends_with(']') {
                    break;
                }
                body.push(j);
                j += 1;
            }
            sections.push(Section { start: i, header: Some(inner), body });
            i = j;
        } else {
            i += 1;
        }
    }
    // Decide which sections are managed.
    let mut is_managed: Vec<bool> = sections.iter().map(|s| {
        match s.header.as_deref() {
            Some(h) if h.eq_ignore_ascii_case("user") => true,
            Some(h) if h.eq_ignore_ascii_case("core") => {
                s.body.iter().any(|&k| {
                    lines[k].trim_start().to_ascii_lowercase()
                        .starts_with("sshcommand")
                })
            }
            _ => false,
        }
    }).collect();
    // For the file head (header = None), strip out standalone
    // `# nicessh-managed` comment lines.
    for (idx, s) in sections.iter().enumerate() {
        if s.header.is_none() {
            // File head is "managed" only if it has nothing but
            // marker comments. We treat the head as "kept" by
            // default; we filter marker comments inline below.
            is_managed[idx] = false;
        }
    }
    // Build output: keep non-managed sections, drop managed ones.
    // Within non-managed bodies, drop standalone `# nicessh-managed`
    // comment lines.
    let mut out = String::with_capacity(raw.len());
    for (idx, s) in sections.iter().enumerate() {
        if is_managed[idx] {
            continue;
        }
        if s.header.is_none() {
            // File head: emit each line unless it is the marker.
            for &k in &s.body {
                if lines[k].trim() == "# nicessh-managed" {
                    continue;
                }
                out.push_str(lines[k]);
            out.push('\n');
            }
        } else {
            // Non-managed section: emit the [header], then body
            // (with marker comments stripped).
            out.push_str(lines[s.start]);
            out.push('\n');
            for &k in &s.body {
                if lines[k].trim() == "# nicessh-managed" {
                    continue;
                }
                out.push_str(lines[k]);
            out.push('\n');
            }
        }
    }
    out.trim_end().to_string()
}

pub(crate) fn build_user_block(name: &str, email: &str) -> String {
    format!("[user]\n    name = {}\n    email = {}\n", name, email)
}

pub(crate) fn build_core_sshcommand_line(ssh_cmd: &str, raw: &str) -> String {
    let has_other = raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('['))
        .any(|l| {
            let t = l.trim_start();
            !(t.starts_with("sshCommand") || t.starts_with("sshcommand"))
        });
    if has_other {
        format!("    sshCommand = {}\n", ssh_cmd)
    } else {
        format!("[core]\n    sshCommand = {}\n", ssh_cmd)
    }
}

pub(crate) fn rewrite_global_defaults(raw: &str, identity: &Identity) -> String {
    let sections = parse_gitconfig_sections(raw);

    let full_key = paths::resolve_key_path(&identity.key_path, &identity.label);
    let ssh_cmd = format!("ssh -i {} -o IdentitiesOnly=yes", full_key);

    // Look for existing [user] and [core] sections (case-insensitive, top-level only).
    let mut user_idx: Option<usize> = None;
    let mut core_idx: Option<usize> = None;
    for (i, s) in sections.iter().enumerate() {
        if s.is_include_if {
            continue;
        }
        match s.name.to_ascii_lowercase().as_str() {
            "user" if user_idx.is_none() => user_idx = Some(i),
            "core" if core_idx.is_none() => core_idx = Some(i),
            _ => {}
        }
    }

    let mut out_sections = sections;

    // Replace or append [user]
    if let Some(i) = user_idx {
        out_sections[i].raw = build_user_block(&identity.user_name, &identity.user_email);
    } else {
        out_sections.push(GitConfigSection {
            name: "user".to_string(),
            raw: build_user_block(&identity.user_name, &identity.user_email),
            is_include_if: false,
        });
    }

    // Replace or merge [core] sshCommand
    if let Some(i) = core_idx {
        // Recompute index in case we appended [user] above
        let i = if user_idx.is_none() { out_sections.len() - 1 } else { i };
        let existing = out_sections[i].raw.clone();
        out_sections[i].raw = build_core_sshcommand_line(&ssh_cmd, &existing);
    } else {
        out_sections.push(GitConfigSection {
            name: "core".to_string(),
            raw: build_core_sshcommand_line(&ssh_cmd, ""),
            is_include_if: false,
        });
    }

    // Reassemble. Trim trailing whitespace on each section to avoid piling
    // up blank lines; we'll add exactly one blank line between sections.
    let mut out = String::new();
    for (i, s) in out_sections.iter().enumerate() {
        if i > 0 {
            // Ensure separation: if previous content didn't end with \n\n,
            // insert a blank line.
            if !out.ends_with("\n\n") {
                if out.ends_with('\n') {
                    out.push('\n');
                } else {
                    out.push_str("\n\n");
                }
            }
        }
        out.push_str(s.raw.trim_end());
        out.push('\n');
    }
    out
}

pub(crate) fn parse_gitconfig_sections(raw: &str) -> Vec<GitConfigSection> {
    let mut sections: Vec<GitConfigSection> = Vec::new();
    let mut current: Option<GitConfigSection> = None;

    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') && trimmed.contains(']') {
            // New section header
            if let Some(s) = current.take() {
                sections.push(s);
            }
            let end = trimmed.find(']').unwrap();
            let name = trimmed[1..end].trim().to_string();
            let lower = name.to_ascii_lowercase();
            let is_include_if = lower.starts_with("includeif");
            current = Some(GitConfigSection {
                name,
                raw: format!("{}\n", line),
                is_include_if,
            });
        } else if let Some(s) = current.as_mut() {
            s.raw.push_str(line);
            s.raw.push('\n');
        } else {
            // Lines before any section (comments / blanks). Attach to next
            // section we create; for simplicity, treat as a synthetic
            // "_prefix" section.
            if sections.is_empty() && current.is_none() {
                // not inside a section yet; create a holder with the line
                current = Some(GitConfigSection {
                    name: String::new(),
                    raw: format!("{}\n", line),
                    is_include_if: false,
                });
            } else if let Some(last) = sections.last_mut() {
                last.raw.push_str(line);
                last.raw.push('\n');
            }
        }
    }
    if let Some(s) = current {
        sections.push(s);
    }
    sections
}


#[cfg(test)]
mod rewrite_tests {
    use super::*;
    use crate::config_store::Identity;

    fn ident() -> Identity {
        Identity {
            id: "i1".into(),
            label: "Work".into(),
            user_name: "工作名".into(),
            user_email: "work@x.com".into(),
            key_path: "~/.ssh/work_ed25519".into(),
            match_path: None,
            host_alias: None,
            git_host: None,
        }
    }

    #[test]
    fn preserves_include_if_block() {
        let raw = "[user]\n    name = old\n    email = old@x.com\n\n[includeIf \"gitdir:~/work/\"]\n    path = ~/.gitconfig-work\n\n[core]\n    sshCommand = ssh -i old\n";
        let after = rewrite_global_defaults(raw, &ident());
        assert!(after.contains("[includeIf \"gitdir:~/work/\"]"));
        assert!(after.contains("path = ~/.gitconfig-work"));
        assert!(after.contains("name = 工作名"));
        assert!(after.contains("email = work@x.com"));
        assert!(after.contains("sshCommand = ssh -i ~/.ssh/work_ed25519"));
        // No leftover old values in the rewritten [user] / [core] sshCommand
        assert!(!after.contains("name = old"));
        assert!(!after.contains("email = old@x.com"));
        assert!(!after.contains("ssh -i old\n"));
    }

    #[test]
    fn appends_user_when_missing() {
        let raw = "[includeIf \"gitdir:~/foo/\"]\n    path = ~/.gitconfig-foo\n";
        let after = rewrite_global_defaults(raw, &ident());
        assert!(after.contains("[includeIf \"gitdir:~/foo/\"]"));
        assert!(after.contains("[user]\n    name = 工作名"));
    }

    #[test]
    fn appends_core_sshcommand_when_missing() {
        let raw = "[user]\n    name = a\n    email = a@x.com\n";
        let after = rewrite_global_defaults(raw, &ident());
        assert!(after.contains("[core]\n    sshCommand = ssh -i ~/.ssh/work_ed25519"));
    }

    #[test]
    fn resolves_directory_keypath_with_label() {
        // New format: key_path is a directory; the actual private key is
        // at <key_path>/<label>. The rewritten sshCommand must point at
        // the resolved full path, not the directory.
        let mut id = ident();
        id.key_path = "/Users/x/.ssh/e9ab98-GitHub".into();
        id.label = "id_work".into();
        let raw = "";
        let after = rewrite_global_defaults(raw, &id);
        assert!(
            after.contains("sshCommand = ssh -i /Users/x/.ssh/e9ab98-GitHub/id_work"),
            "expected resolved full key path, got:\n{}",
            after
        );
    }

    #[test]
    fn handles_empty_input() {
        let raw = "";
        let after = rewrite_global_defaults(raw, &ident());
        assert!(after.contains("[user]"));
        assert!(after.contains("[core]"));
    }
}


#[cfg(test)]
mod https_splice_tests {
    use super::*;
    use crate::config_store::Identity;

    fn ident_user_only() -> Identity {
        Identity {
            id: "i_https".into(),
            label: "alice".into(),
            user_name: "Alice".into(),
            user_email: "alice@co.com".into(),
            key_path: "~/.ssh/id_alice".into(),
            match_path: None,
            host_alias: None,
            git_host: None,
        }
    }

    #[test]
    fn splice_user_only_replaces_user_block_keeps_core_intact() {
        // Pre-existing .git/config with a [core] sshCommand (legacy
        // build wrote it). user-only splice must NOT touch [core],
        // only swap [user].
        let raw = "[user]\n    name = Old\n    email = old@x\n[core]\n    sshCommand = ssh -i ~/.ssh/old\n\
                   [remote \"origin\"]\n    url = https://github.com/u/r.git\n";
        let out = splice_user_only_into_config(raw, "Alice", "alice@co.com");
        assert!(out.contains("[user]\n    name = Alice\n    email = alice@co.com"),
            "expected fresh user block; got: {{out}}");
        assert!(out.contains("sshCommand = ssh -i ~/.ssh/old"),
            "legacy sshCommand must be preserved untouched; got: {{out}}");
        assert!(out.contains("[remote \"origin\"]"));
        assert!(out.contains("url = https://github.com/u/r.git"));
        assert!(!out.contains("name = Old"));
    }

    #[test]
    fn splice_user_only_appends_user_when_missing() {
        let raw = "[core]\n    repositoryformatversion = 0\n";
        let out = splice_user_only_into_config(raw, "Alice", "alice@co.com");
        assert!(out.contains("[user]\n    name = Alice\n    email = alice@co.com"));
        assert!(!out.contains("sshCommand"), "got: {{out}}");
    }

    #[test]
    fn splice_user_only_is_idempotent() {
        let raw = "[user]\n    name = Alice\n    email = alice@co.com\n\
                   [core]\n    repositoryformatversion = 0\n\
                   [remote \"origin\"]\n    url = https://github.com/u/r.git\n";
        let once = splice_user_only_into_config(raw, "Alice", "alice@co.com");
        let twice = splice_user_only_into_config(&once, "Alice", "alice@co.com");
        let user_count = twice.matches("[user]").count();
        assert!(user_count == 1, "expected 1 [user] block, got {user_count}: {twice}");
    }

    #[test]
    fn splice_or_append_remote_replaces_existing_url() {
        let raw = "[remote \"origin\"]\n    url = https://github.com/old/r.git\n    fetch = +refs/heads/*:refs/remotes/origin/*\n[user]\n    name = A\n    email = a@x\n";
        let out = splice_or_append_remote(raw, "origin", "https://github.com/new/r.git");
        assert!(out.contains("url = https://github.com/new/r.git"), "got: {{out}}");
        assert!(!out.contains("old/r.git"));
        assert!(out.contains("fetch = +refs/heads/*:refs/remotes/origin/*"));
        assert!(out.contains("[user]\n    name = A"));
    }

    #[test]
    fn splice_or_append_remote_appends_when_missing() {
        let raw = "[user]\n    name = A\n    email = a@x\n";
        let out = splice_or_append_remote(raw, "upstream", "git@github.com:u/r.git");
        assert!(out.contains("[remote \"upstream\"]"), "got: {{out}}");
        assert!(out.contains("url = git@github.com:u/r.git"));
        assert!(out.contains("fetch = "), "default fetch line should be appended; got: {{out}}");
        assert!(out.contains("[user]\n    name = A"));
    }

    #[test]
    fn splice_or_append_remote_leaves_other_remotes_alone() {
        let raw = "[remote \"origin\"]\n    url = https://github.com/a/b.git\n[remote \"upstream\"]\n    url = https://github.com/c/d.git\n";
        let out = splice_or_append_remote(raw, "origin", "https://github.com/a/NEW.git");
        assert!(out.contains("url = https://github.com/a/NEW.git"));
        assert!(out.contains("[remote \"upstream\"]\n    url = https://github.com/c/d.git"));
    }
}


#[cfg(test)]
mod splice_tests {
    use super::*;

    #[test]
    fn test_strip_managed_block_keeps_only_last_marker() {
        // Simulate a .git/config polluted by 3 stacked identity
        // switches (3 nicessh-managed blocks). strip_managed_block
        // must drop ALL of them; the caller appends one fresh
        // managed block in `write_repo_gitconfig`. So after strip:
        //   - 0 `# nicessh-managed` markers
        //   - 0 `sshCommand` lines
        //   - 0 `[user]` sections
        //   - the [core] (git housekeeping) and [remote] are kept
        let raw = "[core]\n    repositoryformatversion = 0\n[remote \"origin\"]\n    url = git@github.com:x/y.git\n\n# nicessh-managed\n[user]\n    name = first\n    email = f@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n\n# nicessh-managed\n[user]\n    name = second\n    email = s@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k2\n\n# nicessh-managed\n[user]\n    name = third\n    email = t@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k3\n";
        let stripped = strip_managed_block(raw);
        assert_eq!(stripped.matches("# nicessh-managed").count(), 0,
            "strip must drop every managed marker, got: {}",
            stripped);
        assert_eq!(stripped.matches("sshCommand").count(), 0,
            "strip must drop every managed [core], got: {}",
            stripped);
        assert_eq!(stripped.matches("[user]").count(), 0,
            "strip must drop every [user], got: {}",
            stripped);
        // git's own scaffolding survives.
        assert!(stripped.contains("[remote"));
        assert!(stripped.contains("repositoryformatversion"));
    }

    #[test]
    fn test_strip_managed_block_no_marker_legacy_managed_blocks() {
        // Legacy file with two unmanaged [user]+[core] sshCommand
        // blocks (pre-marker era). strip must drop ALL of them
        // (the caller will append a fresh managed block).
        let raw = "[user]\n    name = first\n    email = f@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n[user]\n    name = second\n    email = s@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k2\n";
        let stripped = strip_managed_block(raw);
        assert_eq!(stripped.matches("[user]").count(), 0,
            "strip must drop every [user], got: {}",
            stripped);
        assert_eq!(stripped.matches("sshCommand").count(), 0,
            "strip must drop every [core]-with-sshCommand, got: {}",
            stripped);
    }

    #[test]
    fn test_strip_managed_block_no_managed_at_all() {
        // Plain git config with only remote/branch — must be returned
        // unchanged (legacy fallback path).
        let raw = "[core]\n    repositoryformatversion = 0\n[remote \"origin\"]\n    url = git@github.com:x/y.git\n";
        let stripped = strip_managed_block(raw);
        assert!(stripped.contains("[remote"));
        assert!(!stripped.contains("# nicessh-managed"));
    }

    // ---- splice_identity_into_config ----

    fn call_splice(raw: &str) -> String {
        splice_identity_into_config(raw, "Alice", "a@co.com", "ssh -i ~/.ssh/id_alice -o IdentitiesOnly=yes")
    }

    #[test]
    fn test_splice_replaces_old_managed_user_and_sshcommand_in_core() {
        // Old managed block in the canonical shape written by
        // previous NiceSSH builds: header comment, [user], [core]
        // with a single sshCommand line. Splice must drop the
        // marker comment + [user] and rewrite sshCommand in place.
        let raw = "\n# nicessh-managed\n[user]\n    name = Old\n    email = o@x\n[core]\n    sshCommand = ssh -i ~/.ssh/id_old -o IdentitiesOnly=yes\n";
        let out = call_splice(raw);
        assert!(!out.contains("Old"), "old [user] must be gone, got: {}", out);
        assert!(!out.contains("id_old"), "old sshCommand must be gone, got: {}", out);
        assert!(
        !out.lines().any(|l| l.trim() == "# nicessh-managed"),
        "no free-standing marker line, got: {}", out
    );
        assert!(out.contains("name = Alice"));
        assert!(out.contains("email = a@co.com"));
        assert!(out.contains("sshCommand = ssh -i ~/.ssh/id_alice -o IdentitiesOnly=yes  # nicessh-managed"));
        // exactly one user + exactly one core, exactly one sshCommand
        assert_eq!(out.matches("[user]").count(), 1);
        assert_eq!(out.matches("[core]").count(), 1);
        assert_eq!(out.matches("sshCommand").count(), 1);
    }

    #[test]
    fn test_splice_preserves_non_sshcore_keys() {
        // [core] carries both a nicessh sshCommand and user-added
        // housekeeping keys (autocrlf, filemode). Splice must keep
        // those intact and only touch the sshCommand line.
        let raw = "[core]\n    repositoryformatversion = 0\n    filemode = true\n    autocrlf = input\n    sshCommand = ssh -i ~/.ssh/id_old\n[user]\n    name = Old\n    email = o@x\n";
        let out = call_splice(raw);
        assert!(out.contains("repositoryformatversion = 0"));
        assert!(out.contains("filemode = true"));
        assert!(out.contains("autocrlf = input"));
        assert!(!out.contains("id_old"));
        assert!(!out.contains("name = Old"));
        assert!(out.contains("name = Alice"));
    }

    #[test]
    fn test_splice_keeps_remote_and_branch_sections() {
        // The whole point of the rewrite: don't blow away
        // [remote "..."] / [branch "..."] / [include ...] when
        // switching identity.
        let raw = "[remote \"origin\"]\n    url = git@github.com:x/y.git\n    fetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n    remote = origin\n    merge = refs/heads/main\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n[user]\n    name = Old\n    email = o@x\n";
        let out = call_splice(raw);
        assert!(out.contains("[remote \"origin\"]"));
        assert!(out.contains("url = git@github.com:x/y.git"));
        assert!(out.contains("[branch \"main\"]"));
        assert!(out.contains("merge = refs/heads/main"));
    }

    #[test]
    fn test_splice_handles_no_core_section() {
        // No [core] at all: append a fresh [core] block with just
        // the sshCommand line.
        let raw = "[remote \"origin\"]\n    url = git@github.com:x/y.git\n[user]\n    name = Old\n    email = o@x\n";
        let out = call_splice(raw);
        assert!(out.contains("[core]"));
        assert!(out.contains("sshCommand = ssh -i ~/.ssh/id_alice -o IdentitiesOnly=yes  # nicessh-managed"));
        assert!(out.contains("name = Alice"));
        // remote preserved
        assert!(out.contains("url = git@github.com:x/y.git"));
    }

    #[test]
    fn test_splice_handles_stacked_legacy_managed_blocks() {
        // Pre-marker-era or repeated-switch residue: two stacked
        // `[user]` + `[core] sshCommand` blocks. Splice must
        // collapse to a single `[user]` and a single sshCommand.
        let raw = "[user]\n    name = first\n    email = f@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n[user]\n    name = second\n    email = s@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k2\n";
        let out = call_splice(raw);
        assert_eq!(out.matches("[user]").count(), 1);
        assert_eq!(out.matches("sshCommand").count(), 1);
        assert!(out.contains("name = Alice"));
        assert!(!out.contains("name = first"));
        assert!(!out.contains("name = second"));
    }

    #[test]
    fn test_splice_is_idempotent() {
        // Splice the same identity twice -> result unchanged. This
        // is what `apply_identity_to_repo` does in practice when a
        // user re-applies the same identity; we must not pile up
        // duplicate sshCommand lines.
        let raw = "[core]\n    sshCommand = ssh -i ~/.ssh/k1\n[user]\n    name = X\n    email = x@y\n";
        let once = call_splice(raw);
        let twice = call_splice(&once);
        assert_eq!(once, twice, "splice must be idempotent");
        assert_eq!(twice.matches("sshCommand").count(), 1);
    }

    #[test]
    fn test_splice_drops_free_standing_marker_comment() {
        // Old builds wrote a free-standing `# nicessh-managed` at
        // the top of the file. After splice, the only place
        // `nicessh-managed` should appear is on the sshCommand
        // line itself.
        let raw = "# nicessh-managed\n[core]\n    sshCommand = ssh -i ~/.ssh/old\n[user]\n    name = Old\n    email = o@x\n";
        let out = call_splice(raw);
        assert_eq!(out.matches("# nicessh-managed").count(), 1,
            "marker should appear exactly once (on sshCommand), got: {}", out);
    }
}
