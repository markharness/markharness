use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::fs_safety::replace_file;
use crate::knowledge::{
    Behavior, Condition, ExpectedResult, Feature, Requirement, contains_non_ascii, is_valid_slug,
    normalize_slug_candidate, romanize_label, serialize_behavior, serialize_condition,
    serialize_expected_result, serialize_feature, serialize_requirement,
    strip_redundant_condition_prefix,
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

/// ADR 0015 Phase 1: `steps`は1行1操作、空行で入力終了。少なくとも1つ必須。
/// stdinがEOFに達した場合、1つもstepが無ければエラーを返す(空行の繰り返し
/// 要求でハングしないように)。1つ以上あれば、空行での終了と同様にそこまで
/// 集めたstepsを返す。
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
/// ことを許す(`additional_preconditions`のような0要素許容フィールド向け)。
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

pub fn run_add<R: BufRead, W: Write>(
    root: &Path,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<()> {
    let knowledge_root = root
        .join(crate::project_root::MARKHARNESS_DIR)
        .join("knowledge");

    let requirement_candidates = list_candidate_ids(&knowledge_root, "requirement.yml");
    let (requirement_id, requirement_label) = prompt_id_or_label(
        reader,
        writer,
        "Requirement name (e.g. task-management): ",
        &requirement_candidates,
    )?;
    let requirement_dir = knowledge_root.join(&requirement_id);
    let requirement_path = requirement_dir.join("requirement.yml");
    if requirement_path.exists() {
        writeln!(
            writer,
            "既存のRequirement '{requirement_id}' を再利用します。"
        )?;
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
    }

    let feature_candidates = list_candidate_ids(&requirement_dir, "feature.yml");
    let (feature_id, feature_label) = prompt_id_or_label(
        reader,
        writer,
        "Feature name (e.g. add-todo): ",
        &feature_candidates,
    )?;
    let feature_dir = requirement_dir.join(&feature_id);
    let feature_path = feature_dir.join("feature.yml");
    if feature_path.exists() {
        writeln!(writer, "既存のFeature '{feature_id}' を再利用します。")?;
    } else {
        let axis = prompt_axis(
            reader,
            writer,
            "Axis (comma separated, e.g. ui, validation): ",
        )?;
        let feature = Feature {
            id: feature_id.clone(),
            requirement: requirement_id.clone(),
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
        let steps = prompt_steps(
            reader,
            writer,
            "Behavior steps (one operation per line, blank line to finish, e.g. Click the title field.):",
        )?;
        let behavior = Behavior {
            id: behavior_id.clone(),
            feature: feature_id.clone(),
            label: behavior_label,
            axis,
            description,
            preconditions: steps,
            uid: None,
        };
        replace_file(
            root,
            &behavior_path,
            serialize_behavior(&behavior).as_bytes(),
        )?;
    }

    let condition_candidates = list_candidate_ids(&behavior_dir, "condition.yml");
    let (raw_condition_id, condition_label) = prompt_id_or_label(
        reader,
        writer,
        "Condition name (e.g. empty-title): ",
        &condition_candidates,
    )?;
    let condition_id = {
        let raw_path = behavior_dir.join(&raw_condition_id).join("condition.yml");
        if raw_path.exists() {
            raw_condition_id
        } else if let Some(stripped) =
            strip_redundant_condition_prefix(&behavior_id, &raw_condition_id)
        {
            writeln!(
                writer,
                "Condition id '{raw_condition_id}' から Behavior id '{behavior_id}' と重複する接頭辞を除去し、'{stripped}' として作成します。"
            )?;
            stripped
        } else {
            raw_condition_id
        }
    };
    let condition_dir = behavior_dir.join(&condition_id);
    let condition_path = condition_dir.join("condition.yml");
    if condition_path.exists() {
        writeln!(writer, "既存のCondition '{condition_id}' を再利用します。")?;
    } else {
        let description = prompt_line(
            reader,
            writer,
            "Scenario (e.g. Submit the todo form with an empty title): ",
        )?;
        let steps = prompt_steps(
            reader,
            writer,
            "Condition steps (one operation per line, blank line to finish, e.g. Leave the title field empty.):",
        )?;
        let additional_preconditions = prompt_optional_steps(
            reader,
            writer,
            "Additional preconditions specific to this condition (one operation per line, blank line to finish, leave blank if none):",
        )?;
        let condition = Condition {
            id: condition_id.clone(),
            behavior: behavior_id.clone(),
            label: condition_label,
            description,
            steps,
            additional_preconditions,
            uid: None,
        };
        replace_file(
            root,
            &condition_path,
            serialize_condition(&condition).as_bytes(),
        )?;
    }

    let expected_dir = condition_dir.join("expected");
    fs::create_dir_all(&expected_dir)?;
    let existing_count = fs::read_dir(&expected_dir)?
        .filter(|entry| entry.is_ok())
        .count();
    let seq = existing_count + 1;
    let expected_id = format!("{condition_id}-{seq:03}");

    let description = prompt_line(
        reader,
        writer,
        "Expected result (e.g. shows a validation error): ",
    )?;
    let results = prompt_steps(
        reader,
        writer,
        "Observable results (one per line, blank line to finish, e.g. Shows a validation error under the input field.):",
    )?;
    let expected = ExpectedResult {
        id: expected_id,
        condition: condition_id.clone(),
        description,
        results,
        additional_steps: None,
        implementation_note: None,
        generated_by: None,
        verified_by: None,
        uid: None,
    };
    let expected_path = expected_dir.join(format!("{seq:03}.yml"));
    replace_file(
        root,
        &expected_path,
        serialize_expected_result(&expected).as_bytes(),
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

    const FULL_INPUT: &str = "controls\ngameplay\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nPress the jump button.\n\nground\nJump from the ground and land\nDo it.\n\n\nlands safely\nConfirmed.\n\n";

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

        let steps =
            prompt_optional_steps(&mut reader, &mut writer, "Additional preconditions:").unwrap();

        assert_eq!(steps, Vec::<String>::new());
    }

    #[test]
    fn prompt_optional_steps_collects_lines_until_a_blank_line() {
        let mut reader = Cursor::new(b"The character has already been deleted.\n\n".to_vec());
        let mut writer = Vec::new();

        let steps =
            prompt_optional_steps(&mut reader, &mut writer, "Additional preconditions:").unwrap();

        assert_eq!(
            steps,
            vec!["The character has already been deleted.".to_string()]
        );
    }

    #[test]
    fn creates_new_requirement_feature_behavior_condition_and_expected_from_scratch() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/controls/requirement.yml");
        let feature_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/feature.yml");
        let behavior_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/behavior.yml");
        let condition_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml");
        let expected_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml");

        assert_eq!(
            fs::read_to_string(requirement_path).unwrap(),
            "id: controls\nlabel: controls\naxis: [gameplay]\n"
        );
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay, animation]\n"
        );
        assert_eq!(
            fs::read_to_string(behavior_path).unwrap(),
            "id: jump\nfeature: player-jump\nlabel: jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\n"
        );
        assert_eq!(
            fs::read_to_string(condition_path).unwrap(),
            "id: ground\nbehavior: jump\nlabel: ground\ndescription: |\n  Jump from the ground and land\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n"
        );
        assert_eq!(
            fs::read_to_string(expected_path).unwrap(),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\n"
        );
    }

    #[test]
    fn reuses_existing_feature_and_skips_axis_prompt() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(dir.path(), FULL_INPUT);

        // Second run reuses the feature: no axis prompt is consumed, so the
        // second input line is the behavior id, not an axis list.
        run_with_input(
            dir.path(),
            "controls\nplayer-jump\nair\ngameplay\nPlayer presses jump while airborne.\nPress the jump button while airborne.\n\nspace\nJump while airborne\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        let feature_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/feature.yml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: player-jump\nrequirement: controls\nlabel: player-jump\naxis: [gameplay, animation]\n"
        );
        let behavior_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/air/behavior.yml");
        assert_eq!(
            fs::read_to_string(behavior_path).unwrap(),
            "id: air\nfeature: player-jump\nlabel: air\naxis: [gameplay]\ndescription: |\n  Player presses jump while airborne.\npreconditions:\n  - \"Press the jump button while airborne.\"\n"
        );
    }

    #[test]
    fn reuses_existing_behavior_and_skips_axis_and_description_prompt() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(dir.path(), FULL_INPUT);

        // Second run reuses feature and behavior: no axis/description prompts.
        run_with_input(
            dir.path(),
            "controls\nplayer-jump\njump\nair\nJump while airborne\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        let condition_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/air/condition.yml");
        assert_eq!(
            fs::read_to_string(condition_path).unwrap(),
            "id: air\nbehavior: jump\nlabel: air\ndescription: |\n  Jump while airborne\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n"
        );
    }

    #[test]
    fn reuses_existing_condition_and_skips_scenario_prompt() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(dir.path(), FULL_INPUT);

        // Second run reuses feature, behavior and condition: no scenario prompt.
        run_with_input(
            dir.path(),
            "controls\nplayer-jump\njump\nground\nfalls over\nConfirmed.\n\n",
        );

        let expected_002 = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/002.yml");
        assert_eq!(
            fs::read_to_string(expected_002).unwrap(),
            "id: ground-002\ncondition: ground\ndescription: |\n  falls over\nresults:\n  - \"Confirmed.\"\n"
        );
    }

    #[test]
    fn reprompts_on_empty_expected_result_input() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "controls\ngameplay\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nPress the jump button.\n\nground\nJump from the ground and land\nDo it.\n\n\n\nlands safely\nConfirmed.\n\n",
        );

        let expected_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml");
        assert_eq!(
            fs::read_to_string(expected_path).unwrap(),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\nresults:\n  - \"Confirmed.\"\n"
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
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\n1\nair\ngameplay\nPlayer presses jump while airborne.\nPress the jump button while airborne.\n\nspace\nJump while airborne\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        assert!(output.contains("  1) player-jump\n"));
        assert!(output.contains("既存のFeature 'player-jump' を再利用します。"));
        let behavior_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/air/behavior.yml");
        assert!(behavior_path.exists());
    }

    #[test]
    fn lists_behavior_candidates_by_number_and_selects_by_index() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\nplayer-jump\n1\nspace\nJump while airborne\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        assert!(output.contains("  1) jump\n"));
        assert!(output.contains("既存のBehavior 'jump' を再利用します。"));
        let condition_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/space/condition.yml");
        assert!(condition_path.exists());
    }

    #[test]
    fn lists_condition_candidates_by_number_and_selects_by_index() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\nplayer-jump\njump\n1\nfalls over\nConfirmed.\n\n",
        );

        assert!(output.contains("  1) ground\n"));
        assert!(output.contains("既存のCondition 'ground' を再利用します。"));
        let expected_002 = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/002.yml");
        assert!(expected_002.exists());
    }

    #[test]
    fn typing_literal_existing_id_with_candidates_present_still_works() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        run_with_input(
            dir.path(),
            "controls\nplayer-jump\njump\nground\nfalls over\nConfirmed.\n\n",
        );

        let expected_002 = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/002.yml");
        assert_eq!(
            fs::read_to_string(expected_002).unwrap(),
            "id: ground-002\ncondition: ground\ndescription: |\n  falls over\nresults:\n  - \"Confirmed.\"\n"
        );
    }

    #[test]
    fn auto_dedup_strips_redundant_condition_prefix_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\ngameplay\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nPress the jump button.\n\njump-ground\nJump from the ground and land\nDo it.\n\n\nlands safely\nConfirmed.\n\n",
        );

        assert!(output.contains(
            "Condition id 'jump-ground' から Behavior id 'jump' と重複する接頭辞を除去し、'ground' として作成します。"
        ));
        assert!(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/jump/ground/condition.yml")
                .exists()
        );
        assert!(
            dir.path()
                .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/001.yml")
                .exists()
        );
        assert!(
            !dir.path()
                .join(".markharness/knowledge/controls/player-jump/jump/jump-ground")
                .exists()
        );
    }

    #[test]
    fn legacy_condition_dir_with_redundant_prefix_is_reused_without_stripping() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "controls\ngameplay\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nPress the jump button.\n\njump-ground\nJump from the ground and land\nDo it.\n\n\nlands safely\nConfirmed.\n\n",
        );
        // Above run already dedupes to `ground/`; create a legacy dir with the
        // literal redundant name directly on disk to simulate pre-existing data.
        let legacy_dir = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/jump-ground");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(
            legacy_dir.join("condition.yml"),
            "id: jump-ground\nbehavior: jump\ndescription: |\n  legacy\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n",
        )
        .unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\nplayer-jump\njump\njump-ground\nfell over\nConfirmed.\n\n",
        );

        assert!(!output.contains("重複する接頭辞を除去"));
        assert!(output.contains("既存のCondition 'jump-ground' を再利用します。"));
        assert!(legacy_dir.join("expected/001.yml").exists());
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
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "controls\ngameplay\nプレイヤーがジャンプする\n\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nPress the jump button.\n\nground\nJump from the ground and land\nDo it.\n\n\nlands safely\nConfirmed.\n\n",
        );

        let feature_path = dir
            .path()
            .join(".markharness/knowledge/controls/pureiyaagajanpusuru/feature.yml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: pureiyaagajanpusuru\nrequirement: controls\nlabel: プレイヤーがジャンプする\naxis: [gameplay, animation]\n"
        );
    }

    #[test]
    fn creates_new_behavior_with_japanese_label_and_saves_it_to_yaml() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(dir.path(), FULL_INPUT);

        run_with_input(
            dir.path(),
            "controls\nplayer-jump\nプレイヤーがジャンプする\n\ngameplay\nPlayer presses jump.\nPress the jump button.\n\nlanding\nJump while airborne\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        let behavior_path = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/pureiyaagajanpusuru/behavior.yml");
        assert_eq!(
            fs::read_to_string(behavior_path).unwrap(),
            "id: pureiyaagajanpusuru\nfeature: player-jump\nlabel: プレイヤーがジャンプする\naxis: [gameplay]\ndescription: |\n  Player presses jump.\npreconditions:\n  - \"Press the jump button.\"\n"
        );
    }

    #[test]
    fn creates_new_condition_with_japanese_label_and_saves_it_to_yaml() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(dir.path(), FULL_INPUT);

        run_with_input(
            dir.path(),
            "controls\nplayer-jump\njump\nプレイヤーがジャンプする\n\nJump animation scenario\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        let condition_path = dir.path().join(
            ".markharness/knowledge/controls/player-jump/jump/pureiyaagajanpusuru/condition.yml",
        );
        assert_eq!(
            fs::read_to_string(condition_path).unwrap(),
            "id: pureiyaagajanpusuru\nbehavior: jump\nlabel: プレイヤーがジャンプする\ndescription: |\n  Jump animation scenario\nsteps:\n  - \"Do it.\"\nadditional_preconditions: []\n"
        );
    }

    #[test]
    fn creates_new_requirement_with_japanese_label_and_saves_it_to_yaml() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "プレイヤーがジャンプする\n\ngameplay\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nPress the jump button.\n\nground\nJump from the ground and land\nDo it.\n\n\nlands safely\nConfirmed.\n\n",
        );

        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/pureiyaagajanpusuru/requirement.yml");
        assert_eq!(
            fs::read_to_string(requirement_path).unwrap(),
            "id: pureiyaagajanpusuru\nlabel: プレイヤーがジャンプする\naxis: [gameplay]\n"
        );
        let feature_path = dir
            .path()
            .join(".markharness/knowledge/pureiyaagajanpusuru/player-jump/feature.yml");
        assert!(feature_path.exists());
    }

    #[test]
    fn reuses_existing_requirement_and_skips_axis_prompt() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(dir.path(), FULL_INPUT);

        // Second run reuses the requirement: no axis prompt is consumed, so
        // the second input line is the feature id, not an axis list.
        run_with_input(
            dir.path(),
            "controls\nother-feature\ngameplay\nspace\ngameplay\nPlayer presses jump while airborne.\nPress the jump button while airborne.\n\nlanding\nJump while airborne\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        let requirement_path = dir
            .path()
            .join(".markharness/knowledge/controls/requirement.yml");
        assert_eq!(
            fs::read_to_string(requirement_path).unwrap(),
            "id: controls\nlabel: controls\naxis: [gameplay]\n"
        );
        let feature_path = dir
            .path()
            .join(".markharness/knowledge/controls/other-feature/feature.yml");
        assert!(feature_path.exists());
    }

    #[test]
    fn lists_requirement_candidates_by_number_and_selects_by_index() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(dir.path(), FULL_INPUT);

        let output = run_with_input_capturing_output(
            dir.path(),
            "1\nair-support\ngameplay\nspace\ngameplay\nPlayer presses jump while airborne.\nPress the jump button while airborne.\n\nlanding\nJump while airborne\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        assert!(output.contains("  1) controls\n"));
        assert!(output.contains("既存のRequirement 'controls' を再利用します。"));
        let feature_path = dir
            .path()
            .join(".markharness/knowledge/controls/air-support/feature.yml");
        assert!(feature_path.exists());
    }

    #[test]
    fn prompts_show_human_friendly_labels_with_examples() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(dir.path(), FULL_INPUT);

        assert!(output.contains("Requirement name (e.g. task-management): "));
        assert!(output.contains("Requirement axis (comma separated, e.g. ui, validation): "));
        assert!(output.contains("Feature name (e.g. add-todo): "));
        assert!(output.contains("Axis (comma separated, e.g. ui, validation): "));
        assert!(output.contains("Behavior name (e.g. add-task): "));
        assert!(output.contains("Behavior axis (comma separated, e.g. ui, validation): "));
        assert!(output.contains("Behavior description (e.g. User adds a new task to the list.): "));
        assert!(output.contains("Condition name (e.g. empty-title): "));
        assert!(output.contains("Scenario (e.g. Submit the todo form with an empty title): "));
        assert!(output.contains("Expected result (e.g. shows a validation error): "));
    }

    #[test]
    fn selecting_existing_feature_by_number_does_not_overwrite_its_label() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "controls\ngameplay\nプレイヤーがジャンプする\n\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nPress the jump button.\n\nground\nJump from the ground and land\nDo it.\n\n\nlands safely\nConfirmed.\n\n",
        );

        let feature_path = dir
            .path()
            .join(".markharness/knowledge/controls/pureiyaagajanpusuru/feature.yml");
        let before = fs::read_to_string(&feature_path).unwrap();

        run_with_input(
            dir.path(),
            "controls\n1\nair\ngameplay\nPlayer presses jump while airborne.\nPress the jump button while airborne.\n\nspace\nJump while airborne\nDo it.\n\n\nlands on platform\nConfirmed.\n\n",
        );

        let after = fs::read_to_string(&feature_path).unwrap();
        assert_eq!(before, after);
        assert!(after.contains("label: プレイヤーがジャンプする"));
    }

    #[test]
    fn stripped_id_matches_a_different_preexisting_condition_reuses_it() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "controls\ngameplay\nplayer-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nPress the jump button.\n\nground\nlanded on the ground\nDo it.\n\n\nlands safely\nConfirmed.\n\n",
        );

        let output = run_with_input_capturing_output(
            dir.path(),
            "controls\nplayer-jump\njump\njump-ground\nfalls over\nConfirmed.\n\n",
        );

        assert!(output.contains(
            "Condition id 'jump-ground' から Behavior id 'jump' と重複する接頭辞を除去し、'ground' として作成します。"
        ));
        assert!(output.contains("既存のCondition 'ground' を再利用します。"));
        let expected_002 = dir
            .path()
            .join(".markharness/knowledge/controls/player-jump/jump/ground/expected/002.yml");
        assert!(expected_002.exists());
    }
}
