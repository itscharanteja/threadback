use std::{env, fs, path::PathBuf};

use threadback_core::*;
use ts_rs::TS;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = render();
    let destination = binding_path();
    let check_only = env::args().any(|argument| argument == "--check");

    if check_only {
        let existing = fs::read_to_string(&destination).unwrap_or_default();
        if existing != output {
            return Err(
                format!("{} is stale; run `npm run bindings`", destination.display()).into(),
            );
        }
    } else {
        fs::write(&destination, output)?;
        println!("wrote {}", destination.display());
    }

    Ok(())
}

fn binding_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("src/bindings/contracts.ts")
}

fn render() -> String {
    let declarations = [
        ProjectId::decl(),
        ThreadId::decl(),
        EventId::decl(),
        ArtifactId::decl(),
        CheckpointId::decl(),
        CorrectionId::decl(),
        ThreadState::decl(),
        NoteLabel::decl(),
        Sensitivity::decl(),
        Project::decl(),
        CreateProjectInput::decl(),
        ProjectPolicy::decl(),
        StartThreadInput::decl(),
        AddNoteInput::decl(),
        WorkThread::decl(),
        EventPayload::decl(),
        ThreadEvent::decl(),
        EventPage::decl(),
        ChangedPath::decl(),
        FileDiffStat::decl(),
        DiffStats::decl(),
        GitSnapshot::decl(),
        RecoveryReport::decl(),
        ArtifactRef::decl(),
        SourcedClaim::decl(),
        Confidence::decl(),
        Hypothesis::decl(),
        FailedAttempt::decl(),
        RelevantArtifact::decl(),
        NextAction::decl(),
        ResumeCapsule::decl(),
        Checkpoint::decl(),
        Correction::decl(),
        PreviewDisposition::decl(),
        PayloadPreviewEntry::decl(),
        PayloadPreview::decl(),
        AppStatus::decl(),
    ]
    .map(|declaration| declaration.replacen("type ", "export type ", 1));

    format!(
        "// This file is generated from Rust. Run `npm run bindings` after contract changes.\n\nexport type JsonValue = null | boolean | number | string | JsonValue[] | {{ [key: string]: JsonValue }};\n\n{}\n",
        declarations.join("\n\n")
    )
}
