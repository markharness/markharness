use std::fs;
use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::knowledge::{
    Condition, ExpectedResult, Feature, is_valid_slug, serialize_condition,
    serialize_expected_result, serialize_feature,
};

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

fn prompt_slug<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    label: &str,
) -> io::Result<String> {
    loop {
        let value = prompt_line(reader, writer, label)?;
        if is_valid_slug(&value) {
            return Ok(value);
        }
        writeln!(
            writer,
            "id は小文字英数字とハイフンのみ使用できます。もう一度入力してください。"
        )?;
    }
}

pub fn run_add<R: BufRead, W: Write>(
    root: &Path,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<()> {
    let knowledge_root = root.join("knowledge");

    let feature_id = prompt_slug(reader, writer, "Feature id: ")?;
    let feature_dir = knowledge_root.join(&feature_id);
    let feature_path = feature_dir.join("feature.yaml");
    if feature_path.exists() {
        writeln!(writer, "既存のFeature '{feature_id}' を再利用します。")?;
    } else {
        let axis_line = prompt_line(reader, writer, "Axis (comma separated): ")?;
        let axis: Vec<String> = axis_line
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        fs::create_dir_all(&feature_dir)?;
        let feature = Feature {
            id: feature_id.clone(),
            axis,
        };
        fs::write(&feature_path, serialize_feature(&feature))?;
    }

    let condition_id = prompt_slug(reader, writer, "Condition id: ")?;
    let condition_dir = feature_dir.join(&condition_id);
    let condition_path = condition_dir.join("condition.yaml");
    if condition_path.exists() {
        writeln!(writer, "既存のCondition '{condition_id}' を再利用します。")?;
    } else {
        let summary = prompt_line(reader, writer, "Summary: ")?;
        fs::create_dir_all(&condition_dir)?;
        let condition = Condition {
            id: condition_id.clone(),
            summary,
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

    let result = prompt_line(reader, writer, "Expected result: ")?;
    let expected = ExpectedResult {
        id: expected_id,
        result,
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

    #[test]
    fn creates_new_feature_condition_and_expected_from_scratch() {
        let dir = tempfile::tempdir().unwrap();
        crate::init::run_init(dir.path(), false).unwrap();

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
        crate::init::run_init(dir.path(), false).unwrap();
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
        crate::init::run_init(dir.path(), false).unwrap();
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
        crate::init::run_init(dir.path(), false).unwrap();

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
}
