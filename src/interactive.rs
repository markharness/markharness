use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::knowledge::{
    Condition, ExpectedResult, Feature, contains_non_ascii, is_valid_slug,
    normalize_slug_candidate, romanize_label, serialize_condition, serialize_expected_result,
    serialize_feature, strip_redundant_condition_prefix,
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
) -> io::Result<(String, Option<String>)> {
    for (i, id) in candidates.iter().enumerate() {
        writeln!(writer, "  {}) {}", i + 1, id)?;
    }
    loop {
        let value = prompt_line(reader, writer, label)?;
        if let Ok(n) = value.parse::<usize>()
            && n >= 1
            && n <= candidates.len()
        {
            return Ok((candidates[n - 1].clone(), None));
        }
        if !contains_non_ascii(&value) {
            if is_valid_slug(&value) {
                return Ok((value, None));
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
        return Ok((final_id, Some(value)));
    }
}

pub fn run_add<R: BufRead, W: Write>(
    root: &Path,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<()> {
    let knowledge_root = root.join("knowledge");

    let feature_candidates = list_candidate_ids(&knowledge_root, "feature.yaml");
    let (feature_id, feature_label) = prompt_id_or_label(
        reader,
        writer,
        "Feature name (e.g. add-todo): ",
        &feature_candidates,
    )?;
    let feature_dir = knowledge_root.join(&feature_id);
    let feature_path = feature_dir.join("feature.yaml");
    if feature_path.exists() {
        writeln!(writer, "既存のFeature '{feature_id}' を再利用します。")?;
    } else {
        let axis_line = prompt_line(
            reader,
            writer,
            "Axis (comma separated, e.g. ui, validation): ",
        )?;
        let axis: Vec<String> = axis_line
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        fs::create_dir_all(&feature_dir)?;
        let feature = Feature {
            id: feature_id.clone(),
            axis,
            label: feature_label,
        };
        fs::write(&feature_path, serialize_feature(&feature))?;
    }

    let condition_candidates = list_candidate_ids(&feature_dir, "condition.yaml");
    let (raw_condition_id, condition_label) = prompt_id_or_label(
        reader,
        writer,
        "Condition name (e.g. empty-title): ",
        &condition_candidates,
    )?;
    let condition_id = {
        let raw_path = feature_dir.join(&raw_condition_id).join("condition.yaml");
        if raw_path.exists() {
            raw_condition_id
        } else if let Some(stripped) =
            strip_redundant_condition_prefix(&feature_id, &raw_condition_id)
        {
            writeln!(
                writer,
                "Condition id '{raw_condition_id}' から Feature id '{feature_id}' と重複する接頭辞を除去し、'{stripped}' として作成します。"
            )?;
            stripped
        } else {
            raw_condition_id
        }
    };
    let condition_dir = feature_dir.join(&condition_id);
    let condition_path = condition_dir.join("condition.yaml");
    if condition_path.exists() {
        writeln!(writer, "既存のCondition '{condition_id}' を再利用します。")?;
    } else {
        let summary = prompt_line(
            reader,
            writer,
            "Scenario (e.g. Submit the todo form with an empty title): ",
        )?;
        fs::create_dir_all(&condition_dir)?;
        let condition = Condition {
            id: condition_id.clone(),
            summary,
            label: condition_label,
        };
        fs::write(&condition_path, serialize_condition(&condition))?;
    }

    let expected_dir = condition_dir.join("expected");
    fs::create_dir_all(&expected_dir)?;
    let existing_count = fs::read_dir(&expected_dir)?
        .filter(|entry| entry.is_ok())
        .count();
    let seq = existing_count + 1;
    let expected_id = format!("{feature_id}-{condition_id}-{seq:03}");

    let result = prompt_line(
        reader,
        writer,
        "Expected result (e.g. shows a validation error): ",
    )?;
    let expected = ExpectedResult {
        id: expected_id,
        result,
        label: None,
    };
    let expected_path = expected_dir.join(format!("{seq:03}.yaml"));
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

    #[test]
    fn creates_new_feature_condition_and_expected_from_scratch() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        let feature_path = dir.path().join("knowledge/player-jump/feature.yaml");
        let condition_path = dir
            .path()
            .join("knowledge/player-jump/jump-ground/condition.yaml");
        let expected_path = dir
            .path()
            .join("knowledge/player-jump/jump-ground/expected/001.yaml");

        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: player-jump\nkind: feature\naxis:\n  - gameplay\n  - animation\n"
        );
        assert_eq!(
            fs::read_to_string(condition_path).unwrap(),
            "id: jump-ground\nkind: condition\nsummary: Jump from the ground and land\n"
        );
        assert_eq!(
            fs::read_to_string(expected_path).unwrap(),
            "id: player-jump-jump-ground-001\nkind: expected-result\nresult: lands safely\n"
        );
    }

    #[test]
    fn reuses_existing_feature_and_skips_axis_prompt() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        // Second run reuses the feature: no axis prompt is consumed, so the
        // second input line is the condition id, not an axis list.
        run_with_input(
            dir.path(),
            "player-jump\njump-air\nJump while airborne\nlands on platform\n",
        );

        let feature_path = dir.path().join("knowledge/player-jump/feature.yaml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: player-jump\nkind: feature\naxis:\n  - gameplay\n  - animation\n"
        );
        let condition_path = dir
            .path()
            .join("knowledge/player-jump/jump-air/condition.yaml");
        assert_eq!(
            fs::read_to_string(condition_path).unwrap(),
            "id: jump-air\nkind: condition\nsummary: Jump while airborne\n"
        );
    }

    #[test]
    fn reuses_existing_condition_and_skips_summary_prompt() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();
        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        // Second run reuses feature and condition: no axis/summary prompts.
        run_with_input(dir.path(), "player-jump\njump-ground\nfalls over\n");

        let expected_002 = dir
            .path()
            .join("knowledge/player-jump/jump-ground/expected/002.yaml");
        assert_eq!(
            fs::read_to_string(expected_002).unwrap(),
            "id: player-jump-jump-ground-002\nkind: expected-result\nresult: falls over\n"
        );
    }

    #[test]
    fn reprompts_on_empty_expected_result_input() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\n\nlands safely\n",
        );

        let expected_path = dir
            .path()
            .join("knowledge/player-jump/jump-ground/expected/001.yaml");
        assert_eq!(
            fs::read_to_string(expected_path).unwrap(),
            "id: player-jump-jump-ground-001\nkind: expected-result\nresult: lands safely\n"
        );
    }

    #[test]
    fn no_candidate_list_printed_for_fresh_knowledge_dir() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        assert!(!output.contains("1)"));
    }

    #[test]
    fn lists_feature_candidates_by_number_and_selects_by_index() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        let output = run_with_input_capturing_output(
            dir.path(),
            "1\njump-air\nJump while airborne\nlands on platform\n",
        );

        assert!(output.contains("  1) player-jump\n"));
        assert!(output.contains("既存のFeature 'player-jump' を再利用します。"));
        let condition_path = dir
            .path()
            .join("knowledge/player-jump/jump-air/condition.yaml");
        assert!(condition_path.exists());
    }

    #[test]
    fn lists_condition_candidates_by_number_and_selects_by_index() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        let output = run_with_input_capturing_output(dir.path(), "player-jump\n1\nfalls over\n");

        assert!(output.contains("  1) jump-ground\n"));
        assert!(output.contains("既存のCondition 'jump-ground' を再利用します。"));
        let expected_002 = dir
            .path()
            .join("knowledge/player-jump/jump-ground/expected/002.yaml");
        assert!(expected_002.exists());
    }

    #[test]
    fn typing_literal_existing_id_with_candidates_present_still_works() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        run_with_input(dir.path(), "player-jump\njump-ground\nfalls over\n");

        let expected_002 = dir
            .path()
            .join("knowledge/player-jump/jump-ground/expected/002.yaml");
        assert_eq!(
            fs::read_to_string(expected_002).unwrap(),
            "id: player-jump-jump-ground-002\nkind: expected-result\nresult: falls over\n"
        );
    }

    #[test]
    fn auto_dedup_strips_redundant_condition_prefix_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\ngameplay, animation\nplayer-jump-ground\nJump from the ground and land\nlands safely\n",
        );

        assert!(output.contains(
            "Condition id 'player-jump-ground' から Feature id 'player-jump' と重複する接頭辞を除去し、'ground' として作成します。"
        ));
        assert!(
            dir.path()
                .join("knowledge/player-jump/ground/condition.yaml")
                .exists()
        );
        assert!(
            dir.path()
                .join("knowledge/player-jump/ground/expected/001.yaml")
                .exists()
        );
        assert!(
            !dir.path()
                .join("knowledge/player-jump/player-jump-ground")
                .exists()
        );
    }

    #[test]
    fn legacy_condition_dir_with_redundant_prefix_is_reused_without_stripping() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\nplayer-jump-ground\nJump from the ground and land\nlands safely\n",
        );
        // Above run already dedupes to `ground/`; create a legacy dir with the
        // literal redundant name directly on disk to simulate pre-existing data.
        let legacy_dir = dir.path().join("knowledge/player-jump/player-jump-ground");
        fs::create_dir_all(&legacy_dir).unwrap();
        fs::write(
            legacy_dir.join("condition.yaml"),
            "id: player-jump-ground\nkind: condition\nsummary: legacy\n",
        )
        .unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\nplayer-jump-ground\nfell over\n",
        );

        assert!(!output.contains("重複する接頭辞を除去"));
        assert!(output.contains("既存のCondition 'player-jump-ground' を再利用します。"));
        assert!(legacy_dir.join("expected/001.yaml").exists());
    }

    #[test]
    fn prompt_id_or_label_suggests_romanized_slug_and_accepts_on_empty_input() {
        let input = "プレイヤーがジャンプする\n\n";
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();

        let (id, label) =
            prompt_id_or_label(&mut reader, &mut writer, "Feature id: ", &[]).unwrap();

        assert_eq!(id, "pureiyaa-ga-janpu-suru");
        assert_eq!(label, Some("プレイヤーがジャンプする".to_string()));
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
        assert_eq!(label, Some("プレイヤーがジャンプする".to_string()));
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
        assert_eq!(label, Some("プレイヤーがジャンプする".to_string()));
        let output = String::from_utf8(writer).unwrap();
        assert!(output.contains("既存の候補と衝突しています"));
    }

    #[test]
    fn prompt_id_or_label_returns_none_label_for_direct_ascii_input() {
        let input = "player-jump\n";
        let mut reader = Cursor::new(input.as_bytes());
        let mut writer = Vec::new();

        let (id, label) =
            prompt_id_or_label(&mut reader, &mut writer, "Feature id: ", &[]).unwrap();

        assert_eq!(id, "player-jump");
        assert_eq!(label, None);
    }

    #[test]
    fn creates_new_feature_with_japanese_label_and_saves_it_to_yaml() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "プレイヤーがジャンプする\n\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        let feature_path = dir
            .path()
            .join("knowledge/pureiyaa-ga-janpu-suru/feature.yaml");
        assert_eq!(
            fs::read_to_string(feature_path).unwrap(),
            "id: pureiyaa-ga-janpu-suru\nlabel: プレイヤーがジャンプする\nkind: feature\naxis:\n  - gameplay\n  - animation\n"
        );
    }

    #[test]
    fn creates_new_condition_with_japanese_label_and_saves_it_to_yaml() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        run_with_input(
            dir.path(),
            "player-jump\ngameplay, animation\n地上からジャンプ\n\nJump from the ground and land\nlands safely\n",
        );

        let condition_path = dir
            .path()
            .join("knowledge/player-jump/chijou-kara-janpu/condition.yaml");
        assert_eq!(
            fs::read_to_string(condition_path).unwrap(),
            "id: chijou-kara-janpu\nlabel: 地上からジャンプ\nkind: condition\nsummary: Jump from the ground and land\n"
        );
    }

    #[test]
    fn prompts_show_human_friendly_labels_with_examples() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path()).unwrap();

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        assert!(output.contains("Feature name (e.g. add-todo): "));
        assert!(output.contains("Axis (comma separated, e.g. ui, validation): "));
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
            "プレイヤーがジャンプする\n\ngameplay, animation\njump-ground\nJump from the ground and land\nlands safely\n",
        );

        let feature_path = dir
            .path()
            .join("knowledge/pureiyaa-ga-janpu-suru/feature.yaml");
        let before = fs::read_to_string(&feature_path).unwrap();

        // Select the existing feature by number; a numeric selection always
        // yields label = None, so this also proves the None doesn't overwrite
        // the file (the exists() guard skips the write entirely).
        run_with_input(
            dir.path(),
            "1\njump-air\nJump while airborne\nlands on platform\n",
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
            "player-jump\ngameplay, animation\nground\nlanded on the ground\nlands safely\n",
        );

        let output = run_with_input_capturing_output(
            dir.path(),
            "player-jump\nplayer-jump-ground\nfalls over\n",
        );

        assert!(output.contains(
            "Condition id 'player-jump-ground' から Feature id 'player-jump' と重複する接頭辞を除去し、'ground' として作成します。"
        ));
        assert!(output.contains("既存のCondition 'ground' を再利用します。"));
        let expected_002 = dir
            .path()
            .join("knowledge/player-jump/ground/expected/002.yaml");
        assert!(expected_002.exists());
    }
}
