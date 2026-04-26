//! Trial checkpointing.
//!
//! A baseline run can be 50+ trials and take 1+ hours. Without
//! checkpointing, any process death (laptop sleep into shutdown,
//! harness exit, OOM) costs the full run. Instead every completed
//! `(trial, verdict)` pair is appended as one JSON line to a
//! checkpoint file; on restart we load the file and skip any cell
//! that already has its expected number of entries.
//!
//! Keying on `(section, task_id, model, trial_index)`: the
//! trial_index distinguishes the nth run within a (task, model)
//! cell. The checkpoint file is append-only; resume is idempotent.

use crate::judge::JudgeResult;
use crate::report::Section;
use crate::runner::TrialResult;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

/// One line in the checkpoint JSONL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointEntry {
    pub section: Section,
    pub task_id: String,
    pub model: String,
    pub trial_index: u32,
    pub trial: TrialResult,
    pub verdict: JudgeResult,
}

/// Load every entry from a checkpoint file. Non-existent files return
/// an empty vec (first run). Malformed lines are skipped with a
/// warning to stderr.
pub fn load(path: &Path) -> Result<Vec<CheckpointEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file =
        std::fs::File::open(path).with_context(|| format!("open checkpoint {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("read line {} of checkpoint", i + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<CheckpointEntry>(&line) {
            Ok(e) => out.push(e),
            Err(e) => eprintln!("[jig] skipping malformed checkpoint line {}: {e}", i + 1),
        }
    }
    Ok(out)
}

/// Append one entry to the checkpoint file. Each write is a single
/// JSON line followed by `\n` so partial writes are easy to detect on
/// resume (the malformed-line branch in `load` drops them).
pub fn append(path: &Path, entry: &CheckpointEntry) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open checkpoint for append: {}", path.display()))?;
    let line = serde_json::to_string(entry).context("serialize checkpoint entry")?;
    writeln!(file, "{line}").context("write checkpoint line")?;
    file.sync_data().context("fsync checkpoint")?;
    Ok(())
}

/// Is the (section, task, model, trial_index) cell already done?
pub fn has_entry<'a>(
    entries: &'a [CheckpointEntry],
    section: Section,
    task_id: &str,
    model: &str,
    trial_index: u32,
) -> Option<&'a CheckpointEntry> {
    entries.iter().find(|e| {
        e.section == section
            && e.task_id == task_id
            && e.model == model
            && e.trial_index == trial_index
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::judge::JudgeScore;

    fn entry(section: Section, task: &str, model: &str, idx: u32) -> CheckpointEntry {
        CheckpointEntry {
            section,
            task_id: task.into(),
            model: model.into(),
            trial_index: idx,
            trial: TrialResult {
                task_id: task.into(),
                model: model.into(),
                bash_commands: vec![],
                assistant_texts: vec![],
                num_turns: 1,
                input_tokens: 10,
                output_tokens: 5,
                cost_usd: 0.001,
                duration_ms: 100,
                terminal_reason: "completed".into(),
                is_error: false,
                completed_under_turn_cap: true,
                final_text: String::new(),
                setup_failed: false,
                timed_out: false,
            },
            verdict: JudgeResult {
                task_id: task.into(),
                model_under_test: model.into(),
                judge_model: "haiku".into(),
                first: JudgeScore {
                    score: 1.0,
                    first_command: Some("x".into()),
                    first_command_existed: true,
                    completed: true,
                    invented_commands: vec![],
                    fallback_to_sql: false,
                    reasoning: "r".into(),
                },
                second: None,
                irr_delta: None,
            },
        }
    }

    #[test]
    fn roundtrip_append_load() {
        let tmp = tempfile_path();
        let e1 = entry(Section::Tuning, "t1", "m1", 0);
        let e2 = entry(Section::Tuning, "t1", "m1", 1);
        append(&tmp, &e1).unwrap();
        append(&tmp, &e2).unwrap();
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].trial_index, 0);
        assert_eq!(loaded[1].trial_index, 1);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let p = Path::new("/tmp/jig-definitely-does-not-exist-xyz-42.jsonl");
        let loaded = load(p).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn has_entry_matches_on_all_keys() {
        let es = vec![
            entry(Section::Tuning, "t1", "m1", 0),
            entry(Section::Tuning, "t1", "m1", 1),
            entry(Section::Tuning, "t1", "m2", 0),
            entry(Section::Holdout, "t1", "m1", 0),
        ];
        assert!(has_entry(&es, Section::Tuning, "t1", "m1", 0).is_some());
        assert!(has_entry(&es, Section::Tuning, "t1", "m1", 2).is_none());
        assert!(has_entry(&es, Section::Tuning, "t2", "m1", 0).is_none());
        assert!(has_entry(&es, Section::Holdout, "t1", "m1", 0).is_some());
    }

    #[test]
    fn load_skips_malformed_lines() {
        let tmp = tempfile_path();
        std::fs::write(
            &tmp,
            format!(
                "{}\nnot json\n\n{}\n",
                serde_json::to_string(&entry(Section::Tuning, "t1", "m1", 0)).unwrap(),
                serde_json::to_string(&entry(Section::Tuning, "t1", "m1", 1)).unwrap(),
            ),
        )
        .unwrap();
        let loaded = load(&tmp).unwrap();
        assert_eq!(loaded.len(), 2);
        let _ = std::fs::remove_file(&tmp);
    }

    fn tempfile_path() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("jig-ckpt-test-{nanos}.jsonl"))
    }
}
