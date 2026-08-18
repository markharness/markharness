use markharness::presentation::{
    CommandOutcome, HumanPresenter, JsonPresenter, PresentedResult, Presenter,
};

#[test]
fn human_presenter_renders_generated_outcome_without_side_effects() {
    let result = HumanPresenter.present(&CommandOutcome::Generated {
        count: 2,
        written: Vec::new(),
    });

    assert_eq!(
        result,
        PresentedResult {
            stdout: "generated 2 testcase(s) into generated/testcases/\n".to_string(),
            stderr: String::new(),
            exit_code: 0,
        }
    );
}

#[test]
fn json_presenter_wraps_generated_outcome_in_versioned_contract() {
    let result = JsonPresenter.present(&CommandOutcome::Generated {
        count: 2,
        written: Vec::new(),
    });

    assert_eq!(result.exit_code, 0);
    assert_eq!(result.stderr, "");
    assert_eq!(
        result.stdout,
        "{\"generated\":2,\"ok\":true,\"outcome\":\"generated\",\"schema_version\":1,\"written\":[]}\n"
    );
}
