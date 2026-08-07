use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::knowledge::{
    Behavior, Condition, ExpectedResult, Feature, contains_non_ascii, is_valid_slug,
    normalize_slug_candidate, romanize_label, serialize_behavior, serialize_condition,
    serialize_expected_result, serialize_feature, strip_redundant_condition_prefix,
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

pub fn run_add<R: BufRead, W: Write>(
    root: &Path,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<()> {
    let knowledge_root = root.join("knowledge");

    let feature_candidates = list_candidate_ids(&knowledge_root, "feature.yml");
    let (feature_id, feature_label) = prompt_id_or_label(
        reader,
        writer,
        "Feature name (e.g. add-todo): ",
        &feature_candidates,
    )?;
    let feature_dir = knowledge_root.join(&feature_id);
    let feature_path = feature_dir.join("feature.yml");
    if feature_path.exists() {
        writeln!(writer, "既存のFeature '{feature_id}' を再利用します。")?;
    } else {
        let axis = prompt_axis(
            reader,
            writer,
            "Axis (comma separated, e.g. ui, validation): ",
        )?;
        fs::create_dir_all(&feature_dir)?;
        let feature = Feature {
            id: feature_id.clone(),
            label: feature_label,
            axis,
            description: None,
        };
        fs::write(&feature_path, serialize_feature(&feature))?;
    }

    let behavior_candidates = list_candidate_ids(&feature_dir, "behavior.yml");
    let (behavior_id, _behavior_label) = prompt_id_or_label(
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
        fs::create_dir_all(&behavior_dir)?;
        let behavior = Behavior {
            id: behavior_id.clone(),
            feature: feature_id.clone(),
            axis,
            description,
        };
        fs::write(&behavior_path, serialize_behavior(&behavior))?;
    }

    let condition_candidates = list_candidate_ids(&behavior_dir, "condition.yml");
    let (raw_condition_id, _condition_label) = prompt_id_or_label(
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
        fs::create_dir_all(&condition_dir)?;
        let condition = Condition {
            id: condition_id.clone(),
            behavior: behavior_id.clone(),
            description,
        };
        fs::write(&condition_path, serialize_condition(&condition))?;
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
    let expected = ExpectedResult {
        id: expected_id,
        condition: condition_id.clone(),
        description,
    };
    let expected_path = expected_dir.join(format!("{seq:03}.yml"));
    fs::write(&expected_path, serialize_expected_result(&expected))?;

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

    const FULL_INPUT: &str = "player-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nground\nJump from the ground and land\nlands safely\n";

    #[test]
    fn creates_new_feature_behavior_condition_and_expected_from_scratch() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        let feature_path = dir.path().join("knowledge/player-jump/feature.yml");
        let behavior_path = dir.path().join("knowledge/player-jump/jump/behavior.yml");
        let condition_path = dir
            .path()
            .join("knowledge/player-jump/jump/ground/condition.yml");
        let expected_path = dir
            .path()
            .join("knowledge/player-jump/jump/ground/expected/001.yml");

        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: player-jump\nlabel: player-jump\naxis: [gameplay, animation]\n"
        );
        assert_eq!(
            fs::read_to_string(behavior_path).unwrap(),
            "id: jump\nfeature: player-jump\naxis: [gameplay]\ndescription: |\n  Player presses jump.\n"
        );
        assert_eq!(
            fs::read_to_string(condition_path).unwrap(),
            "id: ground\nbehavior: jump\ndescription: |\n  Jump from the ground and land\n"
        );
        assert_eq!(
            fs::read_to_string(expected_path).unwrap(),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\n"
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
            "player-jump\nair\ngameplay\nPlayer presses jump while airborne.\nspace\nJump while airborne\nlands on platform\n",
        );

        let feature_path = dir.path().join("knowledge/player-jump/feature.yml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: player-jump\nlabel: player-jump\naxis: [gameplay, animation]\n"
        );
        let behavior_path = dir.path().join("knowledge/player-jump/air/behavior.yml");
        assert_eq!(
            fs::read_to_string(behavior_path).unwrap(),
            "id: air\nfeature: player-jump\naxis: [gameplay]\ndescription: |\n  Player presses jump while airborne.\n"
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
            "player-jump\njump\nair\nJump while airborne\nlands on platform\n",
        );

        let condition_path = dir
            .path()
            .join("knowledge/player-jump/jump/air/condition.yml");
        assert_eq!(
            fs::read_to_string(condition_path).unwrap(),
            "id: air\nbehavior: jump\ndescription: |\n  Jump while airborne\n"
        );
    }

    #[test]
    fn reuses_existing_condition_and_skips_scenario_prompt() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(dir.path(), FULL_INPUT);

        // Second run reuses feature, behavior and condition: no scenario prompt.
        run_with_input(dir.path(), "player-jump\njump\nground\nfalls over\n");

        let expected_002 = dir
            .path()
            .join("knowledge/player-jump/jump/ground/expected/002.yml");
        assert_eq!(
            fs::read_to_string(expected_002).unwrap(),
            "id: ground-002\ncondition: ground\ndescription: |\n  falls over\n"
        );
    }

    #[test]
    fn reprompts_on_empty_expected_result_input() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nground\nJump from the ground and land\n\nlands safely\n",
        );

        let expected_path = dir
            .path()
            .join("knowledge/player-jump/jump/ground/expected/001.yml");
        assert_eq!(
            fs::read_to_string(expected_path).unwrap(),
            "id: ground-001\ncondition: ground\ndescription: |\n  lands safely\n"
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
            "1\nair\ngameplay\nPlayer presses jump while airborne.\nspace\nJump while airborne\nlands on platform\n",
        );

        assert!(output.contains("  1) player-jump\n"));
        assert!(output.contains("既存のFeature 'player-jump' を再利用します。"));
        let behavior_path = dir.path().join("knowledge/player-jump/air/behavior.yml");
        assert!(behavior_path.exists());
    }

    #[test]
    fn lists_behavior_candidates_by_number_and_selects_by_index() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\n1\nspace\nJump while airborne\nlands on platform\n",
        );

        assert!(output.contains("  1) jump\n"));
        assert!(output.contains("既存のBehavior 'jump' を再利用します。"));
        let condition_path = dir
            .path()
            .join("knowledge/player-jump/jump/space/condition.yml");
        assert!(condition_path.exists());
    }

    #[test]
    fn lists_condition_candidates_by_number_and_selects_by_index() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        let output =
            run_with_input_capturing_output(dir.path(), "player-jump\njump\n1\nfalls over\n");

        assert!(output.contains("  1) ground\n"));
        assert!(output.contains("既存のCondition 'ground' を再利用します。"));
        let expected_002 = dir
            .path()
            .join("knowledge/player-jump/jump/ground/expected/002.yml");
        assert!(expected_002.exists());
    }

    #[test]
    fn typing_literal_existing_id_with_candidates_present_still_works() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(dir.path(), FULL_INPUT);

        run_with_input(dir.path(), "player-jump\njump\nground\nfalls over\n");

        let expected_002 = dir
            .path()
            .join("knowledge/player-jump/jump/ground/expected/002.yml");
        assert_eq!(
            fs::read_to_string(expected_002).unwrap(),
            "id: ground-002\ncondition: ground\ndescription: |\n  falls over\n"
        );
    }

    #[test]
    fn auto_dedup_strips_redundant_condition_prefix_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        assert!(output.contains(
            "Condition id 'jump-ground' から Behavior id 'jump' と重複する接頭辞を除去し、'ground' として作成します。"
        ));
        assert!(
            dir.path()
                .join("knowledge/player-jump/jump/ground/condition.yml")
                .exists()
        );
        assert!(
            dir.path()
                .join("knowledge/player-jump/jump/ground/expected/001.yml")
                .exists()
        );
        assert!(
            !dir.path()
                .join("knowledge/player-jump/jump/jump-ground")
                .exists()
        );
    }

    #[test]
    fn legacy_condition_dir_with_redundant_prefix_is_reused_without_stripping() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\njump-ground\nJump from the ground and land\nlands safely\n",
        );
        // Above run already dedupes to `ground/`; create a legacy dir with the
        // literal redundant name directly on disk to simulate pre-existing data.
        let legacy_dir = dir.path().join("knowledge/player-jump/jump/jump-ground");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(
            legacy_dir.join("condition.yml"),
            "id: jump-ground\nbehavior: jump\ndescription: |\n  legacy\n",
        )
        .unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\njump\njump-ground\nfell over\n",
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

        assert_eq!(id, "pureiyaa-ga-janpu-suru");
        assert_eq!(label, "プレイヤーがジャンプする");
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("pureiyaa-ga-janpu-suru"));
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
        let candidates = vec!["pureiyaa-ga-janpu-suru".to_string()];
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
            "プレイヤーがジャンプする\n\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nground\nJump from the ground and land\nlands safely\n",
        );

        let feature_path = dir
            .path()
            .join("knowledge/pureiyaa-ga-janpu-suru/feature.yml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: pureiyaa-ga-janpu-suru\nlabel: プレイヤーがジャンプする\naxis: [gameplay, animation]\n"
        );
    }

    #[test]
    fn prompts_show_human_friendly_labels_with_examples() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(dir.path(), FULL_INPUT);

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
            "プレイヤーがジャンプする\n\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nground\nJump from the ground and land\nlands safely\n",
        );

        let feature_path = dir
            .path()
            .join("knowledge/pureiyaa-ga-janpu-suru/feature.yml");
        let before = fs::read_to_string(&feature_path).unwrap();

        run_with_input(
            dir.path(),
            "1\nair\ngameplay\nPlayer presses jump while airborne.\nspace\nJump while airborne\nlands on platform\n",
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
            "player-jump\ngameplay, animation\njump\ngameplay\nPlayer presses jump.\nground\nlanded on the ground\nlands safely\n",
        );

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\njump\njump-ground\nfalls over\n",
        );

        assert!(output.contains(
            "Condition id 'jump-ground' から Behavior id 'jump' と重複する接頭辞を除去し、'ground' として作成します。"
        ));
        assert!(output.contains("既存のCondition 'ground' を再利用します。"));
        let expected_002 = dir
            .path()
            .join("knowledge/player-jump/jump/ground/expected/002.yml");
        assert!(expected_002.exists());
    }
}
