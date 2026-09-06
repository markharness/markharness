use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::fs_safety::replace_file;
use crate::knowledge::{
    Behavior, Feature, Phase, Procedure, Requirement, Scenario, StepItem, contains_non_ascii,
    is_valid_slug, normalize_slug_candidate, parse_requirement, romanize_label, serialize_behavior,
    serialize_feature, serialize_requirement, serialize_scenario, strip_redundant_scenario_prefix,
};

fn list_candidate_ids(dir: &Path, marker_file: &str) -> Vec<String> {
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut ids: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.join(marker_file).is_file())
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .collect();
    ids.sort();
    ids
}

fn prompt_line<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
) -> io::Result<String> {
    loop {
        write!(writer, "{label}")?;
        writer.flush()?;
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            writeln!(writer, "入力が空です。もう一度入力してください。")?;
            continue;
        }
        return Ok(trimmed);
    }
}

/// A line that may legitimately be blank (signaling "finished"/"none"), so
/// unlike `prompt_line` this never reprompts on an empty line.
fn prompt_optional_line<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
) -> io::Result<String> {
    write!(writer, "{label}")?;
    writer.flush()?;
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn prompt_id_or_label<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
    candidates: &[String],
) -> io::Result<(String, String)> {
    for (i, id) in candidates.iter().enumerate() {
        writeln!(writer, "  {}) {}", i + 1, id)?;
    }
    loop {
        let value = prompt_line(reader, writer, label)?;
        if let Ok(n) = value.parse::<usize>()
            && n >= 1
            && n <= candidates.len()
        {
            let id = candidates[n - 1].clone();
            return Ok((id.clone(), id));
        }
        if !contains_non_ascii(&value) {
            if is_valid_slug(&value) {
                return Ok((value.clone(), value));
            }
            writeln!(
                writer,
                "id は小文字英数字とハイフンのみ使用できます。もう一度入力してください。"
            )?;
            continue;
        }

        let candidate_slug = normalize_slug_candidate(&romanize_label(&value));
        write!(
            writer,
            "id候補: {candidate_slug} (Enterで採用、編集する場合は入力): "
        )?;
        writer.flush()?;
        let mut edit_line = String::new();
        reader.read_line(&mut edit_line)?;
        let edited = edit_line.trim();
        let final_id = if edited.is_empty() {
            candidate_slug
        } else {
            normalize_slug_candidate(edited)
        };

        if candidates.iter().any(|c| c == &final_id) {
            writeln!(
                writer,
                "id '{final_id}' は既存の候補と衝突しています。もう一度入力してください。"
            )?;
            continue;
        }
        if !is_valid_slug(&final_id) {
            writeln!(
                writer,
                "id は小文字英数字とハイフンのみ使用できます。もう一度入力してください。"
            )?;
            continue;
        }
        return Ok((final_id, value));
    }
}

fn prompt_axis<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
) -> io::Result<Vec<String>> {
    let axis_line = prompt_line(reader, writer, label)?;
    Ok(axis_line
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// `steps`は1行1操作、空行で入力終了。少なくとも1つ必須。stdinがEOFに達した
/// 場合、1つもstepが無ければエラーを返す(空行の繰り返し要求でハングしない
/// ように)。1つ以上あれば、空行での終了と同様にそこまで集めたstepsを返す。
fn prompt_steps<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
) -> io::Result<Vec<String>> {
    writeln!(writer, "{label}")?;
    let mut steps = Vec::new();
    loop {
        write!(writer, "  step {}: ", steps.len() + 1)?;
        writer.flush()?;
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        let trimmed = line.trim().to_string();
        if bytes_read == 0 || trimmed.is_empty() {
            if steps.is_empty() {
                if bytes_read == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "input ended before at least one step was provided",
                    ));
                }
                writeln!(writer, "少なくとも1つのstepを入力してください。")?;
                continue;
            }
            return Ok(steps);
        }
        steps.push(trimmed);
    }
}

/// `prompt_steps`と同じ入力形式だが、1つも入力せず最初の行を空にして終える
/// ことを許す(procedureの有無やPhaseの追加継続確認のような0要素許容箇所向け)。
fn prompt_optional_steps<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
) -> io::Result<Vec<String>> {
    writeln!(writer, "{label}")?;
    let mut steps = Vec::new();
    loop {
        write!(writer, "  step {}: ", steps.len() + 1)?;
        writer.flush()?;
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        let trimmed = line.trim().to_string();
        if bytes_read == 0 || trimmed.is_empty() {
            return Ok(steps);
        }
        steps.push(trimmed);
    }
}

/// ADR 0017 §2: プロンプトで0個以上の共通手順(procedure)を集める。手順名を
/// 空行で終了する。
fn prompt_procedures<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
) -> io::Result<BTreeMap<String, Procedure>> {
    let mut procedures = BTreeMap::new();
    loop {
        let name = prompt_optional_line(
            reader,
            writer,
            "Procedure name (blank to finish, e.g. login): ",
        )?;
        if name.is_empty() {
            return Ok(procedures);
        }
        let steps = prompt_steps(
            reader,
            writer,
            "Procedure steps (one operation per line, blank line to finish, e.g. Enter credentials.):",
        )?;
        procedures.insert(name, Procedure { steps });
    }
}

/// ADR 0017 §2: プロンプトで1個以上のPhase(操作+観測可能な結果)を集める。
/// 各Phaseの操作は`action:`のみ(`use:`参照は対話フローでは扱わない — バッチ
/// /LLM経由のdraftファイルで行う)。最初のPhase以降は、次のPhaseの操作を
/// 空行で開始してScenario全体を終了できる。
fn prompt_phases<R: BufRead, W: Write>(reader: &mut R, writer: &mut W) -> io::Result<Vec<Phase>> {
    let mut phases = Vec::new();
    loop {
        let steps = if phases.is_empty() {
            prompt_steps(
                reader,
                writer,
                "Scenario steps (one operation per line, blank line to finish this phase, e.g. Leave the title field empty.):",
            )?
        } else {
            let steps = prompt_optional_steps(
                reader,
                writer,
                "Next phase steps (blank first line to finish the scenario, one operation per line otherwise):",
            )?;
            if steps.is_empty() {
                return Ok(phases);
            }
            steps
        };
        let results = prompt_steps(
            reader,
            writer,
            "Observable results for this phase (one per line, blank line to finish, e.g. Shows a validation error under the input field.):",
        )?;
        phases.push(Phase {
            steps: steps
                .into_iter()
                .map(|action| StepItem::Action { action })
                .collect(),
            results,
        });
    }
}

pub fn run_add<R: BufRead, W: Write>(
    root: &Path,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<()> {
    let knowledge_root = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge");

    let requirements_root = knowledge_root.join("requirements");
    let features_root = knowledge_root.join("features");

    let requirement_candidates = list_candidate_ids(&requirements_root, "requirement.yml");
    let (requirement_id, requirement_label) = prompt_id_or_label(
        reader,
        writer,
        "Requirement name (e.g. task-management): ",
        &requirement_candidates,
    )?;
    let requirement_dir = requirements_root.join(&requirement_id);
    let requirement_path = requirement_dir.join("requirement.yml");
    // ADR 0017 §1・§3: Featureは表示IDではなくRequirement.uidで関連付ける。
    // 新規作成したRequirementは`identity migrate`実行前で常にuid: Noneで
    // あり、既存Requirementも未migrateならuidを持たない。
    let requirement_uid: Option<String> = if requirement_path.exists() {
        writeln!(
            writer,
            "既存のRequirement '{requirement_id}' を再利用します。"
        )?;
        fs::read_to_string(&requirement_path)
            .ok()
            .and_then(|yaml| parse_requirement(&yaml).ok())
            .and_then(|requirement| requirement.uid)
    } else {
        let axis = prompt_axis(
            reader,
            writer,
            "Requirement axis (comma separated, e.g. ui, validation): ",
        )?;
        let requirement = Requirement {
            id: requirement_id.clone(),
            label: requirement_label,
            axis,
            description: None,
            source: None,
            related_issues: Vec::new(),
            uid: None,
        };
        replace_file(
            root,
            &requirement_path,
            serialize_requirement(&requirement).as_bytes(),
        )?;
        // ADR 0017 §1・§3: a brand-new Requirement is always `uid: None`
        // until `identity migrate` runs (uids are issued only there, never
        // by `knowledge add`). A Feature can never be created referencing
        // it in this same run, so stop here — successfully, with the
        // Requirement written — rather than continuing on to prompt for a
        // Feature only to reject it afterward and leave that prompting
        // effort (and a confusing error) as the session's outcome.
        writeln!(
            writer,
            "Requirement '{requirement_id}' を作成しました。Featureから参照できるようにするには、先に `markharness identity migrate` を実行してから再度 `markharness knowledge add` を実行してください。"
        )?;
        return Ok(());
    };

    let feature_candidates = list_candidate_ids(&features_root, "feature.yml");
    let (feature_id, feature_label) = prompt_id_or_label(
        reader,
        writer,
        "Feature name (e.g. add-todo): ",
        &feature_candidates,
    )?;
    let feature_dir = features_root.join(&feature_id);
    let feature_path = feature_dir.join("feature.yml");
    if feature_path.exists() {
        writeln!(writer, "既存のFeature '{feature_id}' を再利用します。")?;
    } else {
        let Some(requirement_uid) = requirement_uid else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "requirement \"{requirement_id}\" has no uid yet; run `markharness identity migrate` before creating a Feature that references it"
                ),
            ));
        };
        let axis = prompt_axis(
            reader,
            writer,
            "Axis (comma separated, e.g. ui, validation): ",
        )?;
        let feature = Feature {
            id: feature_id.clone(),
            requirement_uids: vec![requirement_uid],
            label: feature_label,
            axis,
            description: None,
            forked_from: None,
            uid: None,
        };
        replace_file(root, &feature_path, serialize_feature(&feature).as_bytes())?;
    }

    let behavior_candidates = list_candidate_ids(&feature_dir, "behavior.yml");
    let (behavior_id, behavior_label) = prompt_id_or_label(
        reader,
        writer,
        "Behavior name (e.g. add-task): ",
        &behavior_candidates,
    )?;
    let behavior_dir = feature_dir.join(&behavior_id);
    let behavior_path = behavior_dir.join("behavior.yml");
    if behavior_path.exists() {
        writeln!(writer, "既存のBehavior '{behavior_id}' を再利用します。")?;
    } else {
        let axis = prompt_axis(
            reader,
            writer,
            "Behavior axis (comma separated, e.g. ui, validation): ",
        )?;
        let description = prompt_line(
            reader,
            writer,
            "Behavior description (e.g. User adds a new task to the list.): ",
        )?;
        let procedures = prompt_procedures(reader, writer)?;
        let behavior = Behavior {
            id: behavior_id.clone(),
            feature: feature_id.clone(),
            label: behavior_label,
            axis,
            description,
            procedures,
            uid: None,
        };
        replace_file(
            root,
            &behavior_path,
            serialize_behavior(&behavior).as_bytes(),
        )?;
    }

    let scenario_candidates = list_candidate_ids(&behavior_dir, "scenario.yml");
    let (raw_scenario_id, scenario_label) = prompt_id_or_label(
        reader,
        writer,
        "Scenario name (e.g. empty-title): ",
        &scenario_candidates,
    )?;
    let scenario_id = {
        let raw_path = behavior_dir.join(&raw_scenario_id).join("scenario.yml");
        if raw_path.exists() {
            raw_scenario_id
        } else if let Some(stripped) =
            strip_redundant_scenario_prefix(&behavior_id, &raw_scenario_id)
        {
            writeln!(
                writer,
                "Scenario id '{raw_scenario_id}' から Behavior id '{behavior_id}' と重複する接頭辞を除去し、'{stripped}' として作成します。"
            )?;
            stripped
        } else {
            raw_scenario_id
        }
    };
    let scenario_dir = behavior_dir.join(&scenario_id);
    let scenario_path = scenario_dir.join("scenario.yml");
    if scenario_path.exists() {
        writeln!(writer, "既存のScenario '{scenario_id}' を再利用します。")?;
        return Ok(());
    }

    let description = prompt_line(
        reader,
        writer,
        "Scenario description (e.g. Submit the todo form with an empty title): ",
    )?;
    let phases = prompt_phases(reader, writer)?;
    let scenario = Scenario {
        id: scenario_id,
        behavior: behavior_id,
        label: scenario_label,
        description,
        phases,
        implementation_note: None,
        generated_by: None,
        verified_by: None,
        uid: None,
    };
    replace_file(
        root,
        &scenario_path,
        serialize_scenario(&scenario).as_bytes(),
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Cursor;

    fn run_with_input(root: &std::path::Path, input: &str) {
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();
        run_add(root, &mut reader, &mut writer).unwrap();
    }

    fn run_with_input_capturing_output(root: &std::path::Path, input: &str) -> String {
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();
        run_add(root, &mut reader, &mut writer).unwrap();
        String::from_utf8(writer).unwrap()
    }

    const FULL_INPUT: &str = "controls\ngameplay\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\nground\nJump from the ground and land\nDo it.\n\nlands safely\n\n\n";

    /// Same chain as `FULL_INPUT`, but assuming `controls` already exists as
    /// an already-migrated Requirement (real uid) on disk, so no axis prompt
    /// is consumed for it (`prompt_id_or_label` matches the literal existing
    /// id and `run_add` reuses it, skipping straight to the Feature name
    /// prompt — see `reuses_existing_requirement_and_skips_axis_prompt`).
    const SEEDED_FULL_INPUT: &str = "controls\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\nground\nJump from the ground and land\nDo it.\n\nlands safely\n\n\n";

    /// `identity migrate`実行後を模したRequirement.uid値(ULID形式)。
    const CONTROLS_REQUIREMENT_UID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn write_migrated_controls_requirement(dir: &Path) {
        fs::create_dir_all(dir.join(".markharness/knowledge/requirements/controls")).unwrap();
        fs::write(
            dir.join(".markharness/knowledge/requirements/controls/requirement.yml"),
            format!(
                "id: controls\nlabel: controls\naxis: [gameplay]\nuid: {CONTROLS_REQUIREMENT_UID}\n"
            ),
        )
        .unwrap();
    }

    /// A fresh root with `controls` already migrated (real uid), the
    /// starting point for every test whose scenario is not itself about
    /// Requirement-creation behavior: since ADR 0017 §1・§3 means `run_add`
    /// can no longer create a brand-new Requirement and a Feature
    /// referencing it in the same call, these tests need a pre-migrated
    /// Requirement to build the rest of the chain (Feature/Behavior/
    /// Scenario) on top of, via `SEEDED_FULL_INPUT`.
    fn setup_migrated_requirement_root() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        write_migrated_controls_requirement(dir.path());
        dir
    }

    /// Appends a `uid:` line to an existing `requirement.yml`, simulating
    /// what `identity migrate` does to a brand-new (uid: None) Requirement.
    fn simulate_migrate(requirement_path: &Path, uid: &str) {
        let mut content = fs::read_to_string(requirement_path).unwrap();
        content.push_str(&format!("uid: {uid}\n"));
        fs::write(requirement_path, content).unwrap();
    }

    #[test]
    fn prompt_steps_errors_instead_of_looping_forever_when_stdin_hits_eof_before_any_step() {
        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut writer = Vec::new();

        let result = prompt_steps(&mut reader, &mut writer, "Behavior steps:");

        let err = result.expect_err("EOF before any step must return an error, not loop forever");
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn prompt_steps_returns_collected_steps_when_stdin_hits_eof_after_at_least_one_step() {
        let mut reader = Cursor::new(b"Press the jump button.\n".to_vec());
        let mut writer = Vec::new();

        let steps = prompt_steps(&mut reader, &mut writer, "Behavior steps:").unwrap();

        assert_eq!(steps, vec!["Press the jump button.".to_string()]);
    }

    #[test]
    fn prompt_optional_steps_returns_empty_when_the_first_line_is_blank() {
        let mut reader = Cursor::new(b"\n".to_vec());
        let mut writer = Vec::new();

        let steps = prompt_optional_steps(&mut reader, &mut writer, "Next phase steps:").unwrap();

        assert_eq!(steps, Vec::<String>::new());
    }

    #[test]
    fn prompt_optional_steps_collects_lines_until_a_blank_line() {
        let mut reader = Cursor::new(b"Reload the page.\n\n".to_vec());
        let mut writer = Vec::new();

        let steps = prompt_optional_steps(&mut reader, &mut writer, "Next phase steps:").unwrap();

        assert_eq!(steps, vec!["Reload the page.".to_string()]);
    }

    #[test]
    fn prompt_procedures_returns_empty_map_when_the_first_name_is_blank() {
        let mut reader = Cursor::new(b"\n".to_vec());
        let mut writer = Vec::new();

        let procedures = prompt_procedures(&mut reader, &mut writer).unwrap();

        assert!(procedures.is_empty());
    }

    #[test]
    fn prompt_procedures_collects_named_procedures_until_a_blank_name() {
        let mut reader =
            Cursor::new(b"login\nEnter credentials.\nPress the login button.\n\n\n".to_vec());
        let mut writer = Vec::new();

        let procedures = prompt_procedures(&mut reader, &mut writer).unwrap();

        assert_eq!(procedures.len(), 1);
        assert_eq!(
            procedures["login"].steps,
            vec![
                "Enter credentials.".to_string(),
                "Press the login button.".to_string()
            ]
        );
    }

    /// ADR 0017 §1・§3: a brand-new Requirement is always `uid: None` until
    /// `identity migrate` runs, so `run_add` can no longer continue on in
    /// the same call to create a Feature that references it — it writes
    /// the Requirement and stops there, successfully.
    #[test]
    fn creates_new_requirement_only_and_stops_before_feature_prompt() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(dir.path(), "controls\ngameplay\n");

        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/requirements/controls/requirement.yml");
        assert_eq!(
            fs::read_to_string(requirement_path).unwrap(),
            "id: controls\nlabel: controls\naxis: [gameplay]\n"
        );
        assert!(
            !dir.path().join(".markharness/knowledge/features").exists(),
            "run_add must not proceed to Feature creation for an unmigrated Requirement"
        );
        assert!(output.contains("identity migrate"));
    }

    #[test]
    fn creates_feature_behavior_and_scenario_from_a_migrated_requirement() {
        let dir = setup_migrated_requirement_root();

        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        let feature_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/feature.yml");
        let behavior_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/jump/behavior.yml");
        let scenario_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml");

        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            format!(
                "id: player-jump\nrequirement_uids: [{CONTROLS_REQUIREMENT_UID}]\nlabel: player-jump\naxis: [gameplay, animation]\n"
            )
        );
        assert_eq!(
            fs::read_to_string(behavior_path).unwrap(),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures: {}\n"
        );
        assert_eq!(
            fs::read_to_string(scenario_path).unwrap(),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground and land\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands safely\"\n"
        );
    }

    #[test]
    fn reuses_existing_feature_and_skips_axis_prompt() {
        let dir = setup_migrated_requirement_root();
        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        // Second run reuses the feature: no axis prompt is consumed, so the
        // second input line is the behavior id, not an axis list.
        run_with_input(
            dir.path(),
            "controls\nplayer-jump\nair\ngameplay\nPlayer presses jump while airborne.\n\nspace\nJump while airborne\nDo it.\n\nlands on platform\n\n\n",
        );

        let feature_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/feature.yml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            format!(
                "id: player-jump\nrequirement_uids: [{CONTROLS_REQUIREMENT_UID}]\nlabel: player-jump\naxis: [gameplay, animation]\n"
            )
        );
        let behavior_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/air/behavior.yml");
        assert_eq!(
            fs::read_to_string(behavior_path).unwrap(),
            "id: air\nfeature: player-jump\nlabel: air\naxis: [gameplay]\ndescription: |\n  Player presses jump while airborne.\nprocedures: {}\n"
        );
    }

    #[test]
    fn reuses_existing_behavior_and_skips_axis_and_description_prompt() {
        let dir = setup_migrated_requirement_root();
        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        // Second run reuses feature and behavior: no axis/description/procedure prompts.
        run_with_input(
            dir.path(),
            "controls\nplayer-jump\njump\nair\nJump while airborne\nDo it.\n\nlands on platform\n\n\n",
        );

        let scenario_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/jump/air/scenario.yml");
        assert_eq!(
            fs::read_to_string(scenario_path).unwrap(),
            "id: air\nbehavior: jump\nlabel: air\ndescription: |\n  Jump while airborne\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands on platform\"\n"
        );
    }

    #[test]
    fn reuses_existing_scenario_and_writes_nothing_new() {
        let dir = setup_migrated_requirement_root();
        run_with_input(dir.path(), SEEDED_FULL_INPUT);
        let scenario_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml");
        let before = fs::read_to_string(&scenario_path).unwrap();

        // Second run reuses feature, behavior and scenario: run_add returns
        // immediately after the reuse message, no further prompts consumed.
        run_with_input(dir.path(), "controls\nplayer-jump\njump\nground\n");

        let after = fs::read_to_string(&scenario_path).unwrap();
        assert_eq!(
            before, after,
            "reusing an existing Scenario must not rewrite it"
        );
    }

    #[test]
    fn no_candidate_list_printed_for_fresh_knowledge_dir() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(dir.path(), FULL_INPUT);

        assert!(!output.contains("1)"));
    }

    #[test]
    fn lists_feature_candidates_by_number_and_selects_by_index() {
        let dir = setup_migrated_requirement_root();

        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\n1\nair\ngameplay\nPlayer presses jump while airborne.\n\nspace\nJump while airborne\nDo it.\n\nlands on platform\n\n\n",
        );

        assert!(output.contains("  1) player-jump\n"));
        assert!(output.contains("既存のFeature 'player-jump' を再利用します。"));
        let behavior_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/air/behavior.yml");
        assert!(behavior_path.exists());
    }

    #[test]
    fn lists_behavior_candidates_by_number_and_selects_by_index() {
        let dir = setup_migrated_requirement_root();

        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\nplayer-jump\n1\nspace\nJump while airborne\nDo it.\n\nlands on platform\n\n\n",
        );

        assert!(output.contains("  1) jump\n"));
        assert!(output.contains("既存のBehavior 'jump' を再利用します。"));
        let scenario_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/jump/space/scenario.yml");
        assert!(scenario_path.exists());
    }

    #[test]
    fn lists_scenario_candidates_by_number_and_selects_by_index() {
        let dir = setup_migrated_requirement_root();

        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        let output =
            run_with_input_capturing_output(dir.path(), "controls\nplayer-jump\njump\n1\n");

        assert!(output.contains("  1) ground\n"));
        assert!(output.contains("既存のScenario 'ground' を再利用します。"));
    }

    #[test]
    fn typing_literal_existing_id_with_candidates_present_still_works() {
        let dir = setup_migrated_requirement_root();

        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        let output =
            run_with_input_capturing_output(dir.path(), "controls\nplayer-jump\njump\nground\n");

        assert!(output.contains("既存のScenario 'ground' を再利用します。"));
    }

    #[test]
    fn auto_dedup_strips_redundant_scenario_prefix_and_notifies() {
        let dir = setup_migrated_requirement_root();

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\njump-ground\nJump from the ground and land\nDo it.\n\nlands safely\n\n\n",
        );

        assert!(output.contains(
            "Scenario id 'jump-ground' から Behavior id 'jump' と重複する接頭辞を除去し、'ground' として作成します。"
        ));
        assert!(
            dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/ground/scenario.yml")
                .exists()
        );
        assert!(
            !dir.path()
                .join(".markharness/knowledge/features/player-jump/jump/jump-ground")
                .exists()
        );
    }

    #[test]
    fn legacy_scenario_dir_with_redundant_prefix_is_reused_without_stripping() {
        let dir = setup_migrated_requirement_root();

        run_with_input(
            dir.path(),
            "controls\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\njump-ground\nJump from the ground and land\nDo it.\n\nlands safely\n\n\n",
        );
        // Above run already dedupes to `ground/`; create a legacy dir with the
        // literal redundant name directly on disk to simulate pre-existing data.
        let legacy_dir = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/jump/jump-ground");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(
            legacy_dir.join("scenario.yml"),
            "id: jump-ground\nbehavior: jump\nlabel: jump-ground\ndescription: |\n  legacy\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"Confirmed.\"\n",
        )
        .unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\nplayer-jump\njump\njump-ground\n",
        );

        assert!(!output.contains("重複する接頭辞を除去"));
        assert!(output.contains("既存のScenario 'jump-ground' を再利用します。"));
    }

    #[test]
    fn prompt_id_or_label_suggests_romanized_slug_and_accepts_on_empty_input() {
        let input = "プレイヤーがジャンプする\n\n";
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();

        let (id, label) =
            prompt_id_or_label(&mut reader, &mut writer, "Feature id: ", &[]).unwrap();

        assert_eq!(id, "pureiyaagajanpusuru");
        assert_eq!(label, "プレイヤーがジャンプする");
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("pureiyaagajanpusuru"));
    }

    #[test]
    fn prompt_id_or_label_accepts_edited_candidate() {
        let input = "プレイヤーがジャンプする\nplayer-jump\n";
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();

        let (id, label) =
            prompt_id_or_label(&mut reader, &mut writer, "Feature id: ", &[]).unwrap();

        assert_eq!(id, "player-jump");
        assert_eq!(label, "プレイヤーがジャンプする");
    }

    #[test]
    fn prompt_id_or_label_warns_and_reprompts_on_slug_collision() {
        let candidates = vec!["pureiyaagajanpusuru".to_string()];
        let input = "プレイヤーがジャンプする\n\nプレイヤーがジャンプする\nplayer-jump\n";
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();

        let (id, label) =
            prompt_id_or_label(&mut reader, &mut writer, "Feature id: ", &candidates).unwrap();

        assert_eq!(id, "player-jump");
        assert_eq!(label, "プレイヤーがジャンプする");
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("既存の候補と衝突しています"));
    }

    #[test]
    fn prompt_id_or_label_returns_same_value_as_label_for_direct_ascii_input() {
        let input = "player-jump\n";
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();

        let (id, label) =
            prompt_id_or_label(&mut reader, &mut writer, "Feature id: ", &[]).unwrap();

        assert_eq!(id, "player-jump");
        assert_eq!(label, "player-jump");
    }

    #[test]
    fn creates_new_feature_with_japanese_label_and_saves_it_to_yaml() {
        let dir = setup_migrated_requirement_root();

        run_with_input(
            dir.path(),
            "controls\nプレイヤーがジャンプする\n\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\nground\nJump from the ground and land\nDo it.\n\nlands safely\n\n\n",
        );

        let feature_path = dir
            .path()
            .join(".markharness/knowledge/features/pureiyaagajanpusuru/feature.yml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            format!(
                "id: pureiyaagajanpusuru\nrequirement_uids: [{CONTROLS_REQUIREMENT_UID}]\nlabel: プレイヤーがジャンプする\naxis: [gameplay, animation]\n"
            )
        );
    }

    #[test]
    fn creates_new_behavior_with_japanese_label_and_saves_it_to_yaml() {
        let dir = setup_migrated_requirement_root();
        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        run_with_input(
            dir.path(),
            "controls\nplayer-jump\nプレイヤーがジャンプする\n\ngameplay\nPlayer presses jump.\n\nlanding\nJump while airborne\nDo it.\n\nlands on platform\n\n\n",
        );

        let behavior_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/pureiyaagajanpusuru/behavior.yml");
        assert_eq!(
            fs::read_to_string(behavior_path).unwrap(),
            "id: pureiyaagajanpusuru\nfeature: player-jump\nlabel: プレイヤーがジャンプする\naxis: [gameplay]\ndescription: |\n  Player presses jump.\nprocedures: {}\n"
        );
    }

    #[test]
    fn creates_new_scenario_with_japanese_label_and_saves_it_to_yaml() {
        let dir = setup_migrated_requirement_root();
        run_with_input(dir.path(), SEEDED_FULL_INPUT);

        run_with_input(
            dir.path(),
            "controls\nplayer-jump\njump\nプレイヤーがジャンプする\n\nJump animation scenario\nDo it.\n\nlands on platform\n\n\n",
        );

        let scenario_path = dir.path().join(
            ".markharness/knowledge/features/player-jump/jump/pureiyaagajanpusuru/scenario.yml",
        );
        assert_eq!(
            fs::read_to_string(scenario_path).unwrap(),
            "id: pureiyaagajanpusuru\nbehavior: jump\nlabel: プレイヤーがジャンプする\ndescription: |\n  Jump animation scenario\nphases:\n  - steps:\n      - action: \"Do it.\"\n    results:\n      - \"lands on platform\"\n"
        );
    }

    #[test]
    fn creates_new_requirement_with_japanese_label_and_saves_it_to_yaml() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        // The Requirement is brand-new (uid: None until `identity migrate`
        // runs), so this first call only creates it and stops — it cannot
        // continue on to create a Feature referencing it in the same call
        // (ADR 0017 §1・§3).
        run_with_input(dir.path(), "プレイヤーがジャンプする\n\ngameplay\n");

        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/requirements/pureiyaagajanpusuru/requirement.yml");
        assert_eq!(
            fs::read_to_string(&requirement_path).unwrap(),
            "id: pureiyaagajanpusuru\nlabel: プレイヤーがジャンプする\naxis: [gameplay]\n"
        );
        assert!(!dir.path().join(".markharness/knowledge/features").exists());

        // Simulate `identity migrate` assigning a uid, then a second call
        // reuses the now-migrated Requirement (typed literally) to create a
        // Feature referencing it.
        simulate_migrate(&requirement_path, CONTROLS_REQUIREMENT_UID);
        run_with_input(
            dir.path(),
            "pureiyaagajanpusuru\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\nground\nJump from the ground and land\nDo it.\n\nlands safely\n\n\n",
        );

        let feature_path = dir
            .path()
            .join(".markharness/knowledge/features/player-jump/feature.yml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            format!(
                "id: player-jump\nrequirement_uids: [{CONTROLS_REQUIREMENT_UID}]\nlabel: player-jump\naxis: [gameplay, animation]\n"
            )
        );
    }

    #[test]
    fn reuses_existing_requirement_and_skips_axis_prompt() {
        // `controls` must already be migrated (real uid) for a Feature to
        // be created referencing it (ADR 0017 §1・§3), so it's seeded
        // directly rather than via a `run_add` call (which can no longer
        // produce a migrated Requirement on its own).
        let dir = setup_migrated_requirement_root();

        // This run reuses the requirement: no axis prompt is consumed, so
        // the second input line is the feature id, not an axis list.
        run_with_input(
            dir.path(),
            "controls\nother-feature\ngameplay\nspace\ngameplay\nPlayer presses jump while airborne.\n\nlanding\nJump while airborne\nDo it.\n\nlands on platform\n\n\n",
        );

        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/requirements/controls/requirement.yml");
        assert_eq!(
            fs::read_to_string(requirement_path).unwrap(),
            format!(
                "id: controls\nlabel: controls\naxis: [gameplay]\nuid: {CONTROLS_REQUIREMENT_UID}\n"
            )
        );
        let feature_path = dir
            .path()
            .join(".markharness/knowledge/features/other-feature/feature.yml");
        assert!(feature_path.exists());
    }

    #[test]
    fn lists_requirement_candidates_by_number_and_selects_by_index() {
        let dir = setup_migrated_requirement_root();

        let output = run_with_input_capturing_output(
            dir.path(),
            "1\nair-support\ngameplay\nspace\ngameplay\nPlayer presses jump while airborne.\n\nlanding\nJump while airborne\nDo it.\n\nlands on platform\n\n\n",
        );

        assert!(output.contains("  1) controls\n"));
        assert!(output.contains("既存のRequirement 'controls' を再利用します。"));
        let feature_path = dir
            .path()
            .join(".markharness/knowledge/features/air-support/feature.yml");
        assert!(feature_path.exists());
    }

    #[test]
    fn prompts_show_human_friendly_labels_with_examples() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        // The first call only reaches the Requirement name/axis prompts
        // (creating a brand-new Requirement stops there by design); a
        // second call, after simulating `identity migrate`, reaches the
        // rest of the chain's prompts. Concatenate both calls' output so
        // every prompt label below is covered by one test.
        let mut output = run_with_input_capturing_output(dir.path(), "controls\ngameplay\n");
        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/requirements/controls/requirement.yml");
        simulate_migrate(&requirement_path, CONTROLS_REQUIREMENT_UID);
        output.push_str(&run_with_input_capturing_output(
            dir.path(),
            SEEDED_FULL_INPUT,
        ));

        assert!(output.contains("Requirement name (e.g. task-management): "));
        assert!(output.contains("Requirement axis (comma separated, e.g. ui, validation): "));
        assert!(output.contains("Feature name (e.g. add-todo): "));
        assert!(output.contains("Axis (comma separated, e.g. ui, validation): "));
        assert!(output.contains("Behavior name (e.g. add-task): "));
        assert!(output.contains("Behavior axis (comma separated, e.g. ui, validation): "));
        assert!(output.contains("Behavior description (e.g. User adds a new task to the list.): "));
        assert!(output.contains("Procedure name (blank to finish, e.g. login): "));
        assert!(output.contains("Scenario name (e.g. empty-title): "));
        assert!(
            output
                .contains("Scenario description (e.g. Submit the todo form with an empty title): ")
        );
    }

    #[test]
    fn selecting_existing_feature_by_number_does_not_overwrite_its_label() {
        let dir = setup_migrated_requirement_root();

        run_with_input(
            dir.path(),
            "controls\nプレイヤーがジャンプする\n\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\nground\nJump from the ground and land\nDo it.\n\nlands safely\n\n\n",
        );

        let feature_path = dir
            .path()
            .join(".markharness/knowledge/features/pureiyaagajanpusuru/feature.yml");
        let before = fs::read_to_string(&feature_path).unwrap();

        run_with_input(
            dir.path(),
            "controls\n1\nair\ngameplay\nPlayer presses jump while airborne.\n\nspace\nJump while airborne\nDo it.\n\nlands on platform\n\n\n",
        );

        let after = fs::read_to_string(&feature_path).unwrap();
        assert_eq!(before, after);
        assert!(after.contains("label: プレイヤーがジャンプする"));
    }

    #[test]
    fn stripped_id_matches_a_different_preexisting_scenario_reuses_it() {
        let dir = setup_migrated_requirement_root();

        run_with_input(
            dir.path(),
            "controls\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\n\nground\nlanded on the ground\nDo it.\n\nlands safely\n\n\n",
        );

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\nplayer-jump\njump\njump-ground\n",
        );

        assert!(output.contains(
            "Scenario id 'jump-ground' から Behavior id 'jump' と重複する接頭辞を除去し、'ground' として作成します。"
        ));
        assert!(output.contains("既存のScenario 'ground' を再利用します。"));
    }
}
